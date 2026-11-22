//! Short POV route driven by confirmed raid state, not animation timers.
use bevy::prelude::*;
use common::humanoid::Segment;
use std::{
    io::{BufWriter, Write},
    time::{Duration, Instant},
};

pub(super) fn trace_observer(
    driver: Res<super::Driver>,
    time: Res<Time<Real>>,
    session: Res<crate::sync::ClientSession>,
    id: Res<common::PlayerId>,
    poses: Res<crate::render::actors::PoseCache>,
    mut file: Local<Option<BufWriter<std::fs::File>>>,
) {
    if driver.step != super::Step::Pov {
        return;
    }
    let Some(actor) = session.players.iter().find(|p| p.id != id.0 && !p.bot) else {
        return;
    };
    let Some((phase, weights)) = poses.presented_support(actor.id) else {
        return;
    };
    let [Some(left), Some(right), Some(pelvis), Some(chest)] = [
        Segment::LeftFoot,
        Segment::RightFoot,
        Segment::Pelvis,
        Segment::Thorax,
    ]
    .map(|bone| poses.presented_bone(actor.id, bone)) else {
        return;
    };
    let Some(anchors) = poses.presented_foot_anchors(actor.id) else {
        return;
    };
    let root = Transform::from_translation(actor.pos.into())
        .with_rotation(Quat::from_rotation_y(actor.rot.yaw));
    let confirmed = actor
        .pose
        .feet
        .map(|foot| root.transform_point(foot.into()));
    let out = file.get_or_insert_with(|| {
        let root = std::path::PathBuf::from(std::env::var("TK_PLAYTEST_DIR").expect("playtest dir"));
        let mut out = BufWriter::new(std::fs::File::create(root.join("gait-trace.csv")).unwrap());
        writeln!(out, "seconds,snapshot,phase,roll,speed,left_weight,right_weight,left_x,left_y,left_z,right_x,right_y,right_z,pelvis_x,pelvis_y,pelvis_z,chest_qx,chest_qy,chest_qz,chest_qw,left_anchor_x,left_anchor_y,left_anchor_z,right_anchor_x,right_anchor_y,right_anchor_z,left_confirmed_x,left_confirmed_y,left_confirmed_z,right_confirmed_x,right_confirmed_y,right_confirmed_z").unwrap();
        out
    });
    let [vx, _, vz] = actor.pose.velocity;
    write!(
        out,
        "{:.6},{},{phase:.6},{:.6},{:.6},{:.6},{:.6}",
        time.elapsed_secs_f64() - driver.since,
        session.last_sequence,
        actor.pose.aim_roll,
        vx.hypot(vz),
        weights[0],
        weights[1]
    )
    .unwrap();
    for value in left
        .translation
        .to_array()
        .into_iter()
        .chain(right.translation.to_array())
        .chain(pelvis.translation.to_array())
        .chain(chest.rotation.to_array())
        .chain(anchors.into_iter().flat_map(|v| v.to_array()))
        .chain(confirmed.into_iter().flat_map(|v| v.to_array()))
    {
        write!(out, ",{value:.6}").unwrap();
    }
    writeln!(out).unwrap();
    if driver.pov_observed == 3 {
        out.flush().unwrap();
    }
}

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
    eye: Single<(&Camera, &GlobalTransform, &Transform), With<crate::render::WorldModelCamera>>,
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
            "seconds,frame_seconds,stage,eye_x,eye_y,eye_z,roll,input_roll,marker_ndc_x,marker_ndc_y"
        )
        .unwrap();
        out
    });
    let [x, y, z] = eye.1.translation().to_array();
    let (_, _, roll) = eye.2.rotation.to_euler(EulerRot::YXZ);
    // A fixed scene reference in normalized screen coordinates; this
    // measures framing, not whether another mesh or the HUD occludes it.
    let marker = eye
        .0
        .world_to_ndc(eye.1, Vec3::from(common::map::POV_MARKER))
        .map_or(Vec2::splat(f32::NAN), |p| p.truncate());
    writeln!(
        out,
        "{:.6},{:.6},{},{x:.6},{y:.6},{z:.6},{roll:.6},{:.6},{:.6},{:.6}",
        time.elapsed_secs_f64() - driver.since,
        time.delta_secs_f64(),
        if driver.pov_stride != ArmedStride::Rest {
            format!("{:?}", driver.pov_stride)
        } else {
            format!("{:?}", driver.pov_stop_lean)
        },
        input.camera.roll,
        marker.x,
        marker.y
    )
    .unwrap();
    if driver.pov_stop_lean == StopLean::Finish || driver.pov_stride == ArmedStride::Finish {
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
pub(super) enum ArmedStride {
    #[default]
    Rest,
    Walk,
    Run,
    Turn,
    RecoverAim,
    Stop,
    Finish,
}

impl ArmedStride {
    const TURN_YAW: f32 = -0.40;

    fn turned(player: &common::ClientData) -> bool {
        player.rot.yaw <= -0.30
            && player.pose.velocity[0].hypot(player.pose.velocity[2]) > 4.3
            && player.pose.ads < 0.05
    }

    fn moving_ads(player: &common::ClientData) -> bool {
        let [x, _, z] = player.pose.velocity;
        (1.5..3.7).contains(&x.hypot(z)) && player.pose.ads > 0.90
    }

    pub(super) fn advance(
        &mut self,
        elapsed: f64,
        ready: bool,
        player: &common::ClientData,
    ) -> Result<Option<&'static str>, &'static str> {
        if elapsed >= 15.0 {
            return Err("armed stride did not reach its confirmed state before timeout");
        }
        if !ready {
            return Ok(None);
        }
        let [x, _, z] = player.pose.velocity;
        let speed = x.hypot(z);
        let (next, cue) = match self {
            Self::Rest => (Self::Walk, "pov-rest"),
            Self::Walk if (2.5..3.7).contains(&speed) && player.pos[2] < 8.0 => {
                (Self::Run, "pov-walk")
            }
            Self::Run if speed > 4.6 && player.pose.ads < 0.05 => (Self::Turn, "pov-run"),
            Self::Turn
                if Self::turned(player)
                    && player.rot.yaw <= -0.39
                    && player.pose.phase.sin().abs() > 0.5 =>
            {
                (Self::RecoverAim, "pov-turn")
            }
            Self::RecoverAim if Self::moving_ads(player) && player.pose.ads > 0.98 => {
                (Self::Stop, "pov-moving-ads")
            }
            Self::Stop if speed < 0.08 && player.pose.ads < 0.02 => (Self::Finish, "pov-return"),
            _ => return Ok(None),
        };
        *self = next;
        Ok(Some(cue))
    }

    pub(super) fn input(&self, input: &mut common::ClientInput, turn: &mut f32, dt: f32) {
        input.forward = matches!(self, Self::Walk | Self::Run | Self::Turn | Self::RecoverAim);
        input.run = matches!(self, Self::Run | Self::Turn);
        input.aim = matches!(self, Self::Turn | Self::RecoverAim);
        if matches!(
            self,
            Self::Turn | Self::RecoverAim | Self::Stop | Self::Finish
        ) {
            *turn = (*turn - 1.8 * dt).max(Self::TURN_YAW);
        }
        input.camera.yaw = *turn;
    }

    pub(super) fn observe(stage: u8, player: &common::ClientData) -> Option<&'static str> {
        let [x, _, z] = player.pose.velocity;
        match stage {
            0 if Self::turned(player) => Some("turn"),
            1 if Self::moving_ads(player) => Some("moving-ads"),
            2 if x.hypot(z) < 0.08 && player.pose.ads < 0.02 => Some("return"),
            _ => None,
        }
    }
}

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
        side_cover: bool,
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
        let settled_lean = |roll: f32| {
            if side_cover {
                (0.20..0.29).contains(&roll)
            } else {
                roll > 0.29
            }
        };
        let (next, capture) = match self {
            Self::Rest => (Self::Walk, "pov-rest"),
            Self::Walk if speed >= 2.5 && player.pos[2] < 8.0 => (Self::Run, "pov-walk"),
            // Brake just before the near face of the low concrete wall at z=-2.4.
            Self::Run if speed >= 4.5 && player.pos[2] < -0.7 => (Self::Brake, "pov-run"),
            Self::Brake if speed < 0.08 => (Self::Crouch, "pov-stop"),
            Self::Crouch if pose.crouch > 0.95 => (Self::Left, "pov-crouch"),
            Self::Left if settled_lean(-pose.aim_roll) => (Self::Right, "pov-left"),
            Self::Right if settled_lean(pose.aim_roll) => (Self::Neutral, "pov-right"),
            Self::Neutral if pose.aim_roll.abs() < 0.001 => (Self::Finish, "pov-return"),
            _ => return Ok(None),
        };
        *self = next;
        Ok(Some(capture))
    }

    pub(super) fn input(&self, input: &mut common::ClientInput, previous_roll: f32, dt: f32) {
        input.forward = matches!(self, Self::Walk | Self::Run);
        input.run = *self == Self::Run;
        input.crouch = matches!(
            self,
            Self::Crouch | Self::Left | Self::Right | Self::Neutral | Self::Finish
        );
        input.camera.roll =
            crate::input::step_lean(previous_roll, *self == Self::Right, *self == Self::Left, dt);
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

/// The second half of the combined route, after StopLean returns to neutral.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) enum Handling {
    #[default]
    Aim,
    MovingAds,
    Release,
    Inspect,
    Cancel,
    Swap,
    Finish,
}

