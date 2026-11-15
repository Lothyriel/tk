//! Surviving articulated falls. The same segment IDs own fall, get-up and a later death.
use super::{
    Bot, WorldState,
    physical::{self, PhysicsPiece},
    ragdoll,
};
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use common::{combat::*, humanoid::*, *};

struct Recovery {
    from: [Transform; SEGMENT_COUNT],
    root: Transform,
    target: Transform,
    drop: f32,
}

#[derive(Component)]
pub(super) struct Body {
    pub parts: [Option<Entity>; SEGMENT_COUNT],
    start: Transform,
    last_root: Transform,
    quiet: f32,
    floor: Option<f32>,
    recovery: Option<Recovery>,
}

impl Body {
    pub fn resume_dynamic(&mut self, commands: &mut Commands, motion: &ragdoll::Motion) {
        if self.recovery.take().is_some() {
            for (i, part) in self.parts.iter().enumerate() {
                if let Some(part) = part {
                    commands
                        .entity(*part)
                        .insert((
                            RigidBody::Dynamic,
                            Velocity {
                                linear: motion.linear[i],
                                angular: motion.angular[i],
                            },
                        ))
                        .remove::<ColliderDisabled>();
                }
            }
        }
    }
}

fn local_rig(root: &Transform, world: &[Transform; SEGMENT_COUNT]) -> [SegmentPose; SEGMENT_COUNT] {
    std::array::from_fn(|i| SegmentPose {
        position: (root.rotation.inverse() * (world[i].translation - root.translation)).into(),
        rotation: (root.rotation.inverse() * world[i].rotation).to_array(),
    })
}

pub(super) fn begin_falls(
    mut commands: Commands,
    mut state: ResMut<WorldState>,
    mut actors: Query<
        (
            Entity,
            &Client,
            &BodyHealth,
            &mut PoseState,
            &Transform,
            &ragdoll::Motion,
            &mut ragdoll::Impact,
            Has<Bot>,
        ),
        Without<Body>,
    >,
    pieces: Query<(), With<PhysicsPiece>>,
) {
    let mut available = physical::MAX_PIECES.saturating_sub(pieces.iter().count());
    for (entity, client, health, mut pose, root, motion, mut impact, bot) in &mut actors {
        if pose.mode != PhysicalMode::Fallen || !health.alive() {
            continue;
        }
        let count = SEGMENTS
            .iter()
            .filter(|s| health.anatomy.missing & (1 << s.index()) == 0)
            .count();
        if count > available {
            continue;
        } // Existing bounded-budget controller fallback.
        let world = if motion.valid {
            motion.world
        } else {
            let rig = evaluate_pose(
                &pose,
                health.anatomy.capacity(Region::LeftLeg),
                health.anatomy.capacity(Region::RightLeg),
            );
            std::array::from_fn(|i| root.mul_transform(rig[i].transform()))
        };
        let rig = local_rig(root, &world);
        if impact.impulse.length_squared() < 0.001 {
            impact.add(
                Vec3::from(pose.impulse) * 8.0,
                world[Segment::Thorax.index()].translation,
            );
        }
        let velocities = ragdoll::transfer(
            &rig,
            root,
            motion,
            &impact,
            health.anatomy.missing,
            root.rotation * Vec3::from(pose.velocity),
        );
        let parts = physical::corpse(
            &mut commands,
            &mut state,
            &rig,
            physical::PieceActor {
                root,
                owner: client.id,
                bot,
            },
            &velocities,
            health,
        );
        impact.impulse = Vec3::ZERO;
        pose.physical = Some(Box::new(PackedRig::pack(&rig)));
        commands.entity(entity).insert(Body {
            parts,
            start: *root,
            last_root: *root,
            quiet: 0.0,
            floor: None,
            recovery: None,
        });
        available -= count;
        info!(
            "LIVE RAGDOLL BEGIN owner={} segments={} inherited_pelvis={:?}",
            client.id, count, velocities[0].0
        );
    }
}

fn standing_space(
    entity: Entity,
    context: &RapierContext<'_>,
    centre: Vec3,
    crouched: bool,
) -> bool {
    if !super::can_occupy_character(entity, context, centre, crouched) {
        return false;
    }
    let mut clear = true;
    context.intersect_shape(
        centre,
        Quat::IDENTITY,
        &Collider::cuboid(0.48, if crouched { 0.52 } else { 0.84 }, 0.48),
        QueryFilter::new()
            .exclude_collider(entity)
            .exclude_sensors()
            .groups(CollisionGroups::new(
                Group::ALL,
                Group::GROUP_1 | Group::GROUP_3,
            )),
        |_, _| {
            clear = false;
            false
        },
    );
    clear
}

