//! Authored AR retention/retrieval choreography, in magazine-seat space.
//! These are presentation values, not claims of measured human motion.
use super::*;
use crate::catalog::{MagazineSnapshot, MechanicalSnapshot, ReloadTiming};

pub const STOWED: f32 = 0.38;
pub const CHANGEOVER: f32 = 0.41;
pub const RETRIEVE: f32 = 0.44;
const ALIGNED: f32 = 0.60;
const SEATED: f32 = 0.66;

/// Outgoing and incoming are different physical objects even with equal
/// definitions. Replace the displayed object only in the hidden retention gap.
pub fn magazine(mechanics: &MechanicalSnapshot) -> Option<MagazineSnapshot> {
    let value = if mechanics.reloading() {
        if mechanics.fraction < CHANGEOVER {
            mechanics.magazine
        } else {
            mechanics.reload_target
        }
    } else {
        return mechanics.inserted_magazine();
    };
    value.present().then_some(value)
}

fn track(t: f32, keys: &[(f32, Vec3)]) -> Vec3 {
    for pair in keys.windows(2) {
        if t <= pair[1].0 {
            return pair[0].1.lerp(pair[1].1, ramp(t, pair[0].0, pair[1].0));
        }
    }
    keys.last().unwrap().1
}

pub(super) fn reload(fraction: f32, empty: bool) -> WeaponAction {
    let t = fraction.clamp(0.0, 1.0);
    let rig = WeaponRig::for_weapon(WeaponKind::Rifle);
    // Clear feed lips straight down before moving toward retention. Approach
    // the well axially and seat before the INSERTED marker, leaving a dwell.
    let delta = track(
        t,
        &[
            (0.0, Vec3::ZERO),
            (0.12, Vec3::ZERO),
            (ReloadTiming::REMOVED, Vec3::new(0.0, -0.13, 0.0)),
            (STOWED, Vec3::new(-0.07, -0.43, 0.12)),
            (RETRIEVE, Vec3::new(-0.07, -0.43, 0.12)),
            (ALIGNED, Vec3::new(0.0, -0.045, 0.0)),
            (SEATED, Vec3::ZERO),
            (1.0, Vec3::ZERO),
        ],
    );
    let turn = ramp(t, ReloadTiming::REMOVED, STOWED) * (1.0 - ramp(t, RETRIEVE, ALIGNED));
    let magazine = Transform::from_translation(rig.magazine + delta).with_rotation(
        Quat::from_euler(EulerRot::XYZ, -0.20 * turn, 0.12 * turn, 0.0),
    );
    let reach = ramp(t, 0.04, 0.12)
        * (1.0 - ramp(t, ReloadTiming::INSERTED, if empty { 0.81 } else { 0.84 }));
    let control_grip = if empty {
        ramp(t, ReloadTiming::INSERTED, 0.81) * (1.0 - ramp(t, 0.92, 1.0))
    } else {
        0.0
    };
    let present = ramp(t, 0.0, 0.12) * (1.0 - ramp(t, if empty { 0.92 } else { 0.84 }, 1.0));
    // Actual configured hands bind to authored sockets. This legacy contact
    // remains coherent for the anatomical fallback and diagnostic fixtures.
    let hand = rig
        .support
        .lerp(
            magazine.translation + Vec3::new(-0.045, -0.025, 0.025),
            reach,
        )
        .lerp(Vec3::new(-0.067, 0.055, -0.085), control_grip);
    WeaponAction {
        root: Transform::from_translation(Vec3::new(-0.015, -0.045, 0.055) * present)
            .with_rotation(Quat::from_euler(
                EulerRot::XYZ,
                0.10 * present,
                -0.22 * present,
                -0.30 * present,
            )),
        support: Transform::from_translation(hand).with_rotation(
            Quat::from_rotation_z(-0.3 * reach).slerp(
                Quat::from_euler(EulerRot::XYZ, 0.0, -0.70, 0.15),
                control_grip,
            ),
        ),
        magazine_grip: reach,
        control_grip,
        magazine,
        magazine_visible: !(STOWED..RETRIEVE).contains(&t),
        // The carrier cannot close before the authoritative feed commit.
        bolt: if empty && t < ReloadTiming::FED {
            1.0
        } else {
            0.0
        },
        control: if empty {
            ramp(t, 0.82, 0.88) * (1.0 - ramp(t, ReloadTiming::FED, 0.96))
        } else {
            0.0
        },
        latch: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::*;

    #[test]
    fn m4a1_reload_seats_before_commit_and_never_releases_an_unfed_bolt() {
        for empty in [false, true] {
            for duration in [1.2, 2.4, 5.7] {
                for handling_rate in [0.35, 1.0] {
                    for fps in [30, 60, 165] {
                        let mut state = MechanicalState::new(
                            catalog(),
                            WeaponId::M4A1.spec(),
                            [1, 2, 3, 4].map(MagazineInstanceId),
                        )
                        .unwrap();
                        for _ in 0..if empty { 30 } else { 5 } {
                            assert!(state.fire(true, true));
                        }
                        assert!(state.start_reload(duration));
                        let total = state.total_rounds();
                        let mut saw_seated = false;
                        while state.action().is_some() {
                            let snapshot = state.snapshot();
                            let pose = reload(snapshot.fraction, empty);
                            assert_eq!(state.total_rounds(), total);
                            if matches!(
                                snapshot.phase,
                                Some(ManipulationPhase::Feed | ManipulationPhase::Finish)
                            ) {
                                assert_eq!(
                                    pose.magazine.translation,
                                    WeaponRig::for_weapon(WeaponKind::Rifle).magazine
                                );
                                assert_eq!(pose.magazine.rotation, Quat::IDENTITY);
                                saw_seated = true;
                            }
                            if empty && snapshot.phase != Some(ManipulationPhase::Finish) {
                                assert_eq!(pose.bolt, 1.0);
                                assert!(!snapshot.chambered);
                            } else {
                                assert_eq!(pose.bolt, 0.0);
                            }
                            if !empty {
                                assert_eq!(pose.control, 0.0);
                            }
                            state.advance(handling_rate / fps as f32);
                        }
                        assert!(saw_seated);
                        assert_eq!(state.total_rounds(), total);
                    }
                }
            }
        }
    }

    #[test]
    fn m4a1_reload_changes_identity_only_inside_retention_gap_and_uses_axial_seat() {
        let rig = WeaponRig::for_weapon(WeaponKind::Rifle);
        let mut snapshot = MechanicalSnapshot {
            kind: Some(ManipulationKind::TacticalReload),
            magazine: MagazineSnapshot::new(0, 12),
            reload_target: MagazineSnapshot::new(1, 30),
            ..Default::default()
        };
        for t in [CHANGEOVER - 0.001, CHANGEOVER + 0.001] {
            snapshot.fraction = t;
            let selected = magazine(&snapshot).unwrap();
            assert_eq!(selected.pool_index(), Some(usize::from(t > CHANGEOVER)));
            let pose = reload(t, false);
            assert!(!pose.magazine_visible);
            assert!((pose.magazine.translation - rig.magazine).y < -0.42);
        }
        for t in [0.12, 0.18, 0.24, 0.60, 0.63, 0.66, 0.70] {
            let pose = reload(t, false);
            assert_eq!(pose.magazine.rotation, Quat::IDENTITY);
            let delta = pose.magazine.translation - rig.magazine;
            assert!(delta.x.abs() < 1e-6 && delta.z.abs() < 1e-6);
        }
    }
}