impl Handling {
    pub(super) fn advance(
        &mut self,
        elapsed: f64,
        ready: bool,
        player: &common::ClientData,
        fov: f32,
    ) -> Result<Option<&'static str>, &'static str> {
        use common::{EquipmentSlot, catalog::ManipulationKind};
        if elapsed > 20.0 {
            return Err("handling route timed out before confirmed return");
        }
        let mechanics = player.mechanics;
        let ads = player.pose.ads;
        let idle = mechanics.equipped && mechanics.kind.is_none();
        let hip = ads < 0.02 && (fov - 70.0).abs() < 2.0;
        let aimed = ready && ads > 0.95 && (fov - 44.0).abs() < 1.0;
        let [vx, _, vz] = player.pose.velocity;
        let (next, capture) = match self {
            Self::Aim if aimed && vx.hypot(vz) < 0.08 => (Self::MovingAds, "pov-ads"),
            Self::MovingAds if aimed && vz > 0.6 => (Self::Release, "pov-moving-ads"),
            Self::Release if ready && idle && hip && vx.hypot(vz) < 0.08 => {
                (Self::Inspect, "pov-hip")
            }
            Self::Inspect
                if ready
                    && mechanics.kind == Some(ManipulationKind::Inspect)
                    && (0.25..0.6).contains(&mechanics.fraction) =>
            {
                (Self::Cancel, "pov-inspect")
            }
            // Re-aiming interrupts inspect. Require new ADS before switching
            // so the swap cannot mask a missing cancellation.
            Self::Cancel if ready && idle && ads > 0.25 => (Self::Swap, "pov-cancel"),
            Self::Swap if ready && idle && hip && player.weapon_slot == EquipmentSlot::Sidearm => {
                (Self::Finish, "pov-return")
            }
            _ => return Ok(None),
        };
        *self = next;
        Ok(Some(capture))
    }