pub(super) fn advance(
    mut commands: Commands,
    context: ReadRapierContext,
    mut actors: Query<
        (
            Entity,
            &Client,
            &ClientInput,
            &BodyHealth,
            &mut PoseState,
            &mut MovementState,
            &mut Collider,
            &mut Transform,
            &mut Body,
            &mut ragdoll::Motion,
        ),
        Without<PhysicsPiece>,
    >,
    mut pieces: Query<(&PhysicsPiece, &mut Transform, &mut Velocity), With<PhysicsPiece>>,
) {
    let Ok(context) = context.single() else {
        return;
    };
    for (
        entity,
        client,
        input,
        health,
        mut pose,
        mut movement,
        mut collider,
        mut root,
        mut body,
        mut motion,
    ) in &mut actors
    {
        if !health.alive() {
            continue;
        }
        if root.translation.distance(body.last_root.translation) > 2.0 {
            // An authoritative reposition invalidates the old articulated island,
            // just as it invalidates KCC support/motion history on deployment.
            for part in body.parts.iter().flatten() {
                commands.entity(*part).despawn();
            }
            commands.entity(entity).remove::<Body>();
            pose.physical = None;
            pose.mode = PhysicalMode::Standing;
            pose.mode_time = 0.0;
            pose.root_drop = 0.0;
            movement.velocity = Vec3::ZERO;
            movement.grounded = false;
            motion.valid = false;
            *collider = Collider::capsule_y(
                if movement.crouched {
                    PLAYER_CROUCH_COLLIDER_HALF_HEIGHT
                } else {
                    PLAYER_COLLIDER_HALF_HEIGHT
                },
                PLAYER_COLLIDER_RADIUS,
            );
            continue;
        }
        if pose.mode == PhysicalMode::Fallen
            && pose.mode_time > 1.8
            && body.quiet > 0.1
            && body.floor.is_some()
            && health.anatomy.leg_support().0 + health.anatomy.leg_support().1 > 0.7
            && health.anatomy.muscle[Region::Abdomen as usize] > 20.0
            && health.anatomy.blood > BLOOD_COLLAPSE
        {
            let floor = body.floor.unwrap();
            let height = if movement.crouched {
                PLAYER_CROUCH_COLLIDER_HALF_HEIGHT
            } else {
                PLAYER_COLLIDER_HALF_HEIGHT
            } + PLAYER_COLLIDER_RADIUS
                + 0.018;
            let mut target = None;
            for offset in [
                Vec3::ZERO,
                Vec3::X * 0.30,
                -Vec3::X * 0.30,
                Vec3::Z * 0.30,
                -Vec3::Z * 0.30,
            ] {
                let centre =
                    Vec3::new(root.translation.x, floor + height, root.translation.z) + offset;
                if standing_space(entity, &context, centre, movement.crouched) {
                    target = Some(centre);
                    break;
                }
            }
            if let Some(centre) = target {
                let mut from = [Transform::default(); SEGMENT_COUNT];
                for (i, part) in body.parts.iter().enumerate() {
                    if let Some(part) = part
                        && let Ok((_, t, _)) = pieces.get(*part)
                    {
                        from[i] = *t;
                    }
                }
                body.recovery = Some(Recovery {
                    from,
                    root: *root,
                    target: Transform::from_translation(centre)
                        .with_rotation(Quat::from_rotation_y(input.camera.yaw)),
                    drop: pose.root_drop,
                });
                pose.mode = PhysicalMode::Recovering;
                pose.mode_time = 0.0;
                for part in body.parts.iter().flatten() {
                    commands
                        .entity(*part)
                        .insert((RigidBody::KinematicPositionBased, ColliderDisabled));
                }
                *collider = Collider::capsule_y(
                    if movement.crouched {
                        PLAYER_CROUCH_COLLIDER_HALF_HEIGHT
                    } else {
                        PLAYER_COLLIDER_HALF_HEIGHT
                    },
                    PLAYER_COLLIDER_RADIUS,
                );
                info!(
                    "LIVE RAGDOLL RECOVERY owner={} preserved_segment_ids=true floor={floor:.3}",
                    client.id
                );
            }
        }
        if pose.mode == PhysicalMode::Recovering {
            let parts = body.parts;
            let Some(recovery) = &mut body.recovery else {
                continue;
            };
            let progress = common::weapon_animation::ease(pose.mode_time);
            recovery.target.rotation = Quat::from_rotation_y(input.camera.yaw);
            let mut target_pose = pose.clone();
            target_pose.physical = None;
            target_pose.mode = PhysicalMode::Standing;
            target_pose.root_drop = 0.0;
            target_pose.velocity = [0.0; 3];
            target_pose.foot_weight = [0.0; 2];
            let rig = evaluate_pose(
                &target_pose,
                health.anatomy.capacity(Region::LeftLeg),
                health.anatomy.capacity(Region::RightLeg),
            );
            root.translation = recovery
                .root
                .translation
                .lerp(recovery.target.translation, progress);
            root.rotation = recovery
                .root
                .rotation
                .slerp(recovery.target.rotation, progress);
            pose.root_drop = recovery.drop * (1.0 - progress);
            for (i, part) in parts.iter().enumerate() {
                if let Some(part) = part
                    && let Ok((_, mut t, mut velocity)) = pieces.get_mut(*part)
                {
                    let target = recovery.target.mul_transform(rig[i].transform());
                    t.translation = recovery.from[i]
                        .translation
                        .lerp(target.translation, progress);
                    t.rotation = recovery.from[i].rotation.slerp(target.rotation, progress);
                    *velocity = Velocity::zero();
                }
            }
            if pose.mode_time >= 1.0 {
                *root = recovery.target;
                pose.mode = PhysicalMode::Standing;
                pose.mode_time = 0.0;
                pose.root_drop = 0.0;
                pose.physical = None;
                movement.velocity = Vec3::ZERO;
                movement.grounded = true;
                for part in parts.iter().flatten() {
                    commands.entity(*part).despawn();
                }
                commands.entity(entity).remove::<Body>();
                info!(
                    "LIVE RAGDOLL RECOVERED owner={} position={:?}",
                    client.id, root.translation
                );
            }
        }
    }
}

