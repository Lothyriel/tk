//! Original TK presentation anchors and action curves. Never grants gameplay actions.
use crate::WeaponKind;
use bevy::prelude::*;

pub mod m4a1;
pub mod manipulation;

#[derive(Debug, Clone, Copy)]
pub struct WeaponRig {
    pub muzzle: Vec3,
    pub trigger: Vec3,
    pub support: Vec3,
    pub magazine: Vec3,
    pub bolt_travel: f32,
}

impl WeaponRig {
    pub fn for_weapon(weapon: WeaponKind) -> Self {
        match weapon {
            WeaponKind::Pistol => Self {
                muzzle: Vec3::new(0.0, 0.055, -0.245),
                trigger: Vec3::new(0.025, -0.085, 0.015),
                support: Vec3::new(-0.045, -0.085, -0.015),
                magazine: Vec3::new(0.0, -0.11, 0.025),
                bolt_travel: 0.045,
            },
            WeaponKind::Smg => Self {
                muzzle: Vec3::new(0.0, 0.065, -0.46),
                trigger: Vec3::new(0.025, -0.09, 0.035),
                support: Vec3::new(-0.025, 0.0, -0.255),
                magazine: Vec3::new(0.0, -0.14, -0.095),
                bolt_travel: 0.065,
            },
            WeaponKind::Rifle => Self {
                muzzle: Vec3::new(0.0, 0.075, -0.70),
                trigger: Vec3::new(0.025, -0.09, 0.035),
                support: Vec3::new(-0.025, 0.005, -0.35),
                magazine: Vec3::new(0.0, -0.13, -0.105),
                bolt_travel: 0.055,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WeaponAction {
    pub root: Transform,
    pub support: Transform,
    /// Ordered contact blends: authored grip -> moving magazine -> control.
    /// Presentation filters these weights with the mechanisms, not wrist positions.
    pub magazine_grip: f32,
    pub control_grip: f32,
    pub magazine: Transform,
    pub magazine_visible: bool,
    pub bolt: f32,
    pub control: f32,
    pub latch: f32,
}

pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Eye-relative iron-sight placement in metres, shared with authoritative barrel origin.
pub fn ads_offset(weapon: WeaponKind) -> Vec3 {
    match weapon {
        WeaponKind::Pistol => Vec3::new(0.0, -0.118, -0.48),
        WeaponKind::Smg => Vec3::new(0.0, -0.136, -0.24),
        WeaponKind::Rifle => Vec3::new(0.0, -0.175, -0.24),
    }
}

pub fn barrel_offset(weapon: WeaponKind, ads: f32) -> Vec3 {
    resolved_barrel_offset(weapon.spec(), ads)
}

pub fn resolved_barrel_offset(spec: &crate::catalog::ResolvedWeapon, ads: f32) -> Vec3 {
    if let Some(bore) = spec.assembly.anchors.get(&crate::catalog::AnchorKind::Bore) {
        return configured_offset(spec, bore.translation, ads);
    }
    let weapon = spec.class;
    Vec3::from(spec.barrel_offset).lerp(
        ads_offset(weapon) + WeaponRig::for_weapon(weapon).muzzle,
        ease(ads),
    )
}

/// Flight starts at the installed device outlet, or at the bare bore if there
/// is no authored outlet. Clearance still covers the entire eye-to-outlet path.
pub fn resolved_muzzle_offset(spec: &crate::catalog::ResolvedWeapon, ads: f32) -> Vec3 {
    match spec
        .assembly
        .anchors
        .get(&crate::catalog::AnchorKind::DeviceExit)
    {
        Some(exit) => configured_offset(spec, exit.translation, ads),
        None => resolved_barrel_offset(spec, ads),
    }
}

/// Installed outlet in configured weapon-root coordinates. Legacy assemblies
/// without authored bore sockets use the same class anchor as their renderer.
pub fn configured_outlet(spec: &crate::catalog::ResolvedWeapon) -> Transform {
    spec.assembly
        .anchors
        .get(&crate::catalog::AnchorKind::DeviceExit)
        .or_else(|| spec.assembly.anchors.get(&crate::catalog::AnchorKind::Bore))
        .copied()
        .unwrap_or_else(|| Transform::from_translation(WeaponRig::for_weapon(spec.class).muzzle))
}

/// Bore orientation is independent from the outlet of an installed device.
pub fn configured_bore(spec: &crate::catalog::ResolvedWeapon) -> Transform {
    spec.assembly
        .anchors
        .get(&crate::catalog::AnchorKind::Bore)
        .copied()
        .unwrap_or_else(|| Transform::from_translation(WeaponRig::for_weapon(spec.class).muzzle))
}

fn configured_offset(spec: &crate::catalog::ResolvedWeapon, point: Vec3, ads: f32) -> Vec3 {
    let assembly = &spec.assembly;
    assembly
        .hip_from_root
        .transform_point(point)
        .lerp(assembly.ads_from_root.transform_point(point), ease(ads))
}

pub fn resolved_ejection_offset(spec: &crate::catalog::ResolvedWeapon, ads: f32) -> Option<Vec3> {
    let assembly = &spec.assembly;
    let port = assembly
        .anchors
        .get(&crate::catalog::AnchorKind::Ejection)?;
    Some(
        assembly
            .hip_from_root
            .transform_point(port.translation)
            .lerp(
                assembly.ads_from_root.transform_point(port.translation),
                ease(ads),
            ),
    )
}

fn ramp(t: f32, a: f32, b: f32) -> f32 {
    ease((t - a) / (b - a))
}

/// Piecewise contact choreography. The server's reload fraction is the only clock.
/// Tactical: retain chamber, seat magazine, regrip. Empty: add slide/handle release.
pub fn reload_action(weapon: WeaponKind, fraction: f32, empty: bool) -> WeaponAction {
    if weapon == WeaponKind::Rifle {
        return m4a1::reload(fraction, empty);
    }
    let rig = WeaponRig::for_weapon(weapon);
    let t = fraction.clamp(0.0, 1.0);
    let (out, incoming, seat, release) = match weapon {
        WeaponKind::Pistol => (0.22, 0.46, 0.68, 0.87),
        WeaponKind::Smg => (0.28, 0.48, 0.70, 0.90),
        WeaponKind::Rifle => (0.25, 0.52, 0.74, 0.90),
    };
    let mut bolt = if empty {
        (if weapon == WeaponKind::Smg {
            ramp(t, 0.04, 0.12)
        } else {
            1.0
        }) * (1.0 - ramp(t, release - 0.02, release + 0.025))
    } else {
        0.0
    };
    let latch = if empty && weapon == WeaponKind::Smg {
        ramp(t, 0.10, 0.15) * (1.0 - ramp(t, release - 0.025, release + 0.01))
    } else {
        0.0
    };
    let present = ramp(t, 0.0, 0.13) * (1.0 - ramp(t, release, 1.0));
    let withdraw = ramp(t, 0.12, out);
    let insert = ramp(t, incoming, seat);
    let mag_drop = (withdraw - insert).max(0.0);
    let magazine = Transform::from_translation(
        rig.magazine + Vec3::new(-mag_drop * 0.035, -mag_drop * 0.24, mag_drop * 0.045),
    )
    .with_rotation(Quat::from_rotation_x(mag_drop * -0.15));
    let reach = ramp(t, 0.04, 0.12) * (1.0 - ramp(t, seat, seat + 0.08));
    // The support palm follows the magazine during extraction and insertion, not an unrelated sine.
    let contact = magazine.translation + Vec3::new(-0.045, -0.025, 0.025);
    let mut hand = rig.support.lerp(contact, reach);
    let manipulate = if empty {
        ramp(t, seat - 0.01, release - 0.075) * (1.0 - ramp(t, release + 0.015, 1.0))
    } else {
        0.0
    };
    if empty && weapon == WeaponKind::Pistol {
        // Slingshot the already locked slide a little farther rearward before
        // releasing it; an overhand gesture must not grasp a motionless slide.
        bolt *= 1.0
            + 0.22
                * ramp(t, seat + 0.02, release - 0.06)
                * (1.0 - ramp(t, release - 0.025, release));
    }
    let release_point = match weapon {
        // Overhand slide release, HK handle slap, and the AR bolt catch are
        // different contacts. A tactical magazine change never racks a chamber.
        WeaponKind::Pistol => Vec3::new(-0.045, 0.118, -0.07 + rig.bolt_travel * bolt),
        WeaponKind::Smg => {
            Vec3::new(0.0, 0.08, -0.22)
                + Quat::from_rotation_z(-latch * 0.7) * Vec3::new(-0.05, 0.0, 0.0)
                + Vec3::Z * rig.bolt_travel * bolt
                + Vec3::new(-0.02, 0.0, 0.005)
        }
        WeaponKind::Rifle => Vec3::new(-0.067, 0.055, -0.085),
    };
    hand = hand.lerp(release_point, manipulate);
    let charge = if weapon == WeaponKind::Smg && empty {
        ramp(t, 0.0, 0.045) * (1.0 - ramp(t, 0.12, 0.18))
    } else {
        0.0
    };
    if charge > 0.0 {
        hand = hand.lerp(release_point, charge);
    }
    let (pitch, yaw, roll) = match weapon {
        // Present the free side of the slide to the eye. The previous outward
        // cant lined the whole supporting sleeve up over the action.
        WeaponKind::Pistol => (0.12, -0.65, 0.18),
        WeaponKind::Smg => (0.15, -0.22, -0.34),
        WeaponKind::Rifle => (0.10, -0.22, -0.30),
    };
    WeaponAction {
        magazine_grip: reach,
        control_grip: 1.0 - (1.0 - manipulate) * (1.0 - charge),
        root: Transform::from_translation(
            Vec3::new(
                if weapon == WeaponKind::Pistol {
                    -0.20
                } else {
                    -0.015
                },
                -0.045,
                0.055,
            ) * present,
        )
        .with_rotation(Quat::from_euler(
            EulerRot::XYZ,
            pitch * present,
            yaw * present,
            roll * present,
        )),
        support: Transform::from_translation(hand).with_rotation(
            Quat::from_rotation_z(-0.3 * reach).slerp(
                match weapon {
                    // Fingers cross the slide; the wrist exits laterally toward
                    // the supporting elbow, not rearward over the visible rail.
                    WeaponKind::Pistol => {
                        Quat::from_euler(EulerRot::XYZ, 0.0, -std::f32::consts::FRAC_PI_2, -0.35)
                    }
                    WeaponKind::Smg => Quat::from_rotation_z(-0.35),
                    WeaponKind::Rifle => Quat::from_euler(EulerRot::XYZ, 0.0, -0.70, 0.15),
                },
                manipulate,
            ),
        ),
        magazine,
        magazine_visible: !(out + 0.025..incoming).contains(&t),
        bolt,
        control: if empty && weapon != WeaponKind::Smg {
            ramp(t, release - 0.07, release - 0.015)
                * (1.0 - ramp(t, release + 0.01, release + 0.06))
        } else {
            0.0
        },
        latch,
    }
}

pub fn bolt_offset(weapon: WeaponKind, action: &WeaponAction, recoil: f32, reloading: bool) -> f32 {
    let rig = WeaponRig::for_weapon(weapon);
    if weapon == WeaponKind::Smg {
        // MP5 charging handle does not reciprocate with automatic fire or lock on empty.
        rig.bolt_travel * if reloading { action.bolt } else { 0.0 }
    } else {
        rig.bolt_travel * action.bolt.max((recoil / 0.055).min(1.0))
    }
}

pub fn bolt_transform(
    weapon: WeaponKind,
    action: &WeaponAction,
    recoil: f32,
    reloading: bool,
) -> Transform {
    // Presentation actions are filtered from the last displayed pose. Retain a
    // trailing locked handle through cancellation rather than snapping to idle
    // when authoritative reload_remaining becomes zero (both perspectives).
    let settling = reloading || action.bolt > 0.001 || action.latch > 0.001;
    let mut t =
        Transform::from_translation(Vec3::Z * bolt_offset(weapon, action, recoil, settling));
    if weapon == WeaponKind::Smg && settling {
        let pivot = Vec3::new(0.0, 0.08, -0.22);
        t.rotation = Quat::from_rotation_z(-action.latch * 0.7);
        t.translation += pivot - t.rotation * pivot;
    }
    t
}

pub fn control_transform(weapon: WeaponKind, action: &WeaponAction) -> Transform {
    let pivot = match weapon {
        WeaponKind::Pistol => Vec3::new(-0.034, 0.015, -0.04),
        WeaponKind::Smg => Vec3::new(-0.04, 0.02, -0.04),
        WeaponKind::Rifle => Vec3::new(-0.047, 0.055, -0.09),
    };
    let rotation = Quat::from_rotation_z(-0.30 * action.control);
    Transform::from_translation(pivot - rotation * pivot + Vec3::X * action.control * 0.004)
        .with_rotation(rotation)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interrupted_locked_mp5_handle_keeps_the_displayed_pose_then_blends_home() {
        let mut action = reload_action(WeaponKind::Smg, 0.83, true);
        let before = bolt_transform(WeaponKind::Smg, &action, 0.0, true);
        let cancelled = bolt_transform(WeaponKind::Smg, &action, 0.0, false);
        assert_eq!(
            cancelled, before,
            "cancellation alone must not reset a displayed part"
        );
        let mut previous = cancelled;
        for _ in 0..120 {
            let decay = (-32.0_f32 / 120.0).exp();
            action.bolt *= decay;
            action.latch *= decay;
            let current = bolt_transform(WeaponKind::Smg, &action, 0.0, false);
            assert!(current.translation.distance(previous.translation) < 0.022);
            assert!(current.rotation.angle_between(previous.rotation) < 0.17);
            previous = current;
        }
        assert!(previous.translation.length() < 0.0001);
        assert!(previous.rotation.angle_between(Quat::IDENTITY) < 0.001);
    }
    #[test]
    fn overhand_slide_actually_pulls_and_mp5_palm_tracks_the_locked_handle() {
        let pistol = reload_action(WeaponKind::Pistol, 0.82, true);
        assert!(bolt_offset(WeaponKind::Pistol, &pistol, 0.0, true) > 0.051);
        for t in [0.06, 0.10, 0.83, 0.85] {
            let action = reload_action(WeaponKind::Smg, t, true);
            let handle = bolt_transform(WeaponKind::Smg, &action, 0.0, true)
                .transform_point(Vec3::new(-0.05, 0.08, -0.22));
            assert!(
                action
                    .support
                    .translation
                    .distance(handle + Vec3::new(-0.02, 0.0, 0.005))
                    < 0.001,
                "HK hand lost its moving handle at {t}"
            );
        }
    }
    #[test]
    fn ads_centres_the_actual_iron_sight_and_preserves_the_hip_barrel() {
        for (weapon, sight_y) in [(WeaponKind::Pistol, 0.118), (WeaponKind::Smg, 0.136)] {
            let sight = ads_offset(weapon) + Vec3::new(0.0, sight_y, 0.005);
            // User-approved correction: a rear sight belongs near the eye,
            // ahead of the 0.1 m near plane, not arbitrarily beyond 0.4 m.
            assert!(sight.x.abs() < 0.0001 && sight.y.abs() < 0.0001 && sight.z < -0.1);
            assert_eq!(
                barrel_offset(weapon, 0.0),
                Vec3::from(weapon.spec().barrel_offset)
            );
            assert_eq!(
                barrel_offset(weapon, 1.0),
                ads_offset(weapon) + WeaponRig::for_weapon(weapon).muzzle
            );
            assert!(barrel_offset(weapon, 1.0).x.abs() < 0.0001);
            for ads in [0.0, 0.5, 1.0] {
                assert_eq!(
                    resolved_muzzle_offset(weapon.spec(), ads),
                    barrel_offset(weapon, ads)
                );
            }
        }
        let rifle = WeaponKind::Rifle.spec();
        let aimed = rifle.assembly.ads_from_root;
        for kind in [
            crate::catalog::AnchorKind::FrontIron,
            crate::catalog::AnchorKind::RearIron,
        ] {
            let sight = aimed.transform_point(rifle.assembly.anchors[&kind].translation);
            assert!(sight.x.abs() < 0.00025 && sight.y.abs() < 0.00025);
            assert!(sight.z < -0.1, "authored sight must clear the near plane");
        }
        // Independent published barrel and authored eye-relief measurement.
        assert!(
            (barrel_offset(WeaponKind::Rifle, 1.0)
                - Vec3::new(0.0, -0.066, -0.270 - 14.5 * 0.0254))
            .length()
                < 1e-6
        );
        assert_eq!(
            barrel_offset(WeaponKind::Rifle, 0.0),
            Vec3::from(rifle.barrel_offset)
        );
    }
    #[test]
    fn empty_reload_has_readable_slide_handle_and_catch_actions_but_tactical_does_not_rack() {
        for weapon in [WeaponKind::Pistol, WeaponKind::Smg, WeaponKind::Rifle] {
            let held = reload_action(weapon, 0.80, true);
            let released = reload_action(weapon, 0.94, true);
            assert!(held.bolt > 0.95 && released.bolt < 0.01);
            assert!(
                held.support
                    .translation
                    .distance(WeaponRig::for_weapon(weapon).support)
                    > 0.08
            );
            if weapon == WeaponKind::Smg {
                assert!(held.latch > 0.99);
                assert!(
                    bolt_transform(weapon, &held, 0.0, true)
                        .rotation
                        .angle_between(Quat::IDENTITY)
                        > 0.6
                );
                assert_eq!(
                    held.control, 0.0,
                    "MP5 selector must not be pressed as an AR bolt catch"
                );
            } else {
                let press = reload_action(
                    weapon,
                    if weapon == WeaponKind::Pistol {
                        0.86
                    } else {
                        0.89
                    },
                    true,
                );
                assert!(press.control > 0.95);
                assert!(
                    control_transform(weapon, &press)
                        .rotation
                        .angle_between(Quat::IDENTITY)
                        > 0.25
                );
            }
            for i in 0..=100 {
                let tactical = reload_action(weapon, i as f32 / 100.0, false);
                assert_eq!(tactical.bolt, 0.0);
                assert_eq!(tactical.control, 0.0);
                assert_eq!(tactical.latch, 0.0);
            }
        }
    }
    #[test]
    fn mp5_handle_only_moves_during_empty_reload_while_slide_and_carrier_cycle() {
        let ready = reload_action(WeaponKind::Smg, 0.0, false);
        let empty = reload_action(WeaponKind::Smg, 0.5, true);
        assert_eq!(bolt_offset(WeaponKind::Smg, &ready, 0.1, false), 0.0);
        assert_eq!(bolt_offset(WeaponKind::Smg, &empty, 0.0, false), 0.0);
        assert!(bolt_offset(WeaponKind::Smg, &empty, 0.0, true) > 0.06);
        for w in [WeaponKind::Pistol, WeaponKind::Rifle] {
            assert!(bolt_offset(w, &reload_action(w, 0.0, false), 0.05, false) > 0.0);
        }
    }
    #[test]
    fn original_rigs_have_distinct_muzzles_and_support_contacts() {
        let pistol = WeaponRig::for_weapon(WeaponKind::Pistol);
        let smg = WeaponRig::for_weapon(WeaponKind::Smg);
        let rifle = WeaponRig::for_weapon(WeaponKind::Rifle);
        assert!(rifle.muzzle.z < smg.muzzle.z && smg.muzzle.z < pistol.muzzle.z);
        assert!(pistol.trigger.distance(pistol.support) < 0.09);
        assert!(rifle.trigger.distance(rifle.support) > 0.35);
    }
    #[test]
    fn reload_contacts_endpoints_and_empty_release_follow_each_weapon() {
        for w in [WeaponKind::Pistol, WeaponKind::Smg, WeaponKind::Rifle] {
            let rig = WeaponRig::for_weapon(w);
            for empty in [false, true] {
                for t in [0.0, 1.0] {
                    let a = reload_action(w, t, empty);
                    assert!(a.root.translation.length() < 0.0001);
                    assert!(a.root.rotation.angle_between(Quat::IDENTITY) < 0.001);
                    assert!(a.support.translation.distance(rig.support) < 0.0001);
                    assert!(a.magazine.translation.distance(rig.magazine) < 0.0001);
                    assert!(a.magazine_visible);
                }
                for t in [0.18, 0.20, 0.56, 0.60] {
                    let a = reload_action(w, t, empty);
                    let contact = a.magazine.translation + Vec3::new(-0.045, -0.025, 0.025);
                    assert!(a.support.translation.distance(contact) < 0.001, "{w:?} {t}");
                }
                let mut previous = reload_action(w, 0.0, empty);
                for i in 1..=1000 {
                    let a = reload_action(w, i as f32 / 1000.0, empty);
                    assert!(a.support.translation.distance(previous.support.translation) < 0.02);
                    assert!(a.root.rotation.angle_between(previous.root.rotation) < 0.02);
                    previous = a;
                }
            }
            assert_eq!(reload_action(w, 0.75, false).bolt, 0.0);
            assert!(reload_action(w, 0.75, true).bolt > 0.9);
            assert_eq!(reload_action(w, 1.0, true).bolt, 0.0);
        }
        assert!(
            reload_action(WeaponKind::Pistol, 0.5, false)
                .root
                .rotation
                .angle_between(reload_action(WeaponKind::Smg, 0.5, false).root.rotation)
                > 0.1
        );
    }
}
