//! Continuous action route through ordinary replicated controls, with reciprocal observation.
use super::{
    capture,
    mechanics::{Clock, Selector, loaded, ready},
    move_toward,
};
use bevy::prelude::*;
use common::*;

#[derive(Default)]
pub(super) struct Sequence {
    pub actions: u32,
    pub captures: u32,
    pub ads_captures: u8,
    pub mechanism_captures: u8,
    observer_captures: u8,
    clock: Clock,
    selector: Selector,
    next_click: f64,
    started: bool,
    pistol_before: u32,
    pub complete: bool,
}

pub(super) struct ReviewFrame {
    pub elapsed: f64,
    pub peer: bool,
    pub world_fov: f32,
}

impl Sequence {
    /// Safe on the primary is a persistent end-of-route signal, visible to the
    /// observer even if snapshots are lost. No fixed peer start offset is needed.
    fn signalled(local: &ClientData) -> bool {
        ready(local, EquipmentSlot::Primary) && local.mechanics.mode == catalog::FireMode::Safe
    }

    pub(super) fn control(
        &mut self,
        input: &mut ClientInput,
        local: &ClientData,
        other: Option<&ClientData>,
        frame: &ReviewFrame,
    ) -> bool {
        let ReviewFrame {
            elapsed,
            peer,
            world_fov,
        } = *frame;
        assert!(
            elapsed < 150.0,
            "animation route timed out at {}",
            self.clock.value
        );
        if !self.started && (!peer || other.is_some_and(Self::signalled)) {
            self.started = true;
            info!("ANIMATION CLOCK stage-relative");
        }
        if !self.started {
            return true;
        }
        if self.clock.value >= 29.0 {
            input.weapon = EquipmentSlot::Primary;
            self.selector.select(input, local, catalog::FireMode::Safe);
            self.complete = Self::signalled(local) && other.is_some_and(Self::signalled);
            return true;
        }
        use EquipmentSlot::{Primary, Sidearm};
        self.clock.advance(
            elapsed,
            &[
                (0.15, ready(local, Sidearm)),
                (
                    0.95,
                    local.pose.ads > 0.85 && (world_fov - 52.0).abs() < 2.0,
                ),
                (1.6, local.pose.ads < 0.08),
                (5.4, local.ammo_in_mag == 0 && !local.mechanics.chambered),
                (7.4, loaded(local)),
                (8.4, local.ammo_in_mag < self.pistol_before),
                (8.95, !local.mechanics.magazine_inserted),
                (9.5, ready(local, Primary)),
                (10.8, ready(local, Sidearm)),
                (13.5, loaded(local)),
                (14.3, ready(local, Primary)),
                (16.1, local.mechanics.mode == catalog::FireMode::Auto),
                (19.7, local.ammo_in_mag == 0 && !local.mechanics.chambered),
                (22.9, loaded(local)),
                (
                    23.3,
                    local.ammo_in_mag < local.weapon_id.spec().magazine_size,
                ),
                (23.95, !local.mechanics.magazine_inserted),
                (24.5, ready(local, Sidearm)),
                (24.9, ready(local, Primary)),
                (26.7, loaded(local)),
                (
                    28.4,
                    local.pose.ads > 0.85 && (world_fov - 44.0).abs() < 2.0,
                ),
                (28.8, local.pose.ads < 0.08),
            ],
        );
        let t = self.clock.value;
        input.weapon = if t < 14.0 { Sidearm } else { Primary };
        if (9.0..9.6).contains(&t) {
            input.weapon = Primary;
        }
        if (24.0..24.6).contains(&t) {
            input.weapon = Sidearm;
        }
        input.aim = (0.18..1.1).contains(&t)
            || (7.5..8.3).contains(&t)
            || (14.35..15.0).contains(&t)
            || (26.8..28.6).contains(&t);
        if (2.0..5.5).contains(&t) && local.ammo_in_mag > 0 && elapsed >= self.next_click {
            input.fire_pressed_sequence = input.fire_pressed_sequence.wrapping_add(1);
            self.next_click = elapsed + f64::from(local.weapon_id.spec().seconds_per_shot()) + 0.04;
        }
        if (14.3..=16.1).contains(&t) && ready(local, Primary) {
            self.selector.select(input, local, catalog::FireMode::Auto);
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
            if t >= at && self.actions & (1 << index) == 0 {
                self.actions |= 1 << index;
                if reload {
                    input.reload_sequence = input.reload_sequence.wrapping_add(1);
                } else {
                    self.pistol_before = local.ammo_in_mag;
                    input.fire_pressed_sequence = input.fire_pressed_sequence.wrapping_add(1);
                }
            }
        }
        false
    }
}

