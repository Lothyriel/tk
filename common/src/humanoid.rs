//! Original TK rig. Metres, actor-local Y up / forward -Z. Shared by damage and art.
use bevy::prelude::*;
use rkyv::{Archive, Deserialize, Serialize};

pub mod handling;

pub const SEGMENT_COUNT: usize = 17;

/// Lateral chest/head/camera displacement for each radian of aim roll.
pub fn aim_lean_offset(aim_roll: f32) -> Vec3 {
    Vec3::new(-aim_roll * 0.7, 0.0, 0.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Archive, Serialize, Deserialize)]
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
    pub aim_roll: f32,
    pub ads: f32,
    pub weapon: crate::WeaponKind,
    pub reload_fraction: f32,
    pub reload_empty: bool,
    pub arm_weakness: [f32; 2],
    pub root_drop: f32,
    pub acceleration: [f32; 2],
    pub turn_rate: f32,
    pub feet: [[f32; 3]; 2],
    pub foot_weight: [f32; 2],
    pub foot_normal: [[f32; 3]; 2],
    pub foot_yaw: [f32; 2],
    pub fall_direction: [f32; 2],
    pub fall_phase: f32,
    pub fall_velocity: [f32; 3],
    pub physical: Option<Box<PackedRig>>,
}

/// Bounded actual articulated pose, in actor-local millimetres. This supplements
/// float physics snapshots for authoritative hit volumes during live fall/get-up.
#[derive(Debug, Clone, Copy, Archive, Serialize, Deserialize)]
pub struct PackedSegmentPose {
    position: [i16; 3],
    rotation: [i8; 4],
}

#[derive(Debug, Clone, Archive, Serialize, Deserialize)]
pub struct PackedRig(pub [PackedSegmentPose; SEGMENT_COUNT]);

