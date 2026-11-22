//! Telemetry-only observer for the existing articulated recovery path.
use bevy::prelude::*;
use common::{
    PlayerId,
    humanoid::{PhysicalMode, Segment},
    raid::RaidPhase,
};
use serde_json::{Value, json};

use super::{Driver, Step};
use crate::{render::actors::PoseCache, sync::ClientSession};

#[derive(Default)]
pub(super) struct Review {
    identity: Option<(u32, u64, u8)>,
    fallen: Option<(u64, Vec<u64>)>,
    recovering: Option<u64>,
    standing: Option<(u64, f32)>,
    previous: Option<(f64, u64)>,
    gate: Supports,
    done: bool,
}

#[derive(Default)]
struct Supports {
    low: [bool; 2],
    active: [Option<usize>; 2],
    intervals: Vec<Interval>,
}

#[derive(Clone, Copy)]
struct Frame {
    time: f64,
    snapshot: u64,
    foot: Vec3,
    anchor: Vec3,
    confirmed: Vec3,
    chest: Quat,
}

struct Interval {
    side: usize,
    start_snapshot: u64,
    end_snapshot: Option<u64>,
    frames: Vec<Frame>,
}

impl Supports {
    fn sample(
        &mut self,
        snapshot: u64,
        moving: bool,
        weights: [f32; 2],
        frames: [Option<Frame>; 2],
    ) {
        for side in 0..2 {
            if weights[side] <= 0.95 {
                self.low[side] = true;
                if let Some(index) = self.active[side].take() {
                    self.intervals[index].end_snapshot = Some(snapshot);
                }
                continue;
            }
            if self.low[side] {
                self.low[side] = false;
                if moving && self.intervals.len() < 2 {
                    self.active[side] = Some(self.intervals.len());
                    self.intervals.push(Interval {
                        side,
                        start_snapshot: snapshot,
                        end_snapshot: None,
                        frames: Vec::new(),
                    });
                }
            }
            if let Some(index) = self.active[side] {
                let frames_out = &mut self.intervals[index].frames;
                // A late presentation may settle into the confirmed support, but
                // it must not disappear and then be stitched to a later sample.
                match frames[side] {
                    Some(frame) => frames_out.push(frame),
                    None if !frames_out.is_empty() => {
                        panic!("presented contact broke inside an early support");
                    }
                    None => {}
                }
            }
        }
    }

    fn report(&self) -> Option<Vec<Value>> {
        if self.intervals.len() != 2 || self.intervals.iter().any(|i| i.end_snapshot.is_none()) {
            return None;
        }
        assert_ne!(
            self.intervals[0].side, self.intervals[1].side,
            "first supports did not alternate"
        );
        assert!(
            self.intervals[0].start_snapshot < self.intervals[1].start_snapshot,
            "first supports have ambiguous ordering"
        );
        Some(self.intervals.iter().map(Interval::report).collect())
    }
}

impl Interval {
    fn report(&self) -> Value {
        assert!(
            self.frames.len() >= 3,
            "early support lacks two presented pairs: side={} snapshot={} frames={}",
            self.side,
            self.start_snapshot,
            self.frames.len()
        );
        let first = self.frames[0];
        let mut peak = first;
        let mut chest_angle = 0.0_f32;
        let mut max_gap = 0.0_f64;
        for pair in self.frames.windows(2) {
            let dt = pair[1].time - pair[0].time;
            assert!(dt > 0.0 && dt < 0.05, "gap inside first support: {dt}");
            max_gap = max_gap.max(dt);
            let angle = 2.0 * pair[0].chest.dot(pair[1].chest).abs().min(1.0).acos();
            chest_angle = chest_angle.max(angle);
            if pair[1].foot.distance(first.foot) > peak.foot.distance(first.foot) {
                peak = pair[1];
            }
        }
        json!({
            "side": self.side, "confirmed_start": self.start_snapshot, "confirmed_end": self.end_snapshot,
            "presented_start": first.snapshot, "presented_end": self.frames.last().unwrap().snapshot,
            "presented_start_seconds": first.time, "presented_end_seconds": self.frames.last().unwrap().time,
            "pairs": self.frames.len() - 1, "max_frame_seconds": max_gap,
            "peak_snapshot": peak.snapshot, "foot_m": first.foot.distance(peak.foot),
            "presented_anchor_m": first.anchor.distance(peak.anchor),
            "confirmed_anchor_m": first.confirmed.distance(peak.confirmed),
            "peak_chest_pair_rad": chest_angle,
        })
    }
}

