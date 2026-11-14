use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
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
        } else if input.run {
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
        if grounded {
            movement.velocity.y = movement.velocity.y.max(0.0);
            if input.jump && !movement.jump_queued {
                movement.velocity.y = PLAYER_JUMP_SPEED;
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
        &Participant,
        &mut Arsenal,
    )>,
) {
    let Ok(context) = context.single() else {
        return;
    };
    for (entity, input, transform, movement, body, player, mut arsenal) in &mut query {
        if raid.phase != RaidPhase::Active || player.status != RaidStatus::Active || !body.alive() {
            arsenal.synchronize_sequences(input);
            continue;
        }
        let Some(weapon) = arsenal.step(input, time.delta_secs(), time.elapsed_secs()) else {
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
        world_state
            .fired_projectiles
            .push(FiredProjectileData { id, weapon });
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
            &mut BodyHealth,
            &mut ArmorState,
            &mut MovementState,
            &Transform,
            &Participant,
        ),
        Without<Projectile>,
    >,
    surfaces: Query<(&SurfaceKind, &Collider, &Transform), Without<Projectile>>,
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
                    && players.get(hit_entity).map_or(true, |(body, _, _, _, p)| {
                        body.alive() && p.status == RaidStatus::Active
                    })
            };
            let filter = QueryFilter::new()
                .exclude_collider(projectile.owner_entity)
                .exclude_sensors()
                .predicate(&predicate);
            let Some((hit_entity, hit)) =
                context.cast_ray_and_get_normal(start, direction, remaining, true, filter)
            else {
                break;
            };
            if let Ok((mut body, mut armor, mut movement, player_transform, _)) =
                players.get_mut(hit_entity)
            {
                let mut point = player_transform.rotation.inverse()
                    * (hit.point - player_transform.translation);
                if movement.crouched {
                    point.y /= PLAYER_CROUCH_SCALE;
                }
                let part = BodyPart::at_local_point(point);
                body.hit(part, projectile.damage, projectile.penetration, &mut armor);
                if !body.alive() {
                    movement.velocity = Vec3::ZERO;
                }
                stopped = true;
                break;
            }
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
    mut query: Query<(&mut MovementState, &KinematicCharacterControllerOutput)>,
) {
    for (mut movement, output) in &mut query {
        movement.grounded = output.grounded;
        if output.grounded && movement.velocity.y < 0.0 {
            movement.velocity.y = 0.0;
        }
    }
}
