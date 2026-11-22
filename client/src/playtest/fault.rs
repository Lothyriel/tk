//! Opt-in exercise of a fictional game's confirmed availability state.
use common::{ClientData, ClientInput, EquipmentSlot, WeaponInstanceId};

// Leave transient confirmed states on the wire for several snapshots and
// on screen long enough for a capture before driving the next command.
const OBSERVATION_SECONDS: f64 = 0.25;

#[derive(Default)]
pub(super) struct Route {
    stage: u8,
    primary: Option<(WeaponInstanceId, u32, u32)>,
    entered_at: f64,
    recovery_requested: bool,
}

impl Route {
    pub(super) fn advance(
        &mut self,
        elapsed: f64,
        ready: bool,
        actor: &ClientData,
    ) -> Result<Option<&'static str>, &'static str> {
        if self.finished() {
            return Ok(None);
        }
        if elapsed >= 15.0 {
            return Err("fictional fault route timed out");
        }
        if !ready {
            return Ok(None);
        }
        let mechanics = actor.mechanics;
        let primary = actor.weapon_slot == EquipmentSlot::Primary;
        if primary {
            let observed = (actor.weapon_instance, actor.ammo_in_mag, actor.reserve_ammo);
            if self.primary.is_some_and(|before| before != observed) {
                return Err(
                    "primary identity or resource count changed during the fictional fault",
                );
            }
        }
        let unavailable = mechanics.action_unavailable();
        let recovering = mechanics.recovery_ms() > 0;
        let cue = match self.stage {
            0 if primary && mechanics.equipped && !unavailable => {
                self.primary = Some((actor.weapon_instance, actor.ammo_in_mag, actor.reserve_ammo));
                "rest"
            }
            1 if primary && unavailable && !recovering => "fault",
            2 if primary && recovering => "recovering",
            3 if !primary && mechanics.equipped => "interrupted",
            4 if primary && mechanics.equipped && unavailable && !recovering => "persisted",
            5 if primary && recovering => {
                self.stage = 6;
                return Ok(None);
            }
            6 if primary && mechanics.equipped && !unavailable => "return",
            _ => return Ok(None),
        };
        self.stage += 1;
        self.entered_at = elapsed;
        self.recovery_requested = false;
        Ok(Some(cue))
    }

    pub(super) fn input(&mut self, input: &mut ClientInput, cue: Option<&str>, elapsed: f64) {
        let observed = elapsed - self.entered_at >= OBSERVATION_SECONDS;
        input.weapon = if self.stage == 3 && observed {
            EquipmentSlot::Sidearm
        } else {
            EquipmentSlot::Primary
        };
        input.fire = self.stage == 1;
        if cue == Some("rest") {
            input.fire_pressed_sequence = input.fire_pressed_sequence.wrapping_add(1);
        }
        if matches!(self.stage, 2 | 5) && observed && !self.recovery_requested {
            input.reload_sequence = input.reload_sequence.wrapping_add(1);
            self.recovery_requested = true;
        }
    }

    pub(super) fn finished(&self) -> bool {
        self.stage == 7
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fictional_fault_route_requires_persistence_and_confirmed_recovery() {
        let mut route = Route::default();
        let mut actor = ClientData::default();
        let mut input = ClientInput::default();
        let mut step = |i, actor: &ClientData| {
            let cue = route.advance(i, true, actor).unwrap();
            route.input(&mut input, cue, i);
            (cue, input.weapon, input.reload_sequence)
        };
        assert_eq!(step(1.0, &actor).0, Some("rest"));
        actor.mechanics.action_fault = 1;
        assert_eq!(step(2.0, &actor).2, 0);
        assert_eq!(step(2.3, &actor).2, 1);
        assert_eq!(step(2.5, &actor).2, 1);
        actor.mechanics.action_fault = 800;
        assert_eq!(step(3.0, &actor).1, EquipmentSlot::Primary);
        assert_eq!(step(3.3, &actor).1, EquipmentSlot::Sidearm);
        actor.weapon_slot = EquipmentSlot::Sidearm;
        actor.mechanics.action_fault = 0;
        assert_eq!(step(4.0, &actor).0, Some("interrupted"));
        actor.weapon_slot = EquipmentSlot::Primary;
        assert_eq!(step(5.0, &actor).0, None);
        actor.mechanics.action_fault = 1;
        assert_eq!(step(6.0, &actor).2, 1);
        assert_eq!(step(6.3, &actor).2, 2);
        assert_eq!(step(7.0, &actor).0, None);
        actor.mechanics.action_fault = 750;
        assert_eq!(step(8.0, &actor).0, None);
        actor.mechanics.action_fault = 0;
        assert_eq!(step(9.0, &actor).0, Some("return"));
    }
}
