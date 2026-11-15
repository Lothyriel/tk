//! Snapshot-driven angular presentation. Events never inject angular recoil:
//! authority already counted every authorized shot, including coalesced packets.
use bevy::prelude::*;
use common::recoil::RecoilState;

#[derive(Default)]
pub(super) struct Presentation {
    state: RecoilState,
    sequence: u64,
    received: f64,
    rate: f32,
    correction: Vec2,
}

impl Presentation {
    pub fn observe(&mut self, state: RecoilState, sequence: u64, now: f64, age: f32, rate: f32) {
        if sequence <= self.sequence {
            return;
        }
        let previous = self.angles(now);
        self.state = state;
        self.state.advance(age.clamp(0.0, 0.15), rate);
        self.correction = previous - Vec2::from(self.state.angles);
        self.sequence = sequence;
        self.received = now;
        self.rate = rate;
    }

    pub fn angles(&self, now: f64) -> Vec2 {
        let elapsed = (now - self.received).max(0.0) as f32;
        let mut state = self.state;
        state.advance(elapsed, self.rate);
        Vec2::from(state.angles) + self.correction * (-elapsed * 40.0).exp()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_reconcile_continuously_without_reapplying_duplicate_impulses() {
        let state = RecoilState {
            angles: [0.008, 0.05],
            velocity: [0.02, 0.3],
            impulses: 3,
        };
        let mut reference = None;
        for hz in [30, 60, 165] {
            let mut view = Presentation::default();
            view.observe(state, 4, 0.0, 0.0, 14.0);
            assert_eq!(view.angles(0.0), Vec2::ZERO, "no snapshot jump");
            for frame in 1..=hz {
                let now = frame as f64 / hz as f64;
                view.observe(state, 4, now, 0.0, 14.0);
                view.observe(RecoilState::default(), 3, now, 0.0, 14.0);
            }
            let end = view.angles(1.0);
            if let Some(expected) = reference {
                assert!(end.distance(expected) < 1e-7);
            } else {
                reference = Some(end);
            }
            let before = view.angles(1.1);
            view.observe(state, 6, 1.1, 0.08, 14.0);
            assert!(view.angles(1.1).distance(before) < 1e-7);
            let mut projected = state;
            projected.advance(0.08 + 0.3, 14.0);
            assert!(view.angles(1.4).distance(projected.angles.into()) < 1e-6);
        }
    }
}
