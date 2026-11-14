use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use common::humanoid::*;
use common::{combat::*, raid::*, *};

use super::{Projectile, WorldState, can_stand_up, set_crouched_state};

pub(super) fn physx_tick(
    context: ReadRapierContext,
    raid: Res<RaidState>,
    time: Res<Time>,
    mut query: Query<(
        Entity,
        &ClientInput,
        &BodyHealth,
        &PoseState,
        &Participant,
        &mut MovementState,
        &mut Collider,
        &mut KinematicCharacterController,
        Option<&KinematicCharacterControllerOutput>,
        &mut Transform,
    )>,
) {
    let Ok(context) = context.single() else {
        return;
    };
    let delta = time.delta_secs();
    for (
        entity,
        input,
        body,
        pose,
        player,
        mut movement,
        mut collider,
        mut controller,
        output,
        mut transform,
    ) in &mut query
    {
        if raid.phase != RaidPhase::Active || player.status != RaidStatus::Active || !body.alive() {
            movement.velocity = Vec3::ZERO;
            controller.translation = None;
            continue;
        }
        if matches!(pose.mode, PhysicalMode::Fallen | PhysicalMode::Recovering) {
            movement.velocity.x *= 0.92;
            movement.velocity.z *= 0.92;
            movement.velocity.y -= PLAYER_GRAVITY * delta;
            controller.translation = Some(movement.velocity * delta);
            continue;
        }
        if input.crouch && !movement.crouched {
            set_crouched_state(&mut movement, &mut collider, &mut transform, true);
        } else if !input.crouch
            && movement.crouched
            && can_stand_up(entity, &context, transform.translation)
        {
            set_crouched_state(&mut movement, &mut collider, &mut transform, false);
        }
        let x = (input.right as i8 - input.left as i8) as f32;
        let z = (input.backward as i8 - input.forward as i8) as f32;
        let local_input = Vec3::new(x, 0.0, z).normalize_or_zero();
        let wish_dir = Quat::from_rotation_y(input.camera.yaw) * local_input;
        let horizontal = Vec3::new(movement.velocity.x, 0.0, movement.velocity.z);
        let speed = if movement.crouched {
            PLAYER_CROUCH_SPEED
        } else if input.run && body.anatomy.vigor > 10.0 && body.anatomy.speed() > 0.6 {
            PLAYER_RUN_SPEED
        } else {
            PLAYER_WALK_SPEED
        };
        let grounded = output.is_some_and(|o| o.grounded);
        movement.grounded = grounded;
        let accel = if grounded {
            if local_input == Vec3::ZERO {
                PLAYER_GROUND_DECELERATION
            } else {
                PLAYER_GROUND_ACCELERATION
            }
        } else if local_input == Vec3::ZERO {
            0.0
        } else {
            PLAYER_AIR_ACCELERATION
        };
        let control = if grounded { 1.0 } else { PLAYER_AIR_CONTROL };
        let horizontal = horizontal
            + (wish_dir * speed * body.speed_factor() - horizontal)
                * (accel * delta).min(1.0)
                * control;
        movement.velocity.x = horizontal.x;
        movement.velocity.z = horizontal.z;
        if pose.mode == PhysicalMode::Stumbling {
            let impulse = Vec3::from(pose.impulse);
            movement.velocity.x += impulse.x * delta * 3.0;
            movement.velocity.z += impulse.z * delta * 3.0;
        }
        if grounded {
            movement.velocity.y = movement.velocity.y.max(0.0);
            if input.jump
                && !movement.jump_queued
                && body.anatomy.speed() > 0.4
                && body.anatomy.vigor > 10.0
            {
                movement.velocity.y = PLAYER_JUMP_SPEED * body.anatomy.speed().sqrt();
                movement.grounded = false;
                movement.jump_queued = true;
            }
        } else {
            movement.velocity.y -= PLAYER_GRAVITY * delta;
        }
        if !input.jump {
            movement.jump_queued = false;
        }
        controller.translation = Some(movement.velocity * delta);
        transform.rotation = Quat::from_rotation_y(input.camera.yaw);
    }
}