impl PackedRig {
    pub fn pack(rig: &[SegmentPose; SEGMENT_COUNT]) -> Self {
        Self(std::array::from_fn(|i| PackedSegmentPose {
            position: rig[i]
                .position
                .map(|x| (x * 1000.0).round().clamp(-32767.0, 32767.0) as i16),
            rotation: Quat::from_array(rig[i].rotation)
                .normalize()
                .to_array()
                .map(|x| (x * 127.0).round().clamp(-127.0, 127.0) as i8),
        }))
    }
    pub fn unpack(&self) -> [SegmentPose; SEGMENT_COUNT] {
        std::array::from_fn(|i| {
            let q = Quat::from_array(self.0[i].rotation.map(|x| x as f32 / 127.0));
            SegmentPose {
                position: self.0[i].position.map(|x| x as f32 / 1000.0),
                rotation: if q.length_squared() > 0.01 {
                    q.normalize()
                } else {
                    Quat::IDENTITY
                }
                .to_array(),
            }
        })
    }
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

/// One complete left/right stride, rather than the old ~10 steps/s running cadence.
pub fn stride_phase_rate(speed: f32, crouch: f32) -> f32 {
    let cycle_metres = 2.1 + (speed / 5.5).clamp(0.0, 1.0) * 0.7 - crouch.clamp(0.0, 1.0) * 0.65;
    speed.max(0.0) * std::f32::consts::TAU / cycle_metres
}

/// Absolute actor-local segment centres. A bent knee changes geometry, never global scale.
pub fn evaluate_pose(
    state: &PoseState,
    left_leg: f32,
    right_leg: f32,
) -> [SegmentPose; SEGMENT_COUNT] {
    evaluate_pose_with_action(state, left_leg, right_leg, None)
}

/// Presentation may interpolate the rigid weapon action as one unit so its contacts
/// remain exact through snapshot gaps/interruption. Authority uses the normal wrapper.
pub fn evaluate_pose_with_action(
    state: &PoseState,
    left_leg: f32,
    right_leg: f32,
    action: Option<crate::weapon_animation::WeaponAction>,
) -> [SegmentPose; SEGMENT_COUNT] {
    use Segment::*;
    if let Some(physical) = &state.physical {
        return physical.unpack();
    }
    let mut result = [SegmentPose::default(); SEGMENT_COUNT];
    let down_blend = if state.mode == PhysicalMode::Recovering {
        1.0 - crate::weapon_animation::ease(state.mode_time)
    } else if matches!(state.mode, PhysicalMode::Fallen | PhysicalMode::Dead) {
        crate::weapon_animation::ease(state.mode_time / 0.35)
    } else {
        0.0
    };
    let falling = matches!(
        state.mode,
        PhysicalMode::Fallen | PhysicalMode::Dead | PhysicalMode::Recovering
    );
    let velocity = if falling {
        Vec3::from(state.fall_velocity) * (1.0 - down_blend)
    } else {
        state.velocity.into()
    };
    let phase = if falling {
        state.fall_phase
    } else {
        state.phase
    };
    let speed = Vec2::new(velocity.x, velocity.z).length();
    let stride = (speed / 5.5).clamp(0.0, 1.0);
    let crouch = state.crouch.clamp(0.0, 1.0);
    let bob = (phase * 2.0).cos() * 0.022 * stride;
    // Actor root already lowers 0.3 m with the KCC capsule. Knees make up the remaining bend.
    let hip = Vec3::new(
        state.lean[0] * 0.07,
        -0.23 + bob - crouch * 0.04 - state.landing * 0.05,
        state.lean[1] * 0.06,
    );
    let lean_offset = aim_lean_offset(state.aim_roll);
    let chest =
        hip + Vec3::new(
            state.lean[0] * 0.025,
            0.43 - crouch * 0.14,
            -crouch * 0.12 - state.lean[1] * 0.05,
        ) + lean_offset;
    let aim = Quat::from_rotation_x(state.aim_pitch);
    result[Pelvis.index()].position = hip.into();
    result[Pelvis.index()].rotation = Quat::from_rotation_y(-state.turn_rate * 0.025).to_array();
    result[Abdomen.index()] = bone(hip + Vec3::Y * 0.24 + lean_offset, hip + Vec3::Y * 0.02);
    result[Thorax.index()].position = chest.into();
    result[Thorax.index()].rotation = Quat::from_euler(
        EulerRot::XYZ,
        crouch * 0.2 - state.lean[1] * 0.16,
        state.turn_rate * 0.02,
        -state.lean[0] * 0.12 + state.aim_roll * 0.5,
    )
    .to_array();
    result[Neck.index()].position = (chest + Vec3::Y * 0.24).into();
    result[Head.index()].position = (chest + Vec3::Y * 0.45).into();
    result[Head.index()].rotation = (aim * Quat::from_rotation_z(state.aim_roll)).to_array();
    let action = action.unwrap_or_else(|| {
        crate::weapon_animation::reload_action(
            state.weapon,
            state.reload_fraction,
            state.reload_empty,
        )
    });
    let weapon = weapon_pose(state, chest, action);
    let weapon_rig = crate::weapon_animation::WeaponRig::for_weapon(state.weapon);
    for (side, upper, lower, hand) in [
        (-1.0, LeftUpperArm, LeftForearm, LeftHand),
        (1.0, RightUpperArm, RightForearm, RightHand),
    ] {
        let shoulder = chest + Vec3::new(side * 0.30, 0.10, 0.0);
        let desired = weapon.transform_point(if side < 0.0 {
            action.support.translation
        } else {
            weapon_rig.trigger
        });
        let axis = (desired - shoulder).normalize_or_zero();
        let distance = desired.distance(shoulder).clamp(0.10, 0.575);
        let grip = shoulder + axis * distance;
        // Keep elbows outside the thorax while the hands meet at the weapon.
        // A straight-down pole folds the trigger upper arm inside the chest.
        let pole = Vec3::new(side * 0.8, -1.0, 0.0).normalize();
        let elbow = handling::elbow_for_reachable(shoulder, grip, pole);
        result[upper.index()] = bone(shoulder, elbow);
        result[lower.index()] = bone(elbow, grip);
        result[hand.index()].position = grip.into();
        result[hand.index()].rotation = (weapon.rotation
            * if side < 0.0 {
                action.support.rotation
            } else {
                Quat::IDENTITY
            })
        .to_array();
    }
    for (side, thigh, shin, capacity) in [
        (-1.0, LeftThigh, LeftShin, left_leg),
        (1.0, RightThigh, RightShin, right_leg),
    ] {
        let phase = (phase
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
        let direction = Vec3::new(velocity.x, 0.0, velocity.z).normalize_or_zero();
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
                    0.10 * (velocity.y.abs() / 6.5).min(1.0)
                },
            0.0,
        ) + step;
        let foot_index = if side < 0.0 { 0 } else { 1 };
        let weight = state.foot_weight[foot_index].clamp(0.0, 1.0);
        // Server-owned contacts remain planted in world space through braking/turns.
        let foot = (foot + Vec3::new(0.0, -0.005, -0.06))
            .lerp(state.feet[foot_index].into(), weight)
            - Vec3::new(0.0, -0.005, -0.06);
        let leg_axis = (foot - pelvis).normalize_or_zero();
        let bend = (0.32_f32.powi(2) - pelvis.distance_squared(foot) * 0.25)
            .max(0.0)
            .sqrt();
        let forward = (-Vec3::Z - leg_axis * (-Vec3::Z).dot(leg_axis)).normalize_or_zero();
        let knee = (pelvis + foot) * 0.5 + forward * bend;
        result[thigh.index()] = bone(pelvis, knee);
        result[shin.index()] = bone(knee, foot);
        let foot_segment = if side < 0.0 { LeftFoot } else { RightFoot };
        result[foot_segment.index()].position = (foot + Vec3::new(0.0, -0.005, -0.06)).into();
        let normal = Vec3::from(state.foot_normal[foot_index]).normalize_or(Vec3::Y);
        result[foot_segment.index()].rotation = Quat::IDENTITY
            .slerp(
                Quat::from_rotation_arc(Vec3::Y, normal)
                    * Quat::from_rotation_y(state.foot_yaw[foot_index]),
                weight,
            )
            .to_array();
    }
    if matches!(
        state.mode,
        PhysicalMode::Fallen | PhysicalMode::Dead | PhysicalMode::Recovering
    ) {
        let blend = down_blend;
        let direction =
            Vec3::new(state.fall_direction[0], 0.0, state.fall_direction[1]).normalize_or(-Vec3::Z);
        let rot = Quat::from_axis_angle(Vec3::Y.cross(direction), 1.48 * blend);
        for p in &mut result {
            p.position = (rot * Vec3::from(p.position)
                + Vec3::new(0.0, (-0.58 + crouch * 0.3) * blend + state.root_drop, 0.0))
            .into();
            p.rotation = (rot * Quat::from_array(p.rotation)).to_array();
        }
        // A living lateral fall braces the neck instead of burying the head under
        // the upper shoulder. Forward/backward prone poses keep their own contact.
        let brace = direction.x.abs() * blend;
        result[Head.index()].position[1] += brace * 0.16;
        result[Head.index()].position[2] += brace * 0.16;
        result[Neck.index()].position[1] += brace * 0.08;
        result[Neck.index()].position[2] += brace * 0.08;
        // Release the support hold and brace low while down. Keeping both arms in
        // the aiming pose leaves the upper arm draped across the face on a side fall.
        for (side, upper, lower, hand) in [
            (-1.0, LeftUpperArm, LeftForearm, LeftHand),
            (1.0, RightUpperArm, RightForearm, RightHand),
        ] {
            let upper_pose = result[upper.index()].transform();
            let shoulder = upper_pose.transform_point(Vec3::Y * 0.15);
            let current = Vec3::from(result[hand.index()].position);
            let low = Vec3::new(
                current.x,
                -0.12 + state.root_drop - (0.60 - crouch * 0.3),
                current.z - 0.06,
            );
            let desired = current.lerp(low, blend);
            let axis = (desired - shoulder).normalize_or_zero();
            let distance = desired.distance(shoulder).clamp(0.10, 0.575);
            let grip = shoulder + axis * distance;
            let pole = Vec3::new(side * 0.2, -1.0, -0.25).normalize();
            let elbow = handling::elbow_for_reachable(shoulder, grip, pole);
            result[upper.index()] = bone(shoulder, elbow);
            result[lower.index()] = bone(elbow, grip);
            result[hand.index()].position = grip.into();
        }
    }
    result
}

