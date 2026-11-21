//! Authoritative world-space foot contacts and filtered inertial weight transfer.
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use common::{combat::*, humanoid::*, raid::*};

#[derive(Clone, Copy, Default)]
struct Plant {
    position: Vec3,
    normal: Vec3,
    yaw: f32,
    planted: bool,
    replanting: bool,
    released_until_swing: bool,
}

#[derive(Default, Component)]
pub(super) struct Gait {
    feet: [Plant; 2],
    velocity: Vec3,
    position: Vec3,
    yaw: f32,
    raid: u32,
}

pub(super) fn advance(
    time: Res<Time>,
    raid: Res<RaidState>,
    context: ReadRapierContext,
    mut actors: Query<(Entity, &Transform, &BodyHealth, &mut PoseState, &mut Gait)>,
) {
    let Ok(context) = context.single() else {
        return;
    };
    let dt = time.delta_secs().max(0.001);
    for (entity, root, body, mut pose, mut gait) in &mut actors {
        if raid.phase != RaidPhase::Active || !body.alive() {
            continue;
        }
        let yaw = root.rotation.to_euler(EulerRot::YXZ).0;
        let velocity = root.rotation * Vec3::from(pose.velocity);
        if gait.raid != raid.raid_id || root.translation.distance(gait.position) > 2.0 {
            *gait = Gait {
                velocity,
                position: root.translation,
                yaw,
                raid: raid.raid_id,
                ..default()
            };
            pose.foot_weight = [0.0; 2];
        }
        let accel = (root.rotation.inverse() * (velocity - gait.velocity) / dt)
            .clamp(Vec3::splat(-12.0), Vec3::splat(12.0));
        let alpha = 1.0 - (-dt * 12.0).exp();
        pose.acceleration[0] += (accel.x - pose.acceleration[0]) * alpha;
        pose.acceleration[1] += (accel.z - pose.acceleration[1]) * alpha;
        let turn = yaw - gait.yaw;
        let turn = (turn.sin().atan2(turn.cos()) / dt).clamp(-6.0, 6.0);
        pose.turn_rate += (turn - pose.turn_rate) * alpha;
        gait.velocity = velocity;
        gait.position = root.translation;
        gait.yaw = yaw;
        if !matches!(pose.mode, PhysicalMode::Standing | PhysicalMode::Stumbling) || !pose.grounded
        {
            pose.foot_weight = [0.0; 2];
            gait.feet = [Plant::default(); 2];
            continue;
        }
        let mut free = pose.clone();
        free.foot_weight = [0.0; 2];
        let rig = evaluate_pose(
            &free,
            body.anatomy.capacity(Region::LeftLeg),
            body.anatomy.capacity(Region::RightLeg),
        );
        let speed = Vec2::new(pose.velocity[0], pose.velocity[2]).length();
        // A planted foot is a contact, not a permanent world-space tether.
        // Release one foot at a time on a large stationary pivot; otherwise a
        // half-turn crosses the knees and can occlude the opposite shin forever.
        let pivot_foot = (0..2).find(|i| gait.feet[*i].replanting).or_else(|| {
            (0..2).find(|i| {
                let delta = yaw - gait.feet[*i].yaw;
                gait.feet[*i].planted && delta.sin().atan2(delta.cos()).abs() > 0.70
            })
        });
        for (i, segment) in [Segment::LeftFoot, Segment::RightFoot]
            .into_iter()
            .enumerate()
        {
            let phase = (pose.phase + i as f32 * std::f32::consts::PI) % std::f32::consts::TAU;
            let stance = (phase < std::f32::consts::PI || speed < 0.12)
                && body.anatomy.missing & (1 << segment.index()) == 0;
            let proposed = root.transform_point(rig[segment.index()].position.into());
            let foot = &mut gait.feet[i];
            if !stance || speed < 0.12 {
                foot.released_until_swing = false;
            }
            if pivot_foot == Some(i) {
                foot.planted = false;
                foot.replanting = pose.foot_weight[i] > 0.01;
            }
            if stance
                && !foot.planted
                && !foot.replanting
                && !foot.released_until_swing
                && let Some((_, hit)) = context.cast_ray_and_get_normal(
                    proposed + Vec3::Y * 0.35,
                    -Vec3::Y,
                    0.75,
                    true,
                    QueryFilter::new()
                        .exclude_collider(entity)
                        .exclude_sensors()
                        .groups(CollisionGroups::new(Group::ALL, Group::GROUP_1)),
                )
            {
                foot.position = hit.point + hit.normal * 0.055;
                foot.normal = hit.normal;
                foot.yaw = yaw;
                foot.planted = hit.normal.y > 0.65;
            }
            if foot.planted && stance && speed >= 0.12 && proposed.distance(foot.position) > 0.8 {
                // Do not teleport a loaded anchor to a new contact in the same
                // stance. Replication may skip the intervening release tick.
                foot.released_until_swing = true;
            }
            if !stance || foot.released_until_swing {
                foot.planted = false;
            }
            let target = if foot.planted { 1.0 } else { 0.0 };
            let rate = if foot.planted { 60.0 } else { 35.0 };
            pose.foot_weight[i] += (target - pose.foot_weight[i]) * (1.0 - (-dt * rate).exp());
            pose.feet[i] = (root.rotation.inverse() * (foot.position - root.translation)).into();
            pose.foot_normal[i] = (root.rotation.inverse() * foot.normal).into();
            let delta = foot.yaw - yaw;
            pose.foot_yaw[i] = delta.sin().atan2(delta.cos());
        }
    }
}