    pub(super) fn input(&self, input: &mut common::ClientInput) {
        input.crouch = true;
        input.aim = matches!(self, Self::Aim | Self::MovingAds | Self::Cancel);
        input.backward = *self == Self::MovingAds;
        if matches!(self, Self::Swap | Self::Finish) {
            input.weapon = common::EquipmentSlot::Sidearm;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn armed_stride_requires_turned_running_phase_and_moving_ads_confirmation() {
        let mut route = ArmedStride::default();
        let mut player = common::ClientData::default();
        let mut input = common::ClientInput::default();
        let mut yaw = 0.0;
        assert_eq!(route.advance(0.0, false, &player), Ok(None));
        assert_eq!(route.advance(1.0, true, &player), Ok(Some("pov-rest")));
        player.pose.velocity[2] = -3.1;
        assert_eq!(route.advance(2.0, true, &player), Ok(Some("pov-walk")));
        player.pose.velocity[2] = -5.0;
        assert_eq!(route.advance(2.5, true, &player), Ok(Some("pov-run")));
        route.input(&mut input, &mut yaw, 0.05);
        assert!(input.run && input.aim && input.forward && yaw < 0.0);
        assert_eq!(route.advance(2.6, true, &player), Ok(None));
        player.rot.yaw = -0.40;
        assert_eq!(ArmedStride::observe(0, &player), Some("turn"));
        assert_eq!(
            route.advance(2.7, true, &player),
            Ok(None),
            "wait for mid-stride"
        );
        player.pose.phase = 1.0;
        assert_eq!(route.advance(2.8, true, &player), Ok(Some("pov-turn")));
        route.input(&mut input, &mut yaw, 0.05);
        assert!(!input.run && input.aim && input.forward);
        player.pose.ads = 1.0;
        assert_eq!(route.advance(2.9, true, &player), Ok(None), "still running");
        player.pose.velocity[2] = -3.1;
        assert_eq!(ArmedStride::observe(1, &player), Some("moving-ads"));
        assert_eq!(
            route.advance(3.3, true, &player),
            Ok(Some("pov-moving-ads"))
        );
        route.input(&mut input, &mut yaw, 0.05);
        assert!(!input.forward && !input.aim);
        assert_eq!(ArmedStride::observe(2, &player), None);
        player.pose.ads = 0.0;
        player.pose.velocity[2] = 0.0;
        assert_eq!(ArmedStride::observe(2, &player), Some("return"));
        assert_eq!(route.advance(4.0, true, &player), Ok(Some("pov-return")));
        assert!(route.advance(15.0, true, &player).is_err());
    }

    #[test]
    fn handling_route_cancels_inspect_before_requesting_swap() {
        use common::{EquipmentSlot, catalog::ManipulationKind};
        let mut route = Handling::default();
        let mut player = common::ClientData::default();
        let mut input = common::ClientInput::default();
        player.mechanics.equipped = true;
        player.pose.ads = 1.0;
        assert_eq!(route.advance(5.0, true, &player, 50.0), Ok(None));
        assert_eq!(route.advance(5.1, true, &player, 44.0), Ok(Some("pov-ads")));
        route.input(&mut input);
        assert!(input.aim && input.backward);
        assert_eq!(route.advance(5.2, true, &player, 44.0), Ok(None));
        player.pose.velocity[2] = 1.0;
        player.pose.ads = 0.5;
        assert_eq!(route.advance(5.3, true, &player, 44.0), Ok(None));
        player.pose.ads = 1.0;
        assert_eq!(
            route.advance(5.4, true, &player, 44.0),
            Ok(Some("pov-moving-ads"))
        );
        player.pose.ads = 0.0;
        assert_eq!(route.advance(5.45, true, &player, 70.0), Ok(None));
        player.pose.velocity[2] = 0.0;
        assert_eq!(route.advance(5.5, true, &player, 70.0), Ok(Some("pov-hip")));
        player.mechanics.kind = Some(ManipulationKind::Inspect);
        player.mechanics.fraction = 0.3;
        assert_eq!(
            route.advance(6.4, true, &player, 70.0),
            Ok(Some("pov-inspect"))
        );
        route.input(&mut input);
        assert!(input.aim);
        assert_eq!(input.weapon, EquipmentSlot::Primary);
        player.pose.ads = 0.3;
        assert_eq!(route.advance(6.5, true, &player, 65.0), Ok(None));
        player.mechanics.kind = None;
        assert_eq!(
            route.advance(6.6, true, &player, 65.0),
            Ok(Some("pov-cancel"))
        );
        route.input(&mut input);
        assert!(!input.aim);
        assert_eq!(input.weapon, EquipmentSlot::Sidearm);
        player.pose.ads = 0.0;
        assert_eq!(route.advance(7.0, true, &player, 70.0), Ok(None));
        player.weapon_slot = EquipmentSlot::Sidearm;
        assert_eq!(route.advance(7.1, false, &player, 70.0), Ok(None));
        assert_eq!(
            route.advance(7.5, true, &player, 70.0),
            Ok(Some("pov-return"))
        );
    }

    #[test]
    fn pov_stop_lean_requires_braking_and_both_confirmed_roll_signs() {
        let mut route = StopLean::Run;
        let mut player = common::ClientData {
            pos: [0.0, 1.0, -0.8],
            ..Default::default()
        };
        player.pose.velocity[2] = -5.0;
        assert_eq!(
            route.advance(2.0, true, &player, false),
            Ok(Some("pov-run"))
        );
        assert_eq!(route.advance(2.1, true, &player, false), Ok(None));
        player.pose.velocity[2] = 0.0;
        assert_eq!(
            route.advance(2.4, true, &player, false),
            Ok(Some("pov-stop"))
        );
        player.pose.crouch = 1.0;
        assert_eq!(
            route.advance(2.8, true, &player, false),
            Ok(Some("pov-crouch"))
        );
        player.pose.aim_roll = -0.3;
        assert_eq!(
            route.advance(3.0, true, &player, false),
            Ok(Some("pov-left"))
        );
        assert_eq!(route.advance(3.1, true, &player, false), Ok(None));
        player.pose.aim_roll = 0.3;
        assert_eq!(
            route.advance(3.2, true, &player, false),
            Ok(Some("pov-right"))
        );
        player.pose.aim_roll = 0.0;
        assert_eq!(
            route.advance(3.4, true, &player, false),
            Ok(Some("pov-return"))
        );
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