pub(super) fn readback(
    time: Res<Time>,
    context: ReadRapierContext,
    mut actors: Query<
        (
            Entity,
            &BodyHealth,
            &mut PoseState,
            &mut MovementState,
            &mut Transform,
            &mut Body,
            &mut ragdoll::Impact,
        ),
        Without<PhysicsPiece>,
    >,
    mut pieces: Query<(&mut PhysicsPiece, &Transform, &mut Velocity), With<PhysicsPiece>>,
) {
    let Ok(context) = context.single() else {
        return;
    };
    for (entity, health, mut pose, mut movement, mut root, mut body, mut impact) in &mut actors {
        let mut world = [Transform::default(); SEGMENT_COUNT];
        let mut max_speed = 0.0_f32;
        let mut max_spin = 0.0_f32;
        for (i, part) in body.parts.iter().enumerate() {
            if let Some(part) = part
                && let Ok((mut piece, t, velocity)) = pieces.get_mut(*part)
            {
                world[i] = *t;
                max_speed = max_speed.max(velocity.linear.length());
                max_spin = max_spin.max(velocity.angular.length());
                if health.alive() {
                    piece.remaining = physical::PIECE_LIFETIME;
                }
            }
        }
        let pelvis = world[Segment::Pelvis.index()].translation;
        if pose.mode == PhysicalMode::Fallen {
            root.translation.x = pelvis.x;
            root.translation.z = pelvis.z;
            root.translation.y = (pelvis.y + 0.12).clamp(0.40, body.start.translation.y.max(0.40));
            body.floor = context
                .cast_ray_and_get_normal(
                    pelvis + Vec3::Y * 0.1,
                    -Vec3::Y,
                    0.65,
                    true,
                    QueryFilter::new()
                        .exclude_collider(entity)
                        .exclude_sensors()
                        .groups(CollisionGroups::new(Group::ALL, Group::GROUP_1)),
                )
                .filter(|(_, h)| h.normal.y > 0.7)
                .map(|(_, h)| h.point.y);
            movement.grounded = body.floor.is_some();
            if body.floor.is_some() && max_speed < 1.2 && max_spin < 4.0 {
                body.quiet += time.delta_secs();
            } else {
                body.quiet = 0.0;
            }
        }
        let rig = local_rig(&root, &world);
        body.last_root = *root;
        pose.physical = Some(Box::new(PackedRig::pack(&rig)));
        if impact.impulse.length_squared() > 0.001
            && matches!(pose.mode, PhysicalMode::Fallen | PhysicalMode::Dead)
        {
            let changes = ragdoll::transfer(
                &rig,
                &root,
                &ragdoll::Motion::default(),
                &impact,
                health.anatomy.missing,
                Vec3::ZERO,
            );
            for (i, part) in body.parts.iter().enumerate() {
                if let Some(part) = part
                    && let Ok((_, _, mut velocity)) = pieces.get_mut(*part)
                {
                    velocity.linear += changes[i].0;
                    velocity.angular = (velocity.angular + changes[i].1).clamp_length_max(18.0);
                }
            }
            impact.impulse = Vec3::ZERO;
        }
    }
}
