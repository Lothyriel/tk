//! Short POV route driven by confirmed raid state, not animation timers.
use bevy::prelude::*;
use std::{
    io::{BufWriter, Write},
    time::{Duration, Instant},
};

#[derive(Resource)]
pub(super) struct Cadence(Duration);

impl Cadence {
    pub(super) fn new(hz: u32) -> Self {
        Self(Duration::from_secs_f64(1.0 / f64::from(hz)))
    }
}

pub(super) fn pace(cadence: Res<Cadence>, mut last: Local<Option<Instant>>) {
    if let Some(last) = *last {
        std::thread::sleep(cadence.0.saturating_sub(last.elapsed()));
    }
    *last = Some(Instant::now());
}

pub(super) fn trace(
    driver: Res<super::Driver>,
    time: Res<Time<Real>>,
    eye: Single<(&GlobalTransform, &Transform), With<crate::render::WorldModelCamera>>,
    input: Res<common::LocalInput>,
    mut file: Local<Option<BufWriter<std::fs::File>>>,
) {
    if driver.step != super::Step::Pov {
        return;
    }
    let out = file.get_or_insert_with(|| {
        let root =
            std::path::PathBuf::from(std::env::var("TK_PLAYTEST_DIR").expect("playtest dir"));
        let mut out = BufWriter::new(std::fs::File::create(root.join("pov-trace.csv")).unwrap());
        writeln!(
            out,
            "seconds,frame_seconds,stage,eye_x,eye_y,eye_z,roll,input_roll"
        )
        .unwrap();
        out
    });
    let [x, y, z] = eye.0.translation().to_array();
    let (_, _, roll) = eye.1.rotation.to_euler(EulerRot::YXZ);
    writeln!(
        out,
        "{:.6},{:.6},{:?},{x:.6},{y:.6},{z:.6},{roll:.6},{:.6}",
        time.elapsed_secs_f64() - driver.since,
        time.delta_secs_f64(),
        driver.pov_stop_lean,
        input.camera.roll
    )
    .unwrap();
    if driver.pov_stop_lean == StopLean::Finish {
        out.flush().unwrap();
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) enum Ads {
    #[default]
    Rest,
    Aim,
    Release,
    Finish,
}

/// Resolved ECS state can precede GPU mesh/material preparation. Give the
/// ready assembly a short, bounded warm-up before sampling the first pose.
pub(super) const VIEW_WARMUP_SECONDS: f64 = 1.0;

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) enum StopLean {
    #[default]
    Rest,
    Walk,
    Run,
    Brake,
    Crouch,
    Left,
    Right,
    Neutral,
    Finish,
}

impl StopLean {
    pub(super) fn advance(
        &mut self,
        elapsed: f64,
        ready: bool,
        player: &common::ClientData,
    ) -> Result<Option<&'static str>, &'static str> {
        if elapsed >= 15.0 {
            return Err("POV stop/lean did not reach its confirmed state before timeout");
        }
        if !ready {
            return Ok(None);
        }
        let pose = &player.pose;
        let [x, _, z] = pose.velocity;
        let speed = x.hypot(z);
        let (next, capture) = match self {
            Self::Rest => (Self::Walk, "pov-rest"),
            Self::Walk if speed >= 2.5 && player.pos[2] < 8.0 => (Self::Run, "pov-walk"),
            // Brake just before the near face of the low concrete wall at z=-2.4.
            Self::Run if speed >= 4.5 && player.pos[2] < -0.7 => (Self::Brake, "pov-run"),
            Self::Brake if speed < 0.08 => (Self::Crouch, "pov-stop"),
            Self::Crouch if pose.crouch > 0.95 => (Self::Left, "pov-crouch"),
            Self::Left if pose.aim_roll < -0.24 => (Self::Right, "pov-left"),
            Self::Right if pose.aim_roll > 0.24 => (Self::Neutral, "pov-right"),
            Self::Neutral if pose.aim_roll.abs() < 0.02 => (Self::Finish, "pov-return"),
            _ => return Ok(None),
        };
        *self = next;
        Ok(Some(capture))
    }

