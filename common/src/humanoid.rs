//! Original TK rig. Metres, actor-local Y up / forward -Z. Shared by damage and art.
use bevy::prelude::*;
use rkyv::{Archive, Deserialize, Serialize};

pub const SEGMENT_COUNT: usize = 17;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Archive, Serialize, Deserialize)]
#[repr(u8)]
pub enum Segment {
    Pelvis,
    Abdomen,
    Thorax,
    Neck,
    Head,
    LeftUpperArm,
    LeftForearm,
    LeftHand,
    RightUpperArm,
    RightForearm,
    RightHand,
    LeftThigh,
    LeftShin,
    LeftFoot,
    RightThigh,
    RightShin,
    RightFoot,
}

pub const SEGMENTS: [Segment; SEGMENT_COUNT] = [
    Segment::Pelvis,
    Segment::Abdomen,
    Segment::Thorax,
    Segment::Neck,
    Segment::Head,
    Segment::LeftUpperArm,
    Segment::LeftForearm,
    Segment::LeftHand,
    Segment::RightUpperArm,
    Segment::RightForearm,
    Segment::RightHand,
    Segment::LeftThigh,
    Segment::LeftShin,
    Segment::LeftFoot,
    Segment::RightThigh,
    Segment::RightShin,
    Segment::RightFoot,
];

