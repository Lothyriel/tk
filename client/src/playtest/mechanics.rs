//! Visual route clocks wait for replicated mechanical outcomes, not guessed
//! weapon durations. All requests still travel through ordinary ClientInput.
use common::{catalog::FireMode, *};

#[derive(Default)]
pub(super) struct Clock {
    pub value: f64,
    previous: Option<f64>,
}

impl Clock {
    pub fn advance(&mut self, now: f64, gates: &[(f64, bool)]) {
        let delta = self
            .previous
            .map_or(0.0, |last| (now - last).clamp(0.0, 0.05));
        self.previous = Some(now);
        let mut next = self.value + delta;
        for &(at, ready) in gates {
            if self.value <= at && next >= at && !ready {
                // Stay before the cue: capture assertions at the cue must not
                // run until its mechanical prerequisite has actually arrived.
                next = at - 0.000001;
                break;
            }
        }
        self.value = next;
    }
}

pub(super) fn ready(local: &ClientData, slot: EquipmentSlot) -> bool {
    local.weapon_slot == slot && local.mechanics.equipped && local.mechanics.kind.is_none()
}

pub(super) fn loaded(local: &ClientData) -> bool {
    local.mechanics.kind.is_none() && local.mechanics.chambered && local.mechanics.magazine_inserted
}

#[derive(Default)]
pub(super) struct Injury {
    clock: Clock,
    fall_seen: bool,
}

impl Injury {
    pub fn control(
        &mut self,
        input: &mut ClientInput,
        local: &ClientData,
        other: &ClientData,
        now: f64,
        peer: bool,
    ) -> f64 {
        assert!(now < 40.0, "injury route timed out at {}", self.clock.value);
        let victim = if peer { local } else { other };
        self.fall_seen |= victim.pose.mode == common::humanoid::PhysicalMode::Fallen;
        self.clock.advance(
            now,
            &[
                (
                    2.8,
                    ready(local, EquipmentSlot::Sidearm)
                        && loaded(local)
                        && ready(other, EquipmentSlot::Sidearm)
                        && loaded(other),
                ),
                // Frame-rate differences must not make the leader inspect a
                // fall before the peer's movement has reached authority.
                (14.2, self.fall_seen),
                (18.5, peer || ready(local, EquipmentSlot::Primary)),
            ],
        );
        let t = self.clock.value;
        if t >= 2.5 {
            input.weapon = if !peer && t > 18.0 {
                EquipmentSlot::Primary
            } else {
                EquipmentSlot::Sidearm
            };
        }
        t
    }
}

#[derive(Default)]
pub(super) struct Selector {
    requested_from: Option<FireMode>,
}

impl Selector {
    pub fn select(&mut self, input: &mut ClientInput, local: &ClientData, target: FireMode) {
        if local.mechanics.mode != target && self.requested_from != Some(local.mechanics.mode) {
            input.selector_sequence = input.selector_sequence.wrapping_add(1);
            self.requested_from = Some(local.mechanics.mode);
        }
        if local.mechanics.mode == target {
            self.requested_from = None;
        }
    }
}

#[derive(Default)]
pub(super) struct Weapons {
    clock: Clock,
    actions: u32,
    pistol_before: u32,
    selector: Selector,
}

