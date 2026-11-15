//! Pose-to-physics transfer in world space, including articulated and impact momentum.
use bevy::prelude::*;
use common::{combat::*, humanoid::*};

#[derive(Component, Default)]
pub(super) struct Motion {
    pub root: Transform,
    pub world: [Transform; SEGMENT_COUNT],
    pub linear: [Vec3; SEGMENT_COUNT],
    pub angular: [Vec3; SEGMENT_COUNT],
    pub valid: bool,
    pub handling_error: Option<handling::HandlingError>,
}

impl Motion {
    /// The exact accepted anatomical pose used for damage and later handoff.
    pub fn local_rig(&self, root: &Transform) -> Option<[SegmentPose; SEGMENT_COUNT]> {
        self.valid.then(|| {
            std::array::from_fn(|i| SegmentPose {
                position: (root.rotation.inverse()
                    * (self.world[i].translation - root.translation))
                    .to_array(),
                rotation: (root.rotation.inverse() * self.world[i].rotation).to_array(),
            })
        })
    }
}

pub(super) fn evaluate(
    pose: &PoseState,
    body: &BodyHealth,
    arsenal: Option<&Arsenal>,
    armor: Option<&ArmorState>,
) -> Result<[SegmentPose; SEGMENT_COUNT], handling::HandlingError> {
    let legs = [
        body.anatomy.capacity(Region::LeftLeg),
        body.anatomy.capacity(Region::RightLeg),
    ];
    match arsenal {
        Some(arsenal) => common::humanoid::configured::evaluate(
            pose,
            legs,
            arsenal.current().presentation_spec(),
            &arsenal.current().mechanics().snapshot(),
            None,
            Some(armor.map_or(0.0, common::humanoid::configured::shoulder_cover)),
        )
        .map(|pose| pose.segments),
        None => Ok(evaluate_pose(pose, legs[0], legs[1])),
    }
}

#[derive(Component, Default)]
pub(super) struct Impact {
    pub impulse: Vec3,
    pub point: Vec3,
    age: f32,
}
impl Impact {
    pub fn add(&mut self, impulse: Vec3, point: Vec3) {
        if self.age > 0.25 {
            self.impulse = Vec3::ZERO;
        }
        self.impulse += impulse;
        self.point = point;
        self.age = 0.0;
    }
}

pub(super) fn mass(segment: Segment) -> f32 {
    use Segment::*;
    match segment {
        Pelvis => 12.0,
        Abdomen => 6.0,
        Thorax => 18.0,
        Neck => 1.0,
        Head => 4.5,
        LeftUpperArm | RightUpperArm => 2.2,
        LeftForearm | RightForearm => 1.4,
        LeftHand | RightHand => 0.5,
        LeftThigh | RightThigh => 6.0,
        LeftShin | RightShin => 3.5,
        LeftFoot | RightFoot => 1.1,
    }
}

pub(super) fn capture(
    time: Res<Time>,
    mut actors: Query<(
        &Transform,
        &PoseState,
        &BodyHealth,
        Option<&Arsenal>,
        Option<&ArmorState>,
        &mut Motion,
        &mut Impact,
    )>,
) {
    let dt = time.delta_secs().max(0.001);
    for (root, pose, body, arsenal, armor, mut motion, mut impact) in &mut actors {
        if !body.alive() {
            continue;
        }
        impact.age += dt;
        if impact.age > 0.25 {
            impact.impulse = Vec3::ZERO;
        }
        let rig = match evaluate(pose, body, arsenal, armor) {
            Ok(rig) => {
                if pose.mode != PhysicalMode::Recovering {
                    motion.handling_error = None;
                }
                rig
            }
            Err(error) => {
                if motion.handling_error != Some(error) {
                    warn!("Authoritative configured anatomy rejected: {error:?}");
                }
                motion.handling_error = Some(error);
                // Preserve the accepted actor-local anatomy while allowing the
                // authoritative root to move. Never leave tissue behind in the
                // world as the movement collider walks away.
                let Some(previous) = motion.local_rig(&motion.root) else {
                    continue;
                };
                previous
            }
        };
        let reset = !motion.valid
            || motion.world[Segment::Pelvis.index()]
                .translation
                .distance(root.transform_point(rig[Segment::Pelvis.index()].position.into()))
                > 2.0;
        for segment in SEGMENTS {
            let i = segment.index();
            let world = root.mul_transform(rig[i].transform());
            motion.linear[i] = if reset {
                root.rotation * Vec3::from(pose.velocity)
            } else {
                ((world.translation - motion.world[i].translation) / dt).clamp_length_max(15.0)
            };
            motion.angular[i] = if reset {
                Vec3::ZERO
            } else {
                let mut delta = world.rotation * motion.world[i].rotation.inverse();
                if delta.w < 0.0 {
                    delta = -delta;
                }
                (delta.to_scaled_axis() / dt).clamp_length_max(14.0)
            };
            motion.world[i] = world;
        }
        motion.valid = true;
        motion.root = *root;
    }
}