/// Solve the weapon and both contacts together before arm IK. The same anchors drive
/// animated parts, remote hands and anatomical boxes; no render-only stretched arms.
fn weapon_pose(
    state: &PoseState,
    chest: Vec3,
    action: crate::weapon_animation::WeaponAction,
) -> Transform {
    use crate::weapon_animation::*;
    let rig = WeaponRig::for_weapon(state.weapon);
    let aim = Quat::from_rotation_x(state.aim_pitch);
    let mut root = Transform::from_translation(
        chest
            + Vec3::Y * (0.13 + state.ads * 0.07)
            + aim
                * (Vec3::new(0.10, 0.0, -0.18)
                    .lerp(Vec3::new(0.035, 0.0, -0.20), state.ads.clamp(0.0, 1.0))
                    + action.root.translation),
    )
    .with_rotation(aim * action.root.rotation);
    root.translation -= root.rotation * rig.trigger;
    // Project into the intersection of the two arm-reach balls. During a reload
    // the support target is on the magazine or release, never on a canned arc.
    for _ in 0..8 {
        for (side, contact) in [(-1.0, action.support.translation), (1.0, rig.trigger)] {
            let weakness = state.arm_weakness[if side < 0.0 { 0 } else { 1 }];
            let shoulder = chest + Vec3::new(side * 0.30, 0.10 - weakness * 0.01, 0.0);
            let delta = root.transform_point(contact) - shoulder;
            if delta.length() > 0.56 {
                root.translation -= delta.normalize() * (delta.length() - 0.56);
            }
        }
    }
    root
}