    pub(super) fn input(&self, input: &mut common::ClientInput) {
        input.forward = matches!(self, Self::Walk | Self::Run);
        input.run = *self == Self::Run;
        input.crouch = matches!(
            self,
            Self::Crouch | Self::Left | Self::Right | Self::Neutral | Self::Finish
        );
        input.camera.roll = match self {
            Self::Left => -0.3,
            Self::Right => 0.3,
            _ => 0.0,
        };
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) enum Inspect {
    #[default]
    Rest,
    Start,
    Return,
    Finish,
}

impl Inspect {
    pub(super) fn advance(
        &mut self,
        elapsed: f64,
        ready: bool,
        snapshot: common::catalog::MechanicalSnapshot,
    ) -> Result<Option<&'static str>, &'static str> {
        use common::catalog::ManipulationKind;
        if elapsed >= 8.0 {
            return Err("POV inspect did not reach its confirmed state before timeout");
        }
        let (next, capture) = match self {
            Self::Rest if ready && snapshot.kind.is_none() => (Self::Start, "pov-rest"),
            Self::Start
                if ready
                    && snapshot.kind == Some(ManipulationKind::Inspect)
                    && snapshot.fraction >= 0.35 =>
            {
                (Self::Return, "pov-inspect")
            }
            Self::Return if ready && snapshot.kind.is_none() => (Self::Finish, "pov-return"),
            _ => return Ok(None),
        };
        *self = next;
        Ok(Some(capture))
    }
}

impl Ads {
    pub(super) fn advance(
        &mut self,
        elapsed: f64,
        grounded: bool,
        ads: f32,
        world_fov: f32,
    ) -> Result<Option<&'static str>, &'static str> {
        if elapsed >= 8.0 {
            return Err("POV ADS did not reach its confirmed pose before timeout");
        }
        if !grounded {
            return Ok(None);
        }
        let (next, capture) = match self {
            Self::Rest if ads < 0.02 && (world_fov - 70.0).abs() < 2.0 => (Self::Aim, "pov-rest"),
            Self::Aim if ads > 0.9 && (world_fov - 44.0).abs() < 2.0 => (Self::Release, "pov-ads"),
            Self::Release if ads < 0.02 && (world_fov - 70.0).abs() < 2.0 => {
                (Self::Finish, "pov-return")
            }
            _ => return Ok(None),
        };
        *self = next;
        Ok(Some(capture))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pov_stop_lean_requires_braking_and_both_confirmed_roll_signs() {
        let mut route = StopLean::Run;
        let mut player = common::ClientData {
            pos: [0.0, 1.0, -0.8],
            ..Default::default()
        };
        player.pose.velocity[2] = -5.0;
        assert_eq!(route.advance(2.0, true, &player), Ok(Some("pov-run")));
        assert_eq!(route.advance(2.1, true, &player), Ok(None));
        player.pose.velocity[2] = 0.0;
        assert_eq!(route.advance(2.4, true, &player), Ok(Some("pov-stop")));
        player.pose.crouch = 1.0;
        assert_eq!(route.advance(2.8, true, &player), Ok(Some("pov-crouch")));
        player.pose.aim_roll = -0.3;
        assert_eq!(route.advance(3.0, true, &player), Ok(Some("pov-left")));
        assert_eq!(route.advance(3.1, true, &player), Ok(None));
        player.pose.aim_roll = 0.3;
        assert_eq!(route.advance(3.2, true, &player), Ok(Some("pov-right")));
        player.pose.aim_roll = 0.0;
        assert_eq!(route.advance(3.4, true, &player), Ok(Some("pov-return")));
    }

    #[test]
    fn pov_inspect_waits_for_authoritative_action_and_return() {
        use common::catalog::{ManipulationKind, MechanicalSnapshot};
        let mut route = Inspect::default();
        let mut snapshot = MechanicalSnapshot::default();
        assert_eq!(route.advance(0.5, true, snapshot), Ok(Some("pov-rest")));
        assert_eq!(route.advance(1.0, true, snapshot), Ok(None));
        snapshot.kind = Some(ManipulationKind::Inspect);
        snapshot.fraction = 0.2;
        assert_eq!(route.advance(1.1, true, snapshot), Ok(None));
        snapshot.fraction = 0.4;
        assert_eq!(route.advance(1.3, true, snapshot), Ok(Some("pov-inspect")));
        assert_eq!(route.advance(1.5, true, snapshot), Ok(None));
        snapshot.kind = None;
        assert_eq!(route.advance(2.0, true, snapshot), Ok(Some("pov-return")));
    }

    #[test]
    fn pov_ads_requires_confirmed_state_and_matching_render_lens() {
        let mut route = Ads::default();
        assert_eq!(route.advance(0.1, false, 0.0, 70.0), Ok(None));
        assert_eq!(route.advance(0.2, true, 0.0, 70.0), Ok(Some("pov-rest")));
        assert_eq!(route.advance(0.3, true, 0.95, 70.0), Ok(None));
        assert_eq!(route.advance(0.4, true, 0.0, 44.0), Ok(None));
        assert_eq!(route.advance(0.45, false, 0.95, 44.0), Ok(None));
        assert_eq!(route.advance(0.5, true, 0.95, 44.0), Ok(Some("pov-ads")));
        assert_eq!(route.advance(0.6, true, 0.0, 44.0), Ok(None));
        assert_eq!(route.advance(0.7, true, 0.0, 70.0), Ok(Some("pov-return")));
    }

    #[test]
    fn pov_ads_times_out_without_advancing_a_missing_state() {
        let mut route = Ads::default();
        assert!(route.advance(8.0, false, 0.0, 70.0).is_err());
        assert_eq!(route, Ads::Rest);
    }
}