/// Distribute one real impulse by proximity; linear momentum sums to the supplied
/// impulse, and the contact lever arm supplies torque in each segment's inertia frame.
pub(super) fn transfer(
    rig: &[SegmentPose; SEGMENT_COUNT],
    root: &Transform,
    motion: &Motion,
    impact: &Impact,
    missing: u32,
    fallback: Vec3,
) -> [(Vec3, Vec3); SEGMENT_COUNT] {
    let weights: [f32; SEGMENT_COUNT] = std::array::from_fn(|i| {
        if missing & (1 << i) != 0 {
            0.0
        } else {
            let point = root.transform_point(rig[i].position.into());
            mass(SEGMENTS[i]) / (0.04 + point.distance_squared(impact.point)).powi(2)
        }
    });
    let total = weights.iter().sum::<f32>().max(0.001);
    std::array::from_fn(|i| {
        let segment = SEGMENTS[i];
        let t = root.mul_transform(rig[i].transform());
        let impulse = impact.impulse * (weights[i] / total);
        let half = segment.half_size() * 0.88;
        let inertia = mass(segment) / 3.0
            * Vec3::new(
                half.y * half.y + half.z * half.z,
                half.x * half.x + half.z * half.z,
                half.x * half.x + half.y * half.y,
            );
        let lever = (impact.point - t.translation).clamp_length_max(0.35);
        let torque = t.rotation.inverse() * lever.cross(impulse);
        let spin = t.rotation * (torque / inertia.max(Vec3::splat(0.0001)));
        (
            (if motion.valid {
                motion.linear[i]
            } else {
                fallback
            }) + impulse / mass(segment),
            ((if motion.valid {
                motion.angular[i]
            } else {
                Vec3::ZERO
            }) + spin)
                .clamp_length_max(18.0),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn localized_force_preserves_total_impulse_and_changes_spin_with_contact_side() {
        let rig = evaluate_pose(&PoseState::default(), 1.0, 1.0);
        let root = Transform::from_xyz(0.0, 1.0, 0.0);
        let centre = root.transform_point(rig[Segment::Thorax.index()].position.into());
        let impulse = Vec3::new(0.0, 0.0, -4.0);
        let left = Impact {
            impulse,
            point: centre - Vec3::X * 0.2,
            ..default()
        };
        let right = Impact {
            impulse,
            point: centre + Vec3::X * 0.2,
            ..default()
        };
        let a = transfer(&rig, &root, &Motion::default(), &left, 0, Vec3::ZERO);
        let b = transfer(&rig, &root, &Motion::default(), &right, 0, Vec3::ZERO);
        let sum = |v: &[(Vec3, Vec3); SEGMENT_COUNT]| {
            SEGMENTS
                .iter()
                .map(|s| v[s.index()].0 * mass(*s))
                .sum::<Vec3>()
        };
        assert!(sum(&a).distance(impulse) < 0.001);
        assert!(sum(&b).distance(impulse) < 0.001);
        assert!(a[Segment::Thorax.index()].1.y * b[Segment::Thorax.index()].1.y < 0.0);
        assert!(a[Segment::Thorax.index()].0.length() > a[Segment::LeftFoot.index()].0.length());
        let running = transfer(
            &rig,
            &root,
            &Motion::default(),
            &Impact::default(),
            0,
            -Vec3::Z * 5.5,
        );
        assert!(
            running
                .iter()
                .all(|(linear, angular)| linear.distance(-Vec3::Z * 5.5) < 0.001
                    && angular.length() == 0.0)
        );
    }
}
