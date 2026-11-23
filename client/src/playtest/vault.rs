//! No-capture UDP traversal route: ordinary input, confirmed start/clear/land.
use bevy::prelude::*;
use common::{ClientData, ClientInput, humanoid::PhysicalMode, map::POV_STUMBLE_BLOCK};

#[derive(Default)]
pub(super) struct Route {
    identity: Option<(u32, u64, u8)>,
    start: Option<u64>,
    clear: Option<u64>,
    land: Option<u64>,
    active: bool,
    episodes: usize,
    max_progress: u8,
    settled_since: Option<f64>,
    requested: bool,
}

impl Route {
    pub fn update(
        &mut self,
        elapsed: f64,
        sequence: u64,
        raid: u32,
        actor: &ClientData,
        observer: bool,
    ) -> bool {
        assert!(elapsed < 15.0, "vault route timed out");
        let identity = (raid, actor.id, actor.life);
        assert_eq!(
            *self.identity.get_or_insert(identity),
            identity,
            "vault actor changed"
        );
        assert!(Vec3::from(actor.pos).is_finite());
        assert!(
            matches!(
                actor.pose.mode,
                PhysicalMode::Standing | PhysicalMode::Stumbling
            ),
            "unexpected physical fall"
        );
        if actor.pose.vault != 0 && !self.active {
            self.start.get_or_insert(sequence);
            self.episodes += 1;
        }
        self.max_progress = self.max_progress.max(actor.pose.vault);
        if actor.pose.vault == 0 && self.active {
            self.clear = Some(sequence);
        }
        self.active = actor.pose.vault != 0;
        assert!(self.episodes <= 1, "held Space chained traversal");
        let far_side = POV_STUMBLE_BLOCK.center[2] - POV_STUMBLE_BLOCK.half_size[2] - 0.35;
        if self.clear.is_some() && actor.pose.grounded && actor.pos[2] < far_side {
            self.land.get_or_insert(sequence);
        }
        if self.land.is_some()
            && actor.pose.grounded
            && actor.pose.velocity[0].hypot(actor.pose.velocity[2]) < 0.1
        {
            return elapsed - *self.settled_since.get_or_insert(elapsed)
                > if observer { 0.8 } else { 1.4 };
        }
        self.settled_since = None;
        false
    }

    pub fn input(&mut self, elapsed: f64, actor: &ClientData, input: &mut ClientInput) {
        input.camera = default();
        input.forward = elapsed > 1.5 && self.start.is_none();
        input.run = input.forward;
        if input.forward
            && actor.pos[2] < POV_STUMBLE_BLOCK.center[2] + POV_STUMBLE_BLOCK.half_size[2] + 0.85
        {
            self.requested = true;
        }
        input.jump = self.requested; // Held through landing to prove no replay.
    }

    pub fn report(&self) -> serde_json::Value {
        let (raid, actor, life) = self.identity.unwrap();
        serde_json::json!({"raid":raid, "actor":actor, "life":life,
            "start":self.start.unwrap(), "clear":self.clear.unwrap(), "land":self.land.unwrap(),
            "episodes":self.episodes, "max_progress":self.max_progress})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_udp_route_requires_confirmed_start_clear_far_landing_and_settling() {
        let mut route = Route::default();
        let mut actor = ClientData {
            id: 7,
            pos: [0.0, 1.02, 8.0],
            ..default()
        };
        actor.pose.grounded = true;
        assert!(!route.update(1.0, 10, 1, &actor, true));
        let mut input = ClientInput::default();
        route.input(1.6, &actor, &mut input);
        assert!(input.jump && input.forward);
        actor.pose.set_vault(Some(0.2));
        assert!(!route.update(2.0, 12, 1, &actor, true));
        actor.pose.set_vault(Some(0.9));
        assert!(!route.update(2.4, 15, 1, &actor, true));
        actor.pose.vault = 0;
        actor.pose.grounded = false;
        actor.pos[2] = 5.5;
        assert!(!route.update(2.5, 16, 1, &actor, true));
        actor.pose.grounded = true;
        assert!(!route.update(2.8, 18, 1, &actor, true));
        assert!(route.update(3.7, 22, 1, &actor, true));
        route.input(3.7, &actor, &mut input);
        assert!(input.jump && !input.forward);
        assert_eq!(route.report()["episodes"], 1);
    }
}