pub(super) fn observe(
    driver: Res<Driver>,
    time: Res<Time<Real>>,
    session: Res<ClientSession>,
    id: Res<PlayerId>,
    poses: Res<PoseCache>,
    mut review: Local<Review>,
    mut exit: MessageWriter<AppExit>,
) {
    if driver.step != Step::Pov || review.done {
        return;
    }
    let now = time.elapsed_secs_f64();
    assert!(
        now - driver.since < 15.0,
        "recovery steps timed out; matched intervals={}",
        review.gate.intervals.len()
    );
    let Some(actor) = session
        .players
        .iter()
        .filter(|p| !p.bot)
        .min_by_key(|p| p.id)
    else {
        assert!(review.identity.is_none(), "recovery actor disappeared");
        return;
    };
    let identity = (session.raid.raid_id, actor.id, actor.life);
    assert_eq!(*review.identity.get_or_insert(identity), identity);
    assert!(session.raid.phase == RaidPhase::Active && actor.alive);
    let snapshot = session.last_sequence;
    let mut ids: Vec<_> = session
        .physics_pieces
        .iter()
        .filter(|p| p.owner == actor.id && !p.detached)
        .map(|p| p.id)
        .collect();
    ids.sort_unstable();
    match actor.pose.mode {
        PhysicalMode::Fallen if review.fallen.is_none() && actor.pose.physical.is_some() => {
            assert!(!ids.is_empty());
            review.fallen = Some((snapshot, ids));
        }
        PhysicalMode::Recovering => {
            let (fall, initial) = review.fallen.as_ref().expect("skipped articulated fall");
            assert!(snapshot > *fall && actor.pose.physical.is_some());
            assert_eq!(&ids, initial, "get-up replaced attached bodies");
            review.recovering.get_or_insert(snapshot);
        }
        PhysicalMode::Standing if let Some(recovering) = review.recovering => {
            assert!(actor.pose.physical.is_none() && ids.is_empty() && actor.pose.grounded);
            assert!(snapshot > recovering);
            let (standing, start_z) = *review.standing.get_or_insert((snapshot, actor.pos[2]));
            let travelled = start_z - actor.pos[2];
            if actor.id == id.0 {
                if now - driver.recovery_walk.unwrap_or(f64::INFINITY) > 2.4 {
                    assert!(travelled > 0.5);
                    info!("POV PASS recovery-steps t={:.3}", now - driver.since);
                    exit.write(AppExit::Success);
                    review.done = true;
                }
                return;
            }
            if let Some((last_time, last_snapshot)) = review.previous {
                assert!(snapshot >= last_snapshot && now > last_time);
                assert!(
                    now - last_time < 0.05,
                    "observer frame gap before completing first supports"
                );
            }
            assert!(
                now - session.last_received < 0.10,
                "stale recovery confirmation"
            );
            review.previous = Some((now, snapshot));
            let root = Transform::from_translation(actor.pos.into())
                .with_rotation(Quat::from_rotation_y(actor.rot.yaw));
            let floor =
                common::map::BLOCKS[0].center[1] + common::map::BLOCKS[0].half_size[1] + 0.055;
            let in_contact = |p: Vec3| p.is_finite() && (p.y - floor).abs() <= 0.02;
            let frames = [Segment::LeftFoot, Segment::RightFoot].map(|foot| {
                let i = usize::from(foot == Segment::RightFoot);
                let (_, weights) = poses.presented_support(actor.id)?;
                let foot = poses.presented_bone(actor.id, foot)?.translation;
                let anchor = poses.presented_foot_anchors(actor.id)?[i];
                let confirmed = root.transform_point(actor.pose.feet[i].into());
                let chest = poses.presented_bone(actor.id, Segment::Thorax)?.rotation;
                (weights[i] > 0.95
                    && in_contact(foot)
                    && in_contact(anchor)
                    && in_contact(confirmed)
                    && chest.is_finite())
                .then_some(Frame {
                    time: now,
                    snapshot,
                    foot,
                    anchor,
                    confirmed,
                    chest: chest.normalize(),
                })
            });
            if review
                .gate
                .intervals
                .iter()
                .any(|i| i.end_snapshot.is_none())
                || review.gate.intervals.len() < 2
            {
                review.gate.sample(
                    snapshot,
                    actor.pose.velocity[2] < -0.08,
                    actor.pose.foot_weight,
                    frames,
                );
            }
            if travelled > 0.5
                && let Some(supports) = review.gate.report()
            {
                let (fall, ids) = review.fallen.as_ref().unwrap();
                let report = json!({
                    "raid": identity.0, "actor": identity.1, "life": identity.2, "fall": fall, "recovering": recovering,
                    "standing": standing, "attached_ids": ids, "distance_m": travelled, "supports": supports,
                });
                let root = std::path::PathBuf::from(std::env::var_os("TK_PLAYTEST_DIR").unwrap());
                std::fs::write(root.join("recovery-steps.json"), report.to_string()).unwrap();
                info!("POV OBSERVER PASS recovery-steps");
                exit.write(AppExit::Success);
                review.done = true;
            }
        }
        _ => assert!(
            review.standing.is_none(),
            "actor left Standing during the first steps"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record_first_two(gate: &mut Supports, first_frames: bool) {
        gate.sample(1, false, [0.0; 2], [None; 2]);
        for (snapshot, side) in [(2, 0), (6, 1)] {
            for i in snapshot..snapshot + 3 {
                let mut frames = [None; 2];
                let mut weights = [0.0; 2];
                weights[side] = 1.0;
                if side != 0 || first_frames {
                    frames[side] = Some(Frame {
                        time: i as f64 * 0.016,
                        snapshot: i,
                        foot: Vec3::X * (i - snapshot) as f32 * 0.001,
                        anchor: Vec3::ZERO,
                        confirmed: Vec3::ZERO,
                        chest: Quat::IDENTITY,
                    });
                }
                gate.sample(i, true, weights, frames);
            }
            gate.sample(snapshot + 3, true, [0.0; 2], [None; 2]);
        }
    }

    #[test]
    fn recovery_steps_reports_first_alternating_supports() {
        let mut gate = Supports::default();
        record_first_two(&mut gate, true);
        let supports = gate.report().unwrap();
        assert_eq!(supports[0]["confirmed_start"], 2);
        assert_eq!(supports[1]["confirmed_start"], 6);
        assert_eq!(supports[0]["pairs"], 2);
        assert!((supports[0]["foot_m"].as_f64().unwrap() - 0.002).abs() < 1e-8);
    }

    #[test]
    #[should_panic(expected = "early support lacks two presented pairs")]
    fn recovery_steps_never_substitutes_a_later_support() {
        let mut gate = Supports::default();
        record_first_two(&mut gate, false);
        gate.sample(10, true, [1.0, 0.0], [None; 2]);
        gate.report();
    }
}