impl Segment {
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn parent(self) -> Option<Self> {
        use Segment::*;
        Some(match self {
            Pelvis => return None,
            Abdomen => Pelvis,
            Thorax => Abdomen,
            Neck => Thorax,
            Head => Neck,
            LeftUpperArm | RightUpperArm => Thorax,
            LeftForearm => LeftUpperArm,
            LeftHand => LeftForearm,
            RightForearm => RightUpperArm,
            RightHand => RightForearm,
            LeftThigh | RightThigh => Pelvis,
            LeftShin => LeftThigh,
            LeftFoot => LeftShin,
            RightShin => RightThigh,
            RightFoot => RightShin,
        })
    }
    pub fn limb(self) -> bool {
        self.index() >= 5
    }
    pub fn half_size(self) -> Vec3 {
        use Segment::*;
        match self {
            Pelvis => Vec3::new(0.19, 0.12, 0.14),
            Abdomen => Vec3::new(0.17, 0.14, 0.12),
            Thorax => Vec3::new(0.23, 0.23, 0.145),
            Neck => Vec3::new(0.08, 0.07, 0.08),
            Head => Vec3::new(0.15, 0.17, 0.14),
            LeftUpperArm | RightUpperArm => Vec3::new(0.09, 0.15, 0.09),
            LeftForearm | RightForearm => Vec3::new(0.075, 0.14, 0.075),
            LeftHand | RightHand => Vec3::new(0.075, 0.065, 0.09),
            LeftThigh | RightThigh => Vec3::new(0.095, 0.16, 0.105),
            LeftShin | RightShin => Vec3::new(0.08, 0.16, 0.085),
            LeftFoot | RightFoot => Vec3::new(0.09, 0.055, 0.15),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub enum PhysicalMode {
    #[default]
    Standing,
    Stumbling,
    Fallen,
    Recovering,
    Dead,
}

#[derive(Debug, Default, Clone, Component, Archive, Serialize, Deserialize)]
pub struct PoseState {
    pub velocity: [f32; 3],
    pub grounded: bool,
    pub crouch: f32,
    pub phase: f32,
    pub lean: [f32; 2],
    pub mode: PhysicalMode,
    pub mode_time: f32,
    pub impulse: [f32; 3],
    pub left_support: f32,
    pub right_support: f32,
    pub landing: f32,
    pub aim_pitch: f32,
    pub weapon: crate::WeaponKind,
    pub reload_fraction: f32,
    pub arm_weakness: [f32; 2],
    pub root_drop: f32,
}

#[derive(Debug, Clone, Copy, Archive, Serialize, Deserialize)]
pub struct SegmentPose {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}
impl Default for SegmentPose {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            rotation: Quat::IDENTITY.to_array(),
        }
    }
}
impl SegmentPose {
    pub fn transform(self) -> Transform {
        Transform::from_translation(self.position.into())
            .with_rotation(Quat::from_array(self.rotation))
    }
}

fn bone(a: Vec3, b: Vec3) -> SegmentPose {
    SegmentPose {
        position: ((a + b) * 0.5).into(),
        rotation: Quat::from_rotation_arc(Vec3::Y, (a - b).normalize_or_zero()).to_array(),
    }
}

/// Absolute actor-local segment centres. A bent knee changes geometry, never global scale.
pub fn evaluate_pose(
    state: &PoseState,
    left_leg: f32,
    right_leg: f32,
) -> [SegmentPose; SEGMENT_COUNT] {
    use Segment::*;
    let mut result = [SegmentPose::default(); SEGMENT_COUNT];
    let speed = Vec2::new(state.velocity[0], state.velocity[2]).length();
    let stride = (speed / 5.5).clamp(0.0, 1.0);
    let crouch = state.crouch.clamp(0.0, 1.0);
    let bob = (state.phase * 2.0).cos() * 0.022 * stride;
    // Actor root already lowers 0.3 m with the KCC capsule. Knees make up the remaining bend.
    let hip = Vec3::new(
        state.lean[0] * 0.07,
        -0.23 + bob - crouch * 0.04 - state.landing * 0.05,
        state.lean[1] * 0.06,
    );
    let chest = hip + Vec3::new(0.0, 0.43 - crouch * 0.14, -crouch * 0.12);
    let aim = Quat::from_rotation_x(state.aim_pitch);
    result[Pelvis.index()].position = hip.into();
    result[Abdomen.index()] = bone(hip + Vec3::Y * 0.24, hip + Vec3::Y * 0.02);
    result[Thorax.index()].position = chest.into();
    result[Thorax.index()].rotation = Quat::from_euler(
        EulerRot::XYZ,
        crouch * 0.2 + state.lean[1] * 0.12,
        0.0,
        -state.lean[0] * 0.12,
    )
    .to_array();
    result[Neck.index()].position = (chest + Vec3::Y * 0.24).into();
    result[Head.index()].position = (chest + Vec3::Y * 0.45).into();
    result[Head.index()].rotation = aim.to_array();
    for (side, upper, lower, hand) in [
        (-1.0, LeftUpperArm, LeftForearm, LeftHand),
        (1.0, RightUpperArm, RightForearm, RightHand),
    ] {
        let shoulder = chest + Vec3::new(side * 0.30, 0.10, 0.0);
        let reload = (state.reload_fraction * std::f32::consts::PI).sin();
        let pistol = state.weapon == crate::WeaponKind::Pistol;
        let hold = if side < 0.0 {
            Vec3::new(
                if pistol { 0.14 } else { 0.07 },
                0.0,
                if pistol {
                    -0.31
                } else if state.weapon == crate::WeaponKind::Smg {
                    -0.36
                } else {
                    -0.43
                },
            )
        } else {
            Vec3::new(if pistol { 0.18 } else { 0.24 }, 0.0, -0.32)
        };
        let hand_reload = if side < 0.0 {
            Vec3::new(reload * 0.10, -reload * 0.24, reload * 0.18)
        } else {
            Vec3::new(0.0, -reload * 0.04, reload * 0.06)
        };
        let weakness = state.arm_weakness[if side < 0.0 { 0 } else { 1 }];
        let desired = chest + Vec3::Y * (0.17 - weakness * 0.08) + aim * (hold + hand_reload);
        let axis = (desired - shoulder).normalize_or_zero();
        let distance = desired.distance(shoulder).clamp(0.10, 0.575);
        let grip = shoulder + axis * distance;
        let along = (0.30_f32.powi(2) - 0.28_f32.powi(2) + distance * distance) / (2.0 * distance);
        let bend = (0.30_f32.powi(2) - along * along).max(0.0).sqrt();
        let down = (-Vec3::Y - axis * (-Vec3::Y).dot(axis)).normalize_or_zero();
        let elbow = shoulder + axis * along + down * bend;
        result[upper.index()] = bone(shoulder, elbow);
        result[lower.index()] = bone(elbow, grip);
        result[hand.index()].position = grip.into();
    }
    for (side, thigh, shin, capacity) in [
        (-1.0, LeftThigh, LeftShin, left_leg),
        (1.0, RightThigh, RightShin, right_leg),
    ] {
        let phase = (state.phase
            + if side > 0.0 {
                std::f32::consts::PI
            } else {
                0.0
            })
            % std::f32::consts::TAU;
        let stride_length = 0.28 - crouch * 0.10;
        let travel = if phase < std::f32::consts::PI {
            1.0 - 2.0 * phase / std::f32::consts::PI
        } else {
            -1.0 + 2.0 * (phase - std::f32::consts::PI) / std::f32::consts::PI
        };
        let lift = if phase < std::f32::consts::PI {
            0.0
        } else {
            (phase - std::f32::consts::PI).sin()
        };
        let direction = Vec3::new(state.velocity[0], 0.0, state.velocity[2]).normalize_or_zero();
        let step =
            direction * travel * stride_length * (speed * 2.0).min(1.0) * (0.35 + capacity * 0.65);
        let pelvis = hip + Vec3::new(side * 0.12, -0.06, 0.0);
        let foot = Vec3::new(
            side * 0.13,
            -0.84
                + crouch * 0.30
                + lift * stride * 0.10
                + if state.grounded {
                    0.0
                } else {
                    0.10 * (state.velocity[1].abs() / 6.5).min(1.0)
                },
            0.0,
        ) + step;
        let leg_axis = (foot - pelvis).normalize_or_zero();
        let bend = (0.32_f32.powi(2) - pelvis.distance_squared(foot) * 0.25)
            .max(0.0)
            .sqrt();
        let forward = (-Vec3::Z - leg_axis * (-Vec3::Z).dot(leg_axis)).normalize_or_zero();
        let knee = (pelvis + foot) * 0.5 + forward * bend;
        result[thigh.index()] = bone(pelvis, knee);
        result[shin.index()] = bone(knee, foot);
        result[if side < 0.0 { LeftFoot } else { RightFoot }.index()].position =
            (foot + Vec3::new(0.0, -0.005, -0.06)).into();
    }
    if matches!(
        state.mode,
        PhysicalMode::Fallen | PhysicalMode::Dead | PhysicalMode::Recovering
    ) {
        let blend = if state.mode == PhysicalMode::Recovering {
            (1.0 - state.mode_time / 1.0).clamp(0.0, 1.0)
        } else {
            (state.mode_time / 0.35).clamp(0.0, 1.0)
        };
        let rot = Quat::from_rotation_x(-1.48 * blend);
        for p in &mut result {
            p.position = (rot * Vec3::from(p.position)
                + Vec3::new(0.0, (-0.58 + crouch * 0.3) * blend + state.root_drop, 0.0))
            .into();
            p.rotation = (rot * Quat::from_array(p.rotation)).to_array();
        }
    }
    result
}

/// Slab intersection in the oriented box's frame. Includes rays starting inside tissue.
pub fn ray_box(
    origin: Vec3,
    direction: Vec3,
    pose: SegmentPose,
    half: Vec3,
    max: f32,
) -> Option<(f32, f32)> {
    let inv = Quat::from_array(pose.rotation).inverse();
    let o = inv * (origin - Vec3::from(pose.position));
    let d = inv * direction;
    let mut near: f32 = 0.0;
    let mut far = max;
    for axis in 0..3 {
        if d[axis].abs() < 1e-7 {
            if o[axis].abs() > half[axis] {
                return None;
            }
        } else {
            let a = (-half[axis] - o[axis]) / d[axis];
            let b = (half[axis] - o[axis]) / d[axis];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return None;
            }
        }
    }
    (far >= 0.0 && near <= max).then_some((near, far))
}