impl Weapons {
    pub fn control(&mut self, input: &mut ClientInput, local: &ClientData, now: f64) -> f64 {
        assert!(now < 60.0, "weapon route timed out at {}", self.clock.value);
        use EquipmentSlot::{Primary, Sidearm};
        self.clock.advance(
            now,
            &[
                (
                    0.0,
                    ready(local, Primary) && local.mechanics.mode == FireMode::Auto,
                ),
                (3.2, local.ammo_in_mag == 0 && !local.mechanics.chambered),
                (4.0, !local.mechanics.magazine_inserted),
                (4.25, ready(local, Sidearm)),
                (4.5, local.ammo_in_mag < self.pistol_before),
                (6.9, loaded(local)),
                (7.15, ready(local, Primary)),
                (9.9, loaded(local)),
            ],
        );
        let t = self.clock.value;
        input.weapon = if (4.1..7.1).contains(&t) {
            Sidearm
        } else {
            Primary
        };
        input.fire = t < 3.25;
        if t <= 0.0 {
            self.selector.select(input, local, FireMode::Auto);
        }
        for (index, at) in [3.4, 4.3, 4.6, 7.2].into_iter().enumerate() {
            if t >= at && self.actions & (1 << index) == 0 {
                self.actions |= 1 << index;
                if index == 1 {
                    self.pistol_before = local.ammo_in_mag;
                    input.fire_pressed_sequence = input.fire_pressed_sequence.wrapping_add(1);
                } else {
                    input.reload_sequence = input.reload_sequence.wrapping_add(1);
                }
            }
        }
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playtest::animation::{ReviewFrame, Sequence};
    use common::{catalog::*, combat::*};
    use std::collections::VecDeque;

    struct Actor {
        arsenal: Arsenal,
        input: ClientInput,
        received: ClientData,
        pending: VecDeque<(usize, Vec<u8>)>,
        shots: u32,
        initial: u32,
        ads: f32,
    }

    impl Actor {
        fn new(catalog: &Catalog, primary: WeaponId) -> Self {
            let arsenal = Arsenal::from_slots(
                catalog,
                Loadout::default(),
                &[
                    (EquipmentSlot::Primary, primary),
                    (EquipmentSlot::Sidearm, WeaponId::M1911),
                ],
            )
            .unwrap();
            let initial = [EquipmentSlot::Primary, EquipmentSlot::Sidearm]
                .into_iter()
                .map(|slot| arsenal.equipped(slot).unwrap().mechanics().total_rounds())
                .sum();
            let mut actor = Self {
                arsenal,
                input: ClientInput::default(),
                received: ClientData::default(),
                pending: VecDeque::new(),
                shots: 0,
                initial,
                ads: 0.0,
            };
            actor.received = actor.snapshot();
            actor
        }

        fn snapshot(&self) -> ClientData {
            let weapon = self.arsenal.current();
            ClientData {
                alive: true,
                weapon: weapon.spec().class,
                weapon_id: weapon.spec().definition,
                weapon_instance: weapon.instance().id,
                weapon_slot: self.arsenal.active,
                ammo_in_mag: weapon.rounds(),
                reserve_ammo: weapon.reserve(),
                mechanics: weapon.mechanics().snapshot(),
                pose: common::humanoid::PoseState {
                    ads: self.ads,
                    ..Default::default()
                },
                ..Default::default()
            }
        }

        fn tick(&mut self, step: usize, handling: f32) {
            let dt = 1.0 / 128.0;
            self.shots += u32::from(
                self.arsenal
                    .step(&self.input, dt * handling, step as f32 * dt)
                    .is_some(),
            );
            // Presentation readiness is supplied as a confirmed pose. This test
            // exercises route control and serialization, not camera rendering.
            let target = if self.input.aim && self.arsenal.current().mechanics().action().is_none()
            {
                1.0
            } else {
                0.0
            };
            self.ads += (target - self.ads) * 0.12;
            if step.is_multiple_of(4) && !step.is_multiple_of(20) {
                self.pending
                    .push_back((step + 6, data::encode(&self.snapshot()).to_vec()));
            }
            while self
                .pending
                .front()
                .is_some_and(|(arrival, _)| *arrival <= step)
            {
                self.received = data::decode(&self.pending.pop_front().unwrap().1);
            }
            let total: u32 = [EquipmentSlot::Primary, EquipmentSlot::Sidearm]
                .into_iter()
                .map(|slot| {
                    self.arsenal
                        .equipped(slot)
                        .unwrap()
                        .mechanics()
                        .total_rounds()
                })
                .sum();
            assert_eq!(total + self.shots, self.initial);
            self.input.fire = false;
            self.input.aim = false;
        }

        fn fov(&self) -> f32 {
            let aimed = if self.received.weapon == WeaponKind::Pistol {
                52.0
            } else {
                44.0
            };
            70.0 + (aimed - 70.0) * self.received.pose.ads
        }
    }

    #[test]
    fn injury_checkpoint_waits_for_replicated_peer_fall_under_clock_skew() {
        let ready = ClientData {
            weapon_slot: EquipmentSlot::Sidearm,
            mechanics: MechanicalSnapshot {
                equipped: true,
                chambered: true,
                magazine_inserted: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut victim = ready.clone();
        let mut route = Injury::default();
        let mut input = ClientInput::default();
        for step in 0..1600 {
            let t = route.control(&mut input, &ready, &victim, step as f64 * 0.01, false);
            assert!(t < 14.2, "leader outran the peer's fall");
        }
        victim.pose.mode = common::humanoid::PhysicalMode::Fallen;
        assert!(route.control(&mut input, &ready, &victim, 16.0, false) >= 14.2);
        victim.pose.mode = common::humanoid::PhysicalMode::Recovering;
        assert!(route.control(&mut input, &ready, &victim, 16.01, false) > 14.2);
    }

    #[test]
    fn mechanics_injury_waits_for_both_equips_before_consuming_the_shot_edge() {
        let mut legacy = Actor::new(catalog(), WeaponId::M4A1);
        for step in 0..512 {
            let t = step as f64 / 128.0;
            if t >= 2.5 {
                legacy.input.weapon = EquipmentSlot::Sidearm;
            }
            if step == 384 {
                legacy.input.fire_pressed_sequence += 1;
            }
            legacy.tick(step, 1.0);
        }
        assert_eq!(legacy.shots, 0, "the old 3s cue fires during equip");
        assert!(ready(&legacy.received, EquipmentSlot::Sidearm));

        let mut actors = [
            Actor::new(catalog(), WeaponId::M4A1),
            Actor::new(catalog(), WeaponId::MP5),
        ];
        let mut routes = [Injury::default(), Injury::default()];
        let mut fired = [false; 2];
        for step in 0..768 {
            let snapshots = [actors[0].received.clone(), actors[1].received.clone()];
            for index in 0..2 {
                let t = routes[index].control(
                    &mut actors[index].input,
                    &snapshots[index],
                    &snapshots[1 - index],
                    step as f64 / 128.0,
                    index == 1,
                );
                if t >= 3.0 && !fired[index] {
                    assert!(snapshots.iter().all(|s| ready(s, EquipmentSlot::Sidearm)));
                    actors[index].input.fire_pressed_sequence += 1;
                    fired[index] = true;
                }
                actors[index].tick(step, if index == 0 { 1.0 } else { 0.55 });
            }
        }
        assert_eq!(fired, [true; 2]);
        for actor in actors {
            assert_eq!(
                actor.shots, 1,
                "one accepted shot despite snapshot delay/loss"
            );
        }
    }

    #[test]
    fn mechanics_native_routes_follow_delayed_snapshots_and_changed_capacities() {
        for (primary, capacity, handling) in [(WeaponId::M4A1, 17, 1.0), (WeaponId::MP5, 7, 0.55)] {
            let source = CATALOG_SOURCE.replace("capacity: 17", &format!("capacity: {capacity}"));
            let registry = Catalog::parse("route-fixture.ron", &source).unwrap();
            let mut actors = [
                Actor::new(&registry, primary),
                Actor::new(&registry, primary),
            ];
            let mut sequences = [Sequence::default(), Sequence::default()];
            let mut finished_at = None;
            for step in 0..19_000 {
                let snapshots = [actors[0].received.clone(), actors[1].received.clone()];
                for index in 0..2 {
                    let frame = ReviewFrame {
                        elapsed: step as f64 / 128.0,
                        peer: index == 1,
                        world_fov: actors[index].fov(),
                    };
                    sequences[index].control(
                        &mut actors[index].input,
                        &snapshots[index],
                        Some(&snapshots[1 - index]),
                        &frame,
                    );
                    actors[index].tick(step, handling);
                }
                if sequences.iter().all(|s| s.complete) {
                    finished_at = Some(step);
                    break;
                }
            }
            let finished_at =
                finished_at.expect("reciprocal route must finish from confirmed state");
            for actor in &actors {
                assert!(actor.shots >= capacity + primary.spec().magazine_size);
                assert!(ready(&actor.received, EquipmentSlot::Primary));
                assert_eq!(actor.received.mechanics.mode, FireMode::Safe);
            }
            // The normal raid's weapon exercise follows the animation review.
            // It must handle retained partial magazines and a tactical +1.
            let mut routes = [Weapons::default(), Weapons::default()];
            let mut done = [false; 2];
            for frame in 0..7_500 {
                for index in 0..2 {
                    let actor = &mut actors[index];
                    let t = routes[index].control(
                        &mut actor.input,
                        &actor.received,
                        frame as f64 / 128.0,
                    );
                    done[index] |= t > 10.0;
                    actor.tick(finished_at + frame + 1, handling);
                }
                if done.iter().all(|done| *done) {
                    break;
                }
            }
            assert!(done.iter().all(|done| *done));
            for actor in actors {
                assert!(loaded(&actor.received));
                assert_eq!(actor.received.weapon_slot, EquipmentSlot::Primary);
            }
        }
    }
}