pub(super) fn weapons_tick(
    mut commands: Commands,
    time: Res<Time>,
    raid: Res<RaidState>,
    context: ReadRapierContext,
    mut world_state: ResMut<WorldState>,
    mut query: Query<(
        Entity,
        &ClientInput,
        &Transform,
        &MovementState,
        &BodyHealth,
        &Client,
        &PoseState,
        &Participant,
        &mut Arsenal,
    )>,
) {
    let Ok(context) = context.single() else {
        return;
    };
    for (entity, input, transform, movement, body, client, pose, player, mut arsenal) in &mut query
    {
        if raid.phase != RaidPhase::Active || player.status != RaidStatus::Active || !body.alive() {
            arsenal.synchronize_sequences(input);
            arsenal.reload_remaining = 0.0;
            continue;
        }
        if matches!(pose.mode, PhysicalMode::Fallen | PhysicalMode::Recovering)
            || body.anatomy.capacity(Region::RightArm) <= 0.0
        {
            arsenal.synchronize_sequences(input);
            arsenal.reload_remaining = 0.0;
            continue;
        }
        let handling = 0.45 + body.anatomy.handling() * 0.55;
        let Some(weapon) = arsenal.step(input, time.delta_secs() * handling, time.elapsed_secs())
        else {
            continue;
        };
        let rotation = Quat::from(&input.camera);
        let direction = rotation * -Vec3::Z;
        let eye = transform.translation
            + Vec3::Y
                * (PLAYER_VIEW_HEIGHT
                    + if movement.crouched {
                        PLAYER_CROUCH_VIEW_OFFSET
                    } else {
                        0.0
                    });
        let muzzle = eye + rotation * Vec3::from(weapon.spec().barrel_offset);
        let barrel = muzzle - eye;
        // A barrel protruding through cover must not bypass that cover.
        let obstructed = context
            .cast_ray(
                eye,
                barrel.normalize(),
                barrel.length(),
                true,
                QueryFilter::new()
                    .groups(CollisionGroups::new(
                        Group::ALL,
                        Group::GROUP_1 | Group::GROUP_3,
                    ))
                    .exclude_collider(entity)
                    .exclude_sensors(),
            )
            .is_some();
        let id = world_state.next_projectile_id;
        world_state.next_projectile_id = id.wrapping_add(1);
        commands.spawn((
            Projectile {
                id,
                velocity: direction * weapon.spec().muzzle_speed,
                damage: arsenal.loadout.ammo.damage(weapon),
                penetration: arsenal.loadout.ammo.penetration(weapon),
                lifetime: PROJECTILE_LIFETIME,
                owner_entity: entity,
                penetrated: Vec::new(),
            },
            Transform::from_translation(if obstructed { eye } else { muzzle }),
        ));
        world_state.fired_projectiles.push(FiredProjectileData {
            id,
            weapon,
            shooter: client.id,
            time: time.elapsed_secs(),
            muzzle: muzzle.into(),
            direction: direction.into(),
        });
    }
}