pub fn posed_weapon(state: &PoseState, rig: &[SegmentPose; SEGMENT_COUNT]) -> Transform {
    let hand = rig[Segment::RightHand.index()].transform();
    Transform::from_translation(
        hand.translation
            - hand.rotation * crate::weapon_animation::WeaponRig::for_weapon(state.weapon).trigger,
    )
    .with_rotation(hand.rotation)
}

pub mod configured;

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
    fn aim_roll_tilts_the_replicated_torso_and_head_in_both_directions() {
        let upright = evaluate_pose(&PoseState::default(), 1.0, 1.0);
        for roll in [-0.3, 0.3] {
            let state = PoseState {
                aim_roll: roll,
                ..default()
            };
            let tilted = evaluate_pose(&state, 1.0, 1.0);
            let expected_shift = aim_lean_offset(roll).x;
            for segment in [Segment::Thorax, Segment::Head] {
                let shift =
                    tilted[segment.index()].position[0] - upright[segment.index()].position[0];
                assert!(
                    (shift - expected_shift).abs() < 0.001,
                    "{segment:?}: {shift}"
                );
            }
            let tilted_weapon = posed_weapon(&state, &tilted);
            let upright_weapon = posed_weapon(&PoseState::default(), &upright);
            assert!(
                (tilted_weapon.translation.x - upright_weapon.translation.x - expected_shift).abs()
                    < 0.01,
                "weapon shift: {}",
                tilted_weapon.translation.x - upright_weapon.translation.x
            );
            for segment in [Segment::Thorax, Segment::Head] {
                let angle = Quat::from_array(upright[segment.index()].rotation)
                    .angle_between(Quat::from_array(tilted[segment.index()].rotation));
                assert!(angle > roll.abs() * 0.45, "{segment:?}: {angle}");
                let tilted_roll = Quat::from_array(tilted[segment.index()].rotation)
                    .to_euler(EulerRot::XYZ)
                    .2;
                assert_eq!(tilted_roll.signum(), roll.signum(), "{segment:?}");
            }
        }
    }

    #[test]
    fn packed_physical_pose_keeps_contacts_within_a_millimetre_and_two_hundredths_radian() {
        assert_eq!(std::mem::size_of::<PackedSegmentPose>(), 10);
        for i in 0..100 {
            let original = evaluate_pose(
                &PoseState {
                    grounded: true,
                    velocity: [1.0, 0.0, -5.0],
                    phase: i as f32 * 0.1,
                    aim_pitch: i as f32 * 0.02 - 1.0,
                    ..default()
                },
                1.0,
                0.7,
            );
            let state = PoseState {
                mode: PhysicalMode::Fallen,
                physical: Some(Box::new(PackedRig::pack(&original))),
                ..default()
            };
            let actual = evaluate_pose(&state, 0.0, 0.0);
            for (a, b) in original.iter().zip(actual.iter()) {
                assert!(Vec3::from(a.position).distance(b.position.into()) < 0.001);
                assert!(
                    Quat::from_array(a.rotation).angle_between(Quat::from_array(b.rotation)) < 0.02
                );
            }
        }
    }
    #[test]
    fn planted_feet_remain_at_contacts_through_turns_steps_and_landing() {
        for crouch in [0.0, 1.0] {
            let mut state = PoseState {
                grounded: true,
                crouch,
                velocity: [1.0, 0.0, -3.0],
                foot_weight: [1.0; 2],
                feet: [
                    [-0.16, -0.84 + crouch * 0.30, -0.18],
                    [0.13, -0.66 + crouch * 0.30, 0.08],
                ],
                foot_normal: [[0.0, 1.0, 0.0]; 2],
                foot_yaw: [0.3, -0.2],
                ..default()
            };
            for i in 0..100 {
                state.phase = i as f32 * 0.1;
                state.landing = i as f32 / 100.0;
                state.turn_rate = (i as f32 * 0.1).sin() * 4.0;
                let pose = evaluate_pose(&state, 0.6, 1.0);
                for (index, segment) in [Segment::LeftFoot, Segment::RightFoot]
                    .into_iter()
                    .enumerate()
                {
                    assert!(
                        Vec3::from(pose[segment.index()].position)
                            .distance(state.feet[index].into())
                            < 0.001
                    );
                }
                for bone in pose {
                    assert!(bone.transform().translation.is_finite());
                    assert!(Quat::from_array(bone.rotation).is_normalized());
                }
            }
        }
    }
    #[test]
    fn remote_weapon_and_hands_share_contacts_through_all_reload_stances() {
        use crate::weapon_animation::*;
        for weapon in [
            crate::WeaponKind::Pistol,
            crate::WeaponKind::Smg,
            crate::WeaponKind::Rifle,
        ] {
            for empty in [false, true] {
                for pitch in [-1.3, 0.0, 1.3] {
                    for i in 0..=100 {
                        let state = PoseState {
                            weapon,
                            reload_empty: empty,
                            reload_fraction: i as f32 / 100.0,
                            aim_pitch: pitch,
                            crouch: if i % 2 == 0 { 1.0 } else { 0.0 },
                            ..default()
                        };
                        let pose = evaluate_pose(&state, 1.0, 1.0);
                        let gun = posed_weapon(&state, &pose);
                        let action = reload_action(weapon, state.reload_fraction, empty);
                        let rig = WeaponRig::for_weapon(weapon);
                        assert!(
                            gun.transform_point(rig.trigger)
                                .distance(pose[Segment::RightHand.index()].position.into())
                                < 0.001
                        );
                        assert!(
                            gun.transform_point(action.support.translation)
                                .distance(pose[Segment::LeftHand.index()].position.into())
                                < 0.002,
                            "{weapon:?} pitch={pitch} t={} left={:?}",
                            state.reload_fraction,
                            pose[Segment::LeftHand.index()]
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn entering_ads_keeps_both_remote_hands_attached_to_the_shared_weapon() {
        use crate::{weapon_animation::*, WeaponKind};
        for weapon in [WeaponKind::Pistol, WeaponKind::Smg, WeaponKind::Rifle] {
            for pitch in [-1.0, 0.0, 1.0] {
                for ads in [0.0, 0.25, 0.5, 0.75, 1.0] {
                    let state = PoseState {
                        weapon,
                        aim_pitch: pitch,
                        ads,
                        ..default()
                    };
                    let pose = evaluate_pose(&state, 1.0, 1.0);
                    let gun = posed_weapon(&state, &pose);
                    let rig = WeaponRig::for_weapon(weapon);
                    assert!(
                        gun.transform_point(rig.trigger)
                            .distance(pose[Segment::RightHand.index()].position.into())
                            < 0.001
                    );
                    assert!(
                        gun.transform_point(rig.support)
                            .distance(pose[Segment::LeftHand.index()].position.into())
                            < 0.002,
                        "{weapon:?} ads={ads} pitch={pitch}"
                    );
                }
            }
        }
    }
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