pub(super) fn drive(
    commands: &mut Commands,
    sequence: &mut Sequence,
    input: &mut ClientInput,
    local: &ClientData,
    other: Option<&ClientData>,
    frame: ReviewFrame,
) -> bool {
    let ReviewFrame {
        elapsed,
        peer,
        world_fov,
    } = frame;
    let observe = sequence.control(input, local, other, &frame);
    if observe {
        let target = if std::env::var_os("TK_WEAPON_DEBUG").is_some()
            && let Some(other) = other
        {
            // Asset review: a near side view outside the actor's muzzle ray.
            // Locomotion still uses ordinary replicated input.
            (Vec3::from(other.pos) + Vec3::new(1.35, 0.0, 0.15)).to_array()
        } else if peer {
            [-7.0, 1.0, 13.0]
        } else {
            [-3.0, 1.0, 14.0]
        };
        if move_toward(input, local.pos, target) < 0.4
            && let Some(other) = other
        {
            input.forward = false;
            input.backward = false;
            input.left = false;
            input.right = false;
            input.run = false;
            input.camera =
                Transform::from_translation(Vec3::from(local.pos) + Vec3::Y * PLAYER_VIEW_HEIGHT)
                    .looking_at(Vec3::from(other.pos) + Vec3::Y * 0.4, Vec3::Y)
                    .rotation
                    .into();
            let weapon_index = usize::from(other.weapon != WeaponKind::Pistol);
            for (cue, label, visible) in [
                (0, "ads", other.pose.ads > 0.95),
                (
                    1,
                    "reload",
                    other.mechanics.reloading() && (0.30..0.65).contains(&other.mechanics.fraction),
                ),
            ] {
                let bit = 1 << (weapon_index * 2 + cue);
                if visible && sequence.observer_captures & bit == 0 {
                    sequence.observer_captures |= bit;
                    capture(
                        commands,
                        &format!("remote-{}-{label}", other.weapon_id.spec().catalog_key),
                    );
                    info!(
                        "REMOTE CONTACT {label} weapon={:?} ads={} reload={}",
                        other.weapon, other.pose.ads, other.mechanics.fraction
                    );
                }
            }
        }
        if sequence.complete {
            assert_eq!(
                sequence.mechanism_captures, 0b11,
                "both empty reload mechanism captures must be observed"
            );
            assert_eq!(
                sequence.observer_captures, 0b1111,
                "both remote weapons require ADS and reload evidence"
            );
        }
        return true;
    }
    let t = sequence.clock.value;
    input.camera = CameraInput {
        pitch: 0.55,
        yaw: ((t - 1.0).clamp(0.0, 3.0) as f32 * 0.7).sin() * 0.22,
        roll: 0.0,
    };
    input.forward = (1.0..1.8).contains(&t) || (8.6..9.1).contains(&t);
    input.backward = (1.8..2.6).contains(&t) || (9.1..9.6).contains(&t);
    input.left = (15.0..15.5).contains(&t);
    input.right = (15.5..16.0).contains(&t);
    input.crouch = (11.0..11.6).contains(&t);
    input.run = (15.0..16.0).contains(&t);
    // ADS_PROOF_BEGIN
    let fraction = local.mechanics.fraction;
    let mechanism_index = if local.weapon == WeaponKind::Pistol {
        0
    } else {
        1
    };
    let action = common::weapon_animation::reload_action(local.weapon, fraction, true);
    // A delayed/lost snapshot can jump over a timestamp and arrive after the
    // control has already returned. Capture the actual active mechanism instead
    // of asserting that every sample after a timestamp is still at its peak.
    let mechanism_visible = if local.weapon == WeaponKind::Smg {
        (0.85..0.95).contains(&fraction) && action.control_grip > 0.5
    } else {
        action.control > 0.5
    };
    if local.mechanics.kind == Some(catalog::ManipulationKind::EmptyReload)
        && mechanism_visible
        && sequence.mechanism_captures & (1 << mechanism_index) == 0
    {
        assert!(action.control > 0.5 || local.weapon == WeaponKind::Smg);
        let name = if mechanism_index == 0 {
            "m1911-slide-release"
        } else {
            "primary-bolt-release"
        };
        capture(commands, name);
        info!(
            "RELOAD MECHANISM EVIDENCE {name} local_t={elapsed:.3} weapon={:?} fraction={fraction:.3} bolt={:.3} control={:.3} latch={:.3}",
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
                "ADS EVIDENCE {name} local_t={elapsed:.3} weapon={:?} authoritative_ads={:.3} world_fov={world_fov:.3} ammo={}",
                local.weapon, local.pose.ads, local.ammo_in_mag
            );
            sequence.ads_captures |= 1 << index;
        }
    }
    // ADS_PROOF_END
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
                "ANIMATION CONTACT {name} local_t={elapsed:.3} weapon={:?} mag={} reload={:.3} position={:?}",
                local.weapon, local.ammo_in_mag, local.reload_remaining, local.pos
            );
            if name.ends_with("empty") {
                assert_eq!(local.ammo_in_mag, 0, "{name}");
            }
            if name.ends_with("return") {
                assert_eq!(local.ammo_in_mag, local.weapon_id.spec().magazine_size + 1);
                assert!(local.mechanics.chambered && local.mechanics.magazine_inserted);
                assert_eq!(local.reload_remaining, 0.0);
            }
        }
    }
    false
}
