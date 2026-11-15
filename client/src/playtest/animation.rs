//! Continuous action route through ordinary replicated controls, with reciprocal observation.
use super::{capture, move_toward};
use bevy::prelude::*;
use common::*;

#[derive(Default)]
pub(super) struct Sequence {
    pub actions: u32,
    pub captures: u32,
    pub clicks: u32,
    pub ads_captures: u8,
    pub mechanism_captures: u8,
}

pub(super) fn drive(
    commands: &mut Commands,
    sequence: &mut Sequence,
    input: &mut ClientInput,
    local: &ClientData,
    other: Option<&ClientData>,
    elapsed: f64,
    peer: bool,
    primary: WeaponKind,
    world_fov: f32,
) -> bool {
    let observe = if peer {
        elapsed < 29.0
    } else {
        elapsed >= 29.0
    };
    if observe {
        let target = if peer {
            [-7.0, 1.0, 18.0]
        } else {
            [-3.0, 1.0, 18.0]
        };
        let reposition = if peer { elapsed < 1.3 } else { elapsed < 30.3 };
        if reposition {
            move_toward(input, local.pos, target);
        } else if let Some(other) = other {
            input.camera =
                Transform::from_translation(Vec3::from(local.pos) + Vec3::Y * PLAYER_VIEW_HEIGHT)
                    .looking_at(Vec3::from(other.pos) + Vec3::Y * 0.1, Vec3::Y)
                    .rotation
                    .into();
        }
        return true;
    }
    let t = elapsed - if peer { 29.0 } else { 0.0 };
    input.weapon = if t < 14.0 {
        WeaponKind::Pistol
    } else {
        primary
    };
    input.camera = CameraInput {
        pitch: 0.55,
        yaw: ((t - 1.0).max(0.0).min(3.0) as f32 * 0.7).sin() * 0.22,
        roll: 0.0,
    };
    input.forward = (1.0..1.8).contains(&t) || (8.6..9.1).contains(&t);
    input.backward = (1.8..2.6).contains(&t) || (9.1..9.6).contains(&t);
    input.left = (15.0..15.5).contains(&t);
    input.right = (15.5..16.0).contains(&t);
    input.crouch = (11.0..11.6).contains(&t);
    input.run = (15.0..16.0).contains(&t);
    // ADS_PROOF_BEGIN
    input.aim = (0.18..1.1).contains(&t)
        || (7.5..8.3).contains(&t)
        || (14.35..15.0).contains(&t)
        || (26.8..28.6).contains(&t);
    let fraction = local.pose.reload_fraction;
    let mechanism_index = if local.weapon == WeaponKind::Pistol {
        0
    } else {
        1
    };
    let mechanism_at = match local.weapon {
        WeaponKind::Pistol => 0.85,
        WeaponKind::Smg => 0.88,
        WeaponKind::Rifle => 0.89,
    };
    if local.ammo_in_mag == 0
        && local.reload_remaining > 0.0
        && fraction >= mechanism_at
        && fraction < 0.98
        && sequence.mechanism_captures & (1 << mechanism_index) == 0
    {
        let action = common::weapon_animation::reload_action(local.weapon, fraction, true);
        assert!(action.control > 0.5 || local.weapon == WeaponKind::Smg);
        let name = if mechanism_index == 0 {
            "m1911-slide-release"
        } else {
            "primary-bolt-release"
        };
        capture(commands, name);
        info!(
            "RELOAD MECHANISM EVIDENCE {name} local_t={t:.3} weapon={:?} fraction={fraction:.3} bolt={:.3} control={:.3} latch={:.3}",
            local.weapon, action.bolt, action.control, action.latch
        );
        sequence.mechanism_captures |= 1 << mechanism_index;
    }
    for (index, (at, name, aimed)) in [
        (0.95, "m1911-ads", true),
        (1.7, "m1911-ads-release", false),
        (28.4, "primary-ads", true),
        (28.85, "primary-ads-release", false),
    ]
    .into_iter()
    .enumerate()
    {
        if t >= at && sequence.ads_captures & (1 << index) == 0 {
            if aimed {
                assert!(
                    local.pose.ads > 0.85,
                    "authoritative ADS did not engage for {name}"
                );
                let expected = if local.weapon == WeaponKind::Pistol {
                    52.0_f32
                } else {
                    44.0_f32
                };
                assert!(
                    (world_fov - expected).abs() < 2.0,
                    "ADS did not apply the rendered camera zoom: {world_fov}"
                );
            } else {
                assert!(local.pose.ads < 0.08, "ADS did not release for {name}");
            }
            capture(commands, name);
            info!(
                "ADS EVIDENCE {name} local_t={t:.3} weapon={:?} authoritative_ads={:.3} world_fov={world_fov:.3} ammo={}",
                local.weapon, local.pose.ads, local.ammo_in_mag
            );
            sequence.ads_captures |= 1 << index;
        }
    }
    // ADS_PROOF_END
    if (2.0..5.5).contains(&t) {
        let click = (((t - 2.0) / 0.205).floor() as u32 + 1).min(17);
        if click > sequence.clicks {
            input.fire_pressed_sequence = input.fire_pressed_sequence.wrapping_add(1);
            sequence.clicks = click;
        }
    }
    input.fire = (16.2..19.8).contains(&t) || (23.0..23.2).contains(&t);
    for (index, (at, reload)) in [
        (5.9, true),
        (8.0, false),
        (8.5, true),
        (11.0, true),
        (20.2, true),
        (23.4, true),
        (25.0, true),
    ]
    .into_iter()
    .enumerate()
    {
        if t >= at && sequence.actions & (1 << index) == 0 {
            sequence.actions |= 1 << index;
            if reload {
                input.reload_sequence = input.reload_sequence.wrapping_add(1);
            } else {
                input.fire_pressed_sequence = input.fire_pressed_sequence.wrapping_add(1);
            }
        }
    }
    if (9.0..9.6).contains(&t) {
        input.weapon = primary;
    }
    if (24.0..24.6).contains(&t) {
        input.weapon = WeaponKind::Pistol;
    }
    for (index, (at, name)) in [
        (0.8, "m1911-draw"),
        (2.8, "m1911-fire"),
        (5.6, "m1911-empty"),
        (6.5, "m1911-empty-reload"),
        (7.6, "m1911-ready"),
        (8.8, "m1911-tactical"),
        (9.3, "m1911-interrupt"),
        (11.6, "m1911-resume"),
        (13.6, "m1911-return"),
        (14.8, "primary-draw"),
        (17.0, "primary-fire"),
        (20.0, "primary-empty"),
        (21.4, "primary-empty-reload"),
        (23.7, "primary-tactical"),
        (24.3, "primary-interrupt"),
        (26.0, "primary-resume"),
        (28.5, "primary-return"),
    ]
    .into_iter()
    .enumerate()
    {
        if t >= at && sequence.captures & (1 << index) == 0 {
            sequence.captures |= 1 << index;
            capture(commands, name);
            info!(
                "ANIMATION CONTACT {name} local_t={t:.3} weapon={:?} mag={} reload={:.3} position={:?}",
                local.weapon, local.ammo_in_mag, local.reload_remaining, local.pos
            );
            if name.ends_with("empty") {
                assert_eq!(local.ammo_in_mag, 0, "{name}");
            }
            if name.ends_with("return") {
                assert_eq!(local.ammo_in_mag, local.weapon.spec().magazine_size);
                assert_eq!(local.reload_remaining, 0.0);
            }
        }
    }
    false
}