pub(super) fn projectiles_tick(
    mut commands: Commands,
    mut world_state: ResMut<WorldState>,
    context: ReadRapierContext,
    time: Res<Time>,
    mut projectiles: Query<(Entity, &mut Projectile, &mut Transform)>,
    mut players: Query<
        (
            Entity,
            &mut BodyHealth,
            &mut ArmorState,
            &mut MovementState,
            &Transform,
            &Participant,
            Option<&mut PoseState>,
            Option<&Client>,
            Has<super::Bot>,
        ),
        Without<Projectile>,
    >,
    surfaces: Query<(&SurfaceKind, &Collider, &Transform), Without<Projectile>>,
    physics_pieces: Query<(), With<super::physical::PhysicsPiece>>,
) {
    let Ok(context) = context.single() else {
        return;
    };
    let delta = time.delta_secs();
    for (entity, mut projectile, mut transform) in &mut projectiles {
        projectile.velocity.y -= PROJECTILE_GRAVITY * delta;
        let displacement = projectile.velocity * delta;
        let mut remaining = displacement.length();
        let direction = displacement.normalize_or_zero();
        let mut start = transform.translation;
        let mut stopped = false;
        for _ in 0..8 {
            if remaining <= 0.0 {
                break;
            }
            let predicate = |hit_entity, _: &_| {
                !projectile.penetrated.contains(&hit_entity)
                    && players.get(hit_entity).is_err()
                    && physics_pieces.get(hit_entity).is_err()
            };
            let filter = QueryFilter::new()
                .exclude_collider(projectile.owner_entity)
                .exclude_sensors()
                .predicate(&predicate);
            let wall = context.cast_ray_and_get_normal(start, direction, remaining, true, filter);
            let max = wall.as_ref().map_or(remaining, |(_, h)| h.time_of_impact);
            let mut nearest = None;
            for (target, body, _, movement, t, p, pose, _, _) in &players {
                if target == projectile.owner_entity
                    || !body.alive()
                    || p.status != RaidStatus::Active
                {
                    continue;
                }
                let fallback = PoseState {
                    crouch: if movement.crouched { 1.0 } else { 0.0 },
                    ..default()
                };
                let pose = evaluate_pose(
                    pose.as_deref().unwrap_or(&fallback),
                    body.anatomy.capacity(Region::LeftLeg),
                    body.anatomy.capacity(Region::RightLeg),
                );
                let inv = t.rotation.inverse();
                let local = inv * (start - t.translation);
                if let Some((segment, distance, _)) =
                    ray_segments(local, inv * direction, max, &pose, body.anatomy.missing)
                        .first()
                        .copied()
                {
                    if nearest
                        .as_ref()
                        .is_none_or(|(_, _, d): &(Entity, Segment, f32)| distance < *d)
                    {
                        nearest = Some((target, segment, distance));
                    }
                }
            }
            if let Some((target, segment, _)) = nearest {
                let (_, mut body, mut armor, movement, t, _, mut pose, client, bot) =
                    players.get_mut(target).unwrap();
                let fallback = PoseState {
                    crouch: if movement.crouched { 1.0 } else { 0.0 },
                    ..default()
                };
                let rig = evaluate_pose(
                    pose.as_deref().unwrap_or(&fallback),
                    body.anatomy.capacity(Region::LeftLeg),
                    body.anatomy.capacity(Region::RightLeg),
                );
                let inv = t.rotation.inverse();
                let mask = body.hit_anatomy(
                    segment,
                    projectile.damage,
                    projectile.penetration,
                    &mut armor,
                    inv * (start - t.translation),
                    inv * direction,
                    max,
                    &rig,
                );
                if let Some(pose) = pose.as_deref_mut() {
                    pose.impulse =
                        (Vec3::from(pose.impulse) + direction * projectile.damage * 0.028).into();
                }
                if mask != 0 {
                    super::physical::spawn_fragments(
                        &mut commands,
                        &mut world_state,
                        mask,
                        &rig,
                        t,
                        movement.velocity + direction * 3.0,
                        client.map_or(0, |c| c.id),
                        bot,
                    );
                }
                stopped = true;
                break;
            }
            let Some((hit_entity, hit)) = wall else {
                break;
            };
            world_state.impact(hit.point, hit.normal);
            let Ok((surface, collider, surface_transform)) = surfaces.get(hit_entity) else {
                stopped = true;
                break;
            };
            if matches!(surface, SurfaceKind::Concrete) {
                stopped = true;
                break;
            }
            let reverse_origin = hit.point + direction * 100.0;
            let Some(exit) = collider.cast_ray_and_get_normal(
                surface_transform.translation,
                surface_transform.rotation,
                reverse_origin,
                -direction,
                100.0,
                true,
            ) else {
                stopped = true;
                break;
            };
            let thickness = hit.point.distance(exit.point);
            let Some(energy) = surface.penetrate(projectile.penetration, thickness) else {
                stopped = true;
                break;
            };
            projectile.damage *= energy / projectile.penetration;
            projectile.penetration = energy;
            projectile.penetrated.push(hit_entity);
            world_state.impact(exit.point, exit.normal);
            let traveled = hit.time_of_impact + 0.002;
            start = hit.point + direction * 0.002;
            remaining -= traveled;
            if projectile.penetrated.len() >= 8 {
                stopped = true;
                break;
            }
        }
        projectile.lifetime -= delta;
        if stopped || projectile.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        } else {
            transform.translation += displacement;
        }
    }
}

pub(super) fn sync_ground_state(
    mut query: Query<(
        &mut MovementState,
        &KinematicCharacterControllerOutput,
        &mut PoseState,
    )>,
) {
    for (mut movement, output, mut pose) in &mut query {
        if output.grounded && !movement.grounded && movement.velocity.y < -2.0 {
            pose.landing = (-movement.velocity.y / 10.0).min(1.0);
            if movement.velocity.y < -9.0 {
                pose.impulse = [3.0, 0.0, 0.0];
            }
        }
        movement.grounded = output.grounded;
        if output.grounded && movement.velocity.y < 0.0 {
            movement.velocity.y = 0.0;
        }
    }
}