pub fn ray_segments(
    origin: Vec3,
    direction: Vec3,
    max: f32,
    pose: &[SegmentPose; SEGMENT_COUNT],
    missing: u32,
) -> Vec<(Segment, f32, f32)> {
    let mut hits: Vec<_> = SEGMENTS
        .iter()
        .filter(|s| missing & (1 << s.index()) == 0)
        .filter_map(|s| {
            ray_box(origin, direction, pose[s.index()], s.half_size(), max).map(|(a, b)| (*s, a, b))
        })
        .collect();
    hits.sort_by(|a, b| a.1.total_cmp(&b.1));
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ray_between_legs_misses_and_lateral_hits_are_distinct() {
        let pose = evaluate_pose(&PoseState::default(), 1.0, 1.0);
        assert!(ray_segments(Vec3::new(0.0, -0.68, -2.0), Vec3::Z, 4.0, &pose, 0).is_empty());
        for (x, expected) in [(-0.13, Segment::LeftShin), (0.13, Segment::RightShin)] {
            assert_eq!(
                ray_segments(Vec3::new(x, -0.68, -2.0), Vec3::Z, 4.0, &pose, 0)[0].0,
                expected
            );
        }
    }
    #[test]
    fn crouch_bends_knees_without_scaling_and_absent_tissue_is_not_hit() {
        let standing = evaluate_pose(&PoseState::default(), 1.0, 1.0);
        let crouched = evaluate_pose(
            &PoseState {
                crouch: 1.0,
                ..default()
            },
            1.0,
            1.0,
        );
        assert_ne!(standing[11].rotation, crouched[11].rotation);
        assert!(Vec3::from(crouched[12].position).y > Vec3::from(standing[12].position).y);
        let missing = (1 << Segment::LeftThigh.index())
            | (1 << Segment::LeftShin.index())
            | (1 << Segment::LeftFoot.index());
        assert!(ray_segments(
            Vec3::new(-0.13, -0.68, -2.0),
            Vec3::Z,
            4.0,
            &standing,
            missing
        )
        .is_empty());
    }
    #[test]
    fn fallen_head_and_chest_follow_pose() {
        let pose = evaluate_pose(
            &PoseState {
                mode: PhysicalMode::Fallen,
                mode_time: 1.0,
                ..default()
            },
            1.0,
            1.0,
        );
        assert!(Vec3::from(pose[Segment::Head.index()].position).y < -0.4);
        assert_eq!(
            ray_segments(Vec3::new(0.0, -0.52, -2.0), Vec3::Z, 4.0, &pose, 0)[0].0,
            Segment::Head
        );
    }
    #[test]
    fn grounded_stance_foot_is_planted_during_strafe_and_jump_tucks_the_leg() {
        let mut state = PoseState {
            grounded: true,
            velocity: [3.25, 0.0, 0.0],
            phase: 0.5,
            ..default()
        };
        let before = evaluate_pose(&state, 1.0, 1.0);
        let dt = 0.01;
        state.phase += 3.25 * dt * std::f32::consts::PI / 0.56;
        let after = evaluate_pose(&state, 1.0, 1.0);
        assert!(
            (before[Segment::LeftFoot.index()].position[0]
                - (after[Segment::LeftFoot.index()].position[0] + 3.25 * dt))
                .abs()
                < 0.001
        );
        state.grounded = false;
        state.velocity = [0.0, 6.5, 0.0];
        let jump = evaluate_pose(&state, 1.0, 1.0);
        assert!(
            jump[Segment::LeftFoot.index()].position[1]
                > before[Segment::LeftFoot.index()].position[1] + 0.08
        );
    }
}
