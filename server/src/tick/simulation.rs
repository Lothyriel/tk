use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use common::humanoid::*;
use common::{combat::*, raid::*, *};

use super::{Projectile, WorldState, can_stand_up, set_crouched_state};

pub(super) fn impact_mark_size_mm(weapon: WeaponKind) -> u8 {
    match weapon {
        WeaponKind::Pistol => 25,
        WeaponKind::Smg => 35,
        WeaponKind::Rifle => 50,
    }
}

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
        Has<super::live_body::Body>,
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
        articulated,
    ) in &mut query
    {
        if raid.phase != RaidPhase::Active || player.status != RaidStatus::Active || !body.alive() {
            movement.velocity = Vec3::ZERO;
            controller.translation = None;
            continue;
        }
        if matches!(pose.mode, PhysicalMode::Fallen | PhysicalMode::Recovering) {
            if articulated {
                movement.velocity = Vec3::ZERO;
                controller.translation = None;
                continue;
            }
            movement.velocity.x *= (-delta * 3.0).exp();
            movement.velocity.z *= (-delta * 3.0).exp();
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
            + transform.rotation * common::humanoid::aim_lean_offset(input.camera.roll)
            + Vec3::Y
                * (PLAYER_VIEW_HEIGHT
                    + if movement.crouched {
                        PLAYER_CROUCH_VIEW_OFFSET
                    } else {
                        0.0
                    });
        let muzzle = eye + rotation * common::weapon_animation::barrel_offset(weapon, pose.ads);
        // Resolve actual eye-to-muzzle contacts in projectiles_tick. A broad
        // movement capsule must neither redirect a clear offset ray to the eye
        // nor let a muzzle spawn beyond real anatomy/concrete at point blank.
        let id = world_state.next_projectile_id;
        world_state.next_projectile_id = id.wrapping_add(1);
        commands.spawn((
            Projectile {
                id,
                weapon,
                velocity: direction * weapon.spec().muzzle_speed,
                damage: arsenal.loadout.ammo.damage(weapon),
                penetration: arsenal.loadout.ammo.penetration(weapon),
                lifetime: PROJECTILE_LIFETIME,
                owner_entity: entity,
                penetrated: Vec::new(),
            },
            super::MuzzleClearance(muzzle),
            Transform::from_translation(eye),
        ));
        if std::env::var_os("TK_SHOT_DIAGNOSTICS").is_some() {
            info!(
                "SHOT ORIGIN id={id} shooter={} eye={eye:?} muzzle={muzzle:?} direction={direction:?} ads={} clearance=posed-anatomy-and-world",
                client.id, pose.ads
            );
        }
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

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct ProjectileEnvironment<'w, 's> {
    surfaces: Query<
        'w,
        's,
        (&'static SurfaceKind, &'static Collider, &'static Transform),
        Without<Projectile>,
    >,
    physics_pieces: Query<'w, 's, (Entity, &'static mut super::physical::PhysicsPiece)>,
    voxels: Query<'w, 's, &'static mut super::BarrierVoxel>,
}

pub(super) fn projectiles_tick(
    mut commands: Commands,
    mut world_state: ResMut<WorldState>,
    context: ReadRapierContext,
    time: Res<Time>,
    mut projectiles: Query<(
        Entity,
        &mut Projectile,
        &mut Transform,
        Option<&super::MuzzleClearance>,
    )>,
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
            Option<&mut super::ragdoll::Impact>,
            Option<&mut super::live_body::Body>,
        ),
        Without<Projectile>,
    >,
    environment: ProjectileEnvironment,
) {
    let ProjectileEnvironment {
        surfaces,
        mut physics_pieces,
        mut voxels,
    } = environment;
    let Ok(context) = context.single() else {
        return;
    };
    let delta = time.delta_secs();
    for (entity, mut projectile, mut transform, clearance) in &mut projectiles {
        projectile.velocity.y -= PROJECTILE_GRAVITY * delta;
        let displacement = projectile.velocity * delta;
        let muzzle = clearance.map_or(transform.translation, |pending| pending.0);
        if clearance.is_some() {
            commands.entity(entity).remove::<super::MuzzleClearance>();
        }
        let mut stopped = false;
        // The short clearance segment and the ordinary muzzle ray share exactly
        // the same ordered anatomy/cover/penetration resolver. Surface IDs stay
        // in `penetrated` across both paths, so wood is never charged twice.
        for (path, origin, travel) in [
            (
                "clearance",
                transform.translation,
                muzzle - transform.translation,
            ),
            ("flight", muzzle, displacement),
        ] {
            let mut remaining = travel.length();
            let direction = travel.normalize_or_zero();
            let mut start = origin;
            for _ in 0..8 {
                if remaining <= 0.0 {
                    break;
                }
                let predicate = |hit_entity, _: &_| {
                    !projectile.penetrated.contains(&hit_entity)
                        && !voxels.get(hit_entity).is_ok_and(|cell| cell.health <= 0.0)
                        && players.get(hit_entity).is_err()
                        && physics_pieces.get(hit_entity).is_err()
                };
                let filter = QueryFilter::new()
                    .exclude_collider(projectile.owner_entity)
                    .exclude_sensors()
                    .predicate(&predicate);
                let wall =
                    context.cast_ray_and_get_normal(start, direction, remaining, true, filter);
                let max = wall.as_ref().map_or(remaining, |(_, h)| h.time_of_impact);
                let mut nearest = None;
                for (target, body, _, movement, t, p, pose, _, _, _, _) in &players {
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
                        pose.unwrap_or(&fallback),
                        body.anatomy.capacity(Region::LeftLeg),
                        body.anatomy.capacity(Region::RightLeg),
                    );
                    let inv = t.rotation.inverse();
                    let local = inv * (start - t.translation);
                    if let Some((segment, distance, _)) =
                        ray_segments(local, inv * direction, max, &pose, body.anatomy.missing)
                            .first()
                            .copied()
                        && nearest
                            .as_ref()
                            .is_none_or(|(_, _, d): &(Entity, Segment, f32)| distance < *d)
                    {
                        nearest = Some((target, segment, distance));
                    }
                }
                if let Some((target, segment, distance)) = nearest {
                    if path == "clearance" && std::env::var_os("TK_SHOT_DIAGNOSTICS").is_some() {
                        info!(
                            "SHOT CLEARANCE HIT id={} actor={target:?} segment={segment:?} origin={origin:?} start={start:?} direction={direction:?} distance={distance} point={:?}",
                            projectile.id,
                            start + direction * distance
                        );
                    }
                    let (
                        _,
                        mut body,
                        mut armor,
                        movement,
                        t,
                        _,
                        mut pose,
                        client,
                        bot,
                        mut impact,
                        mut articulated,
                    ) = players.get_mut(target).unwrap();
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
                        AnatomyHit {
                            segment,
                            damage: projectile.damage,
                            penetration: projectile.penetration,
                            origin: inv * (start - t.translation),
                            direction: inv * direction,
                            range: max,
                        },
                        &mut armor,
                        &rig,
                    );
                    if let Some(pose) = pose.as_deref_mut() {
                        pose.impulse = (Vec3::from(pose.impulse)
                            + direction * projectile.damage * 0.028)
                            .into();
                    }
                    if let Some(impact) = impact.as_deref_mut() {
                        impact.add(
                            direction * projectile.damage * 0.14,
                            start + direction * distance,
                        );
                    }
                    if mask != 0 {
                        let mut reused = 0;
                        if let Some(articulated) = articulated.as_deref_mut() {
                            for (piece_entity, mut piece) in &mut physics_pieces {
                                if piece.owner == client.map_or(0, |c| c.id)
                                    && !piece.detached
                                    && mask & (1 << piece.segment.index()) != 0
                                {
                                    piece.detached = true;
                                    reused |= 1 << piece.segment.index();
                                    articulated.parts[piece.segment.index()] = None;
                                    commands.entity(piece_entity).remove::<ImpulseJoint>();
                                }
                            }
                        }
                        super::physical::spawn_fragments(
                            &mut commands,
                            &mut world_state,
                            mask & !reused,
                            &rig,
                            super::physical::PieceActor {
                                root: t,
                                owner: client.map_or(0, |c| c.id),
                                bot,
                            },
                            movement.velocity + direction * 3.0,
                        );
                    }
                    stopped = true;
                    break;
                }
                let Some((hit_entity, hit)) = wall else {
                    break;
                };
                if path == "clearance" && std::env::var_os("TK_SHOT_DIAGNOSTICS").is_some() {
                    info!(
                        "SHOT CLEARANCE HIT id={} surface={hit_entity:?} origin={origin:?} start={start:?} direction={direction:?} distance={} point={:?}",
                        projectile.id, hit.time_of_impact, hit.point
                    );
                }
                world_state.impact(
                    hit.point,
                    hit.normal,
                    impact_mark_size_mm(projectile.weapon),
                );
                let Ok((surface, collider, surface_transform)) = surfaces.get(hit_entity) else {
                    stopped = true;
                    break;
                };
                if let Ok(mut voxel) = voxels.get_mut(hit_entity) {
                    voxel.health = (voxel.health - projectile.damage).max(0.0);
                    if voxel.health == 0.0 {
                        world_state.destroyed_cells.push(voxel.id);
                        commands.entity(hit_entity).insert(ColliderDisabled);
                    }
                }
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
                world_state.impact(
                    exit.point,
                    exit.normal,
                    impact_mark_size_mm(projectile.weapon),
                );
                let traveled = hit.time_of_impact + 0.002;
                start = hit.point + direction * 0.002;
                remaining -= traveled;
                if projectile.penetrated.len() >= 8 {
                    stopped = true;
                    break;
                }
            }
            if stopped {
                break;
            }
        }
        projectile.lifetime -= delta;
        if stopped || projectile.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        } else {
            transform.translation = muzzle + displacement;
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
        if pose.physical.is_some() {
            continue;
        }
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
