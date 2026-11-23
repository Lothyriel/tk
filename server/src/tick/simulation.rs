use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use common::humanoid::*;
use common::{combat::*, raid::*, *};

use super::{Bot, LastInput, Projectile, WorldState, can_stand_up, set_crouched_state};
use common::stance::{PRONE_CRAWL_SPEED, PRONE_TRANSITION_DELAY, Stance};

pub(super) fn impact_mark_size_mm(weapon: WeaponId) -> u8 {
    weapon.spec().impact_size_mm
}

pub(super) fn advance_recoil(time: Res<Time>, mut actors: Query<&mut Arsenal>) {
    for mut arsenal in &mut actors {
        let rate = arsenal.current().spec().recoil.return_rate;
        arsenal.recoil.advance(time.delta_secs(), rate);
    }
}

pub(super) fn physx_tick(
    context: ReadRapierContext,
    raid: Res<RaidState>,
    time: Res<Time>,
    mut query: Query<(
        Entity,
        &ClientInput,
        &LastInput,
        &Arsenal,
        &BodyHealth,
        &mut PoseState,
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
        last,
        arsenal,
        body,
        mut pose,
        player,
        mut movement,
        mut collider,
        mut controller,
        output,
        mut transform,
        articulated,
    ) in &mut query
    {
        let now = time.elapsed_secs();
        let distance = input.prone_sequence.wrapping_sub(movement.prone_sequence);
        let toggle = distance != 0 && distance < (1 << 31);
        if toggle {
            // Every edge is consumed, including those blocked by headroom,
            // throttle, injury, landing or inactive/stale input.
            movement.prone_sequence = input.prone_sequence;
        }
        controller.autostep = (!movement.prone).then(super::character_autostep);
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
        let grounded = output.is_some_and(|o| o.grounded);
        if toggle
            && now - last.received_at <= 0.5
            && now >= movement.prone_transition_time
            && grounded
            && pose.mode == PhysicalMode::Standing
        {
            let upright = Stance::from_crouched(input.crouch);
            let (previous, next) = if movement.prone {
                (Stance::Prone, upright)
            } else {
                (Stance::from_crouched(movement.crouched), Stance::Prone)
            };
            let center = next.centre_from(previous, transform.translation);
            if super::can_occupy_stance(entity, &context, center, transform.rotation, next) {
                transform.translation = center;
                *collider = next.collider();
                movement.prone = next == Stance::Prone;
                movement.crouched = next == Stance::Crouched;
                movement.prone_transition_time = now + PRONE_TRANSITION_DELAY;
                movement.velocity.x = 0.0;
                movement.velocity.z = 0.0;
                controller.autostep = (!movement.prone).then(super::character_autostep);
                pose.prone = movement.prone;
                pose.crouch = f32::from(movement.crouched);
            }
        }
        if !movement.prone && input.crouch && !movement.crouched {
            set_crouched_state(&mut movement, &mut collider, &mut transform, true);
        } else if !movement.prone
            && !input.crouch
            && movement.crouched
            && can_stand_up(entity, &context, transform.translation)
        {
            set_crouched_state(&mut movement, &mut collider, &mut transform, false);
        }
        let x = (input.right as i8 - input.left as i8) as f32;
        let z = (input.backward as i8 - input.forward as i8) as f32;
        let local_input = Vec3::new(x, 0.0, z).normalize_or_zero();
        let yaw = if movement.prone {
            let old_yaw = transform.rotation.to_euler(EulerRot::YXZ).0;
            let delta_yaw = input.camera.yaw - old_yaw;
            let step = delta_yaw
                .sin()
                .atan2(delta_yaw.cos())
                .clamp(-5.0 * delta, 5.0 * delta);
            let target = old_yaw + step;
            if step.abs() > 1e-5
                && super::can_occupy_stance(
                    entity,
                    &context,
                    transform.translation,
                    Quat::from_rotation_y(target),
                    Stance::Prone,
                )
            {
                target
            } else {
                old_yaw
            }
        } else {
            input.camera.yaw
        };
        let wish_dir = Quat::from_rotation_y(yaw) * local_input;
        let horizontal = Vec3::new(movement.velocity.x, 0.0, movement.velocity.z);
        let speed = if movement.prone {
            PRONE_CRAWL_SPEED
        } else if movement.crouched {
            PLAYER_CROUCH_SPEED
        } else if input.run && body.anatomy.vigor > 10.0 && body.anatomy.speed() > 0.6 {
            PLAYER_RUN_SPEED
        } else {
            PLAYER_WALK_SPEED
        };
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
            if !movement.prone
                && input.jump
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
        } else if movement.prone {
            movement.jump_queued = true;
        }
        controller.translation = Some(movement.velocity * delta);
        transform.rotation = Quat::from_rotation_y(if movement.prone {
            yaw
        } else {
            arsenal.recoil.camera(input.camera).yaw
        });
    }
}

pub(super) fn neutral_eye(transform: &Transform, movement: &MovementState) -> Vec3 {
    if movement.prone {
        transform.transform_point(Stance::Prone.eye_offset())
    } else {
        transform.translation + Vec3::Y * Stance::from_crouched(movement.crouched).eye_above_root()
    }
}

pub(super) fn resolve_eye_lean(
    context: ReadRapierContext,
    raid: Res<RaidState>,
    mut actors: Query<(
        Entity,
        &ClientInput,
        &Transform,
        &MovementState,
        &Participant,
        &mut PoseState,
    )>,
) {
    let Ok(context) = context.single() else {
        return;
    };
    if raid.phase != RaidPhase::Active {
        return;
    }
    // Reserve room around the eye for the lens near plane and cosmetic bob.
    let clearance = Collider::ball(0.18);
    for (entity, input, root, movement, participant, mut pose) in &mut actors {
        if participant.status != RaidStatus::Active
            || !matches!(pose.mode, PhysicalMode::Standing | PhysicalMode::Stumbling)
        {
            continue;
        }
        let requested = if movement.prone {
            0.0
        } else {
            input.camera.roll
        };
        let fraction = if requested == 0.0 {
            1.0
        } else {
            let travel = root.rotation * common::humanoid::aim_lean_offset(requested);
            context
                .cast_shape(
                    neutral_eye(root, movement),
                    Quat::IDENTITY,
                    travel,
                    &clearance,
                    ShapeCastOptions::with_max_time_of_impact(1.0),
                    QueryFilter::new()
                        .groups(CollisionGroups::new(Group::ALL, Group::GROUP_1))
                        .exclude_collider(entity)
                        .exclude_sensors(),
                )
                .map_or(1.0, |(_, hit)| hit.time_of_impact.clamp(0.0, 1.0))
        };
        pose.aim_roll = requested * fraction;
    }
}

// Arena gameplay reference, not an adjustment to the rendered camera.
const M4A1_GAME_ZERO: f32 = 20.0;

fn shot_direction(
    spec: &common::catalog::ResolvedWeapon,
    rotation: Quat,
    ads: f32,
    dt: f32,
) -> Vec3 {
    let forward = rotation * Vec3::NEG_Z;
    let weight = common::weapon_animation::ease(ads);
    if weight == 0.0 || spec.definition != WeaponId::M4A1 {
        return forward;
    }
    let outlet = common::weapon_animation::resolved_muzzle_offset(spec, 1.0);
    let to_target = rotation * (Vec3::NEG_Z * M4A1_GAME_ZERO - outlet);
    let mut offset = to_target;
    for _ in 0..3 {
        let t = offset.length() / spec.muzzle_speed;
        // Flight updates velocity before moving. Interpolate the gravity
        // displacement on the same piecewise-linear path as the ray casts.
        let ticks = t / dt;
        let whole = ticks.floor();
        let drop = PROJECTILE_GRAVITY * dt * dt * (whole + 1.0) * (whole * 0.5 + ticks.fract());
        offset = to_target + Vec3::Y * drop;
    }
    forward.lerp(offset.normalize(), weight).normalize()
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
        &LastInput,
        Has<Bot>,
    )>,
) {
    for (entity, input, transform, movement, body, client, pose, player, mut arsenal, last, bot) in
        &mut query
    {
        if raid.phase != RaidPhase::Active
            || player.status != RaidStatus::Active
            || !body.alive()
            || (!bot && time.elapsed_secs() - last.received_at > 0.5)
        {
            arsenal.synchronize_sequences(input);
            arsenal.cancel_actions();
            continue;
        }
        if matches!(pose.mode, PhysicalMode::Fallen | PhysicalMode::Recovering)
            || body.anatomy.capacity(Region::RightArm) <= 0.0
        {
            arsenal.synchronize_sequences(input);
            arsenal.cancel_actions();
            continue;
        }
        let handling = 0.45 + body.anatomy.handling() * 0.55;
        if world_state.next_projectile_id == u64::MAX {
            error!("Projectile identity exhausted; refusing fire before consuming ammunition");
            continue;
        }
        let Some(weapon) = arsenal.step_with_fixture(
            input,
            time.delta_secs() * handling,
            time.elapsed_secs(),
            input.run && !movement.prone,
            world_state.action_fault_fixture && !bot,
        ) else {
            continue;
        };
        let camera = arsenal.recoil.camera(CameraInput {
            yaw: if movement.prone {
                transform.rotation.to_euler(EulerRot::YXZ).0
            } else {
                input.camera.yaw
            },
            roll: pose.aim_roll,
            ..input.camera
        });
        let rotation = Quat::from(&camera);
        let spec = arsenal.current().spec();
        let direction = shot_direction(spec, rotation, pose.ads, time.delta_secs());
        let eye = neutral_eye(transform, movement)
            + transform.rotation * common::humanoid::aim_lean_offset(pose.aim_roll);
        let muzzle =
            eye + rotation * common::weapon_animation::resolved_muzzle_offset(spec, pose.ads);
        // Resolve actual eye-to-muzzle contacts in projectiles_tick. A broad
        // movement capsule must neither redirect a clear offset ray to the eye
        // nor let the device outlet spawn beyond real anatomy/concrete at point blank.
        let id = world_state.next_projectile_id;
        world_state.next_projectile_id = id + 1;
        commands.spawn((
            Projectile {
                id,
                weapon,
                velocity: direction * spec.muzzle_speed,
                damage: arsenal.loadout.ammo.damage(arsenal.current().spec()),
                penetration: arsenal.loadout.ammo.penetration(arsenal.current().spec()),
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
            ejection: common::weapon_animation::resolved_ejection_offset(
                arsenal.current().spec(),
                pose.ads,
            )
            .map(|offset| (eye + rotation * offset).into()),
            audio_signature: FiredProjectileData::encode_audio_signature(
                arsenal.current().spec().assembly.signature_multiplier,
            ),
        });
        // The current projectile uses the pre-impulse bore. Reaction changes
        // subsequent aim, once per authorized shot, independently of rendering.
        let profile = arsenal.current().spec().recoil;
        arsenal
            .recoil
            .impulse(profile, client.id ^ u64::from(raid.raid_id));
    }
}

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct ProjectileEnvironment<'w, 's> {
    lives: Query<'w, 's, &'static super::arena::Life>,
    surfaces: Query<
        'w,
        's,
        (&'static SurfaceKind, &'static Collider, &'static Transform),
        Without<Projectile>,
    >,
    physics_pieces: Query<'w, 's, (Entity, &'static mut super::physical::PhysicsPiece)>,
    rubble: Query<'w, 's, &'static super::destruction::Rubble>,
    voxels: Query<
        'w,
        's,
        (Entity, &'static mut super::BarrierVoxel, &'static Transform),
        Without<Projectile>,
    >,
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
            Option<&super::ragdoll::Motion>,
        ),
        Without<Projectile>,
    >,
    environment: ProjectileEnvironment,
) {
    let ProjectileEnvironment {
        lives,
        surfaces,
        mut physics_pieces,
        rubble,
        mut voxels,
    } = environment;
    let Ok(context) = context.single() else {
        return;
    };
    let delta = time.delta_secs();
    for (entity, mut projectile, mut transform, clearance) in &mut projectiles {
        // One projectile must pay for the material that existed before its own
        // structure-wide bore. Later projectiles in this tick see the committed
        // holes. The cache spans clearance and flight, like penetrated IDs.
        let mut cover_before = std::collections::BTreeMap::new();
        let mut damaged_structures = std::collections::BTreeSet::new();
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
                        && voxels.get(hit_entity).is_err()
                        && players.get(hit_entity).is_err()
                        && physics_pieces.get(hit_entity).is_err()
                };
                let filter = QueryFilter::new()
                    .exclude_collider(projectile.owner_entity)
                    .exclude_sensors()
                    .predicate(&predicate);
                let mut wall =
                    context.cast_ray_and_get_normal(start, direction, remaining, true, filter);
                // Use current sparse shapes, including holes carved by another
                // projectile in this same tick before Rapier's next backend sync.
                for (cell_entity, cell, cell_transform) in &voxels {
                    if projectile.penetrated.contains(&cell_entity) {
                        continue;
                    }
                    let shape = cover_before
                        .get(&cell_entity)
                        .or_else(|| (cell.health > 0.0).then_some(&cell.shape));
                    let Some(shape) = shape else {
                        continue;
                    };
                    if let Some(hit) = shape.cast_ray_and_get_normal(
                        cell_transform.translation,
                        cell_transform.rotation,
                        start,
                        direction,
                        remaining,
                        true,
                    ) && wall
                        .as_ref()
                        .is_none_or(|(_, previous)| hit.time_of_impact < previous.time_of_impact)
                    {
                        wall = Some((cell_entity, hit));
                    }
                }
                let max = wall.as_ref().map_or(remaining, |(_, h)| h.time_of_impact);
                let mut nearest = None;
                for (target, body, _, movement, t, p, pose, _, _, _, _, motion) in &players {
                    if target == projectile.owner_entity
                        || !body.alive()
                        || p.status != RaidStatus::Active
                        || lives
                            .get(target)
                            .is_ok_and(|life| projectile.id < life.projectile_floor)
                    {
                        continue;
                    }
                    let fallback = PoseState {
                        crouch: if movement.crouched { 1.0 } else { 0.0 },
                        prone: movement.prone,
                        ..default()
                    };
                    let pose = motion.and_then(|m| m.local_rig(t)).unwrap_or_else(|| {
                        evaluate_pose(
                            pose.unwrap_or(&fallback),
                            body.anatomy.capacity(Region::LeftLeg),
                            body.anatomy.capacity(Region::RightLeg),
                        )
                    });
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
                        motion,
                    ) = players.get_mut(target).unwrap();
                    let fallback = PoseState {
                        crouch: if movement.crouched { 1.0 } else { 0.0 },
                        ..default()
                    };
                    let rig = motion.and_then(|m| m.local_rig(t)).unwrap_or_else(|| {
                        evaluate_pose(
                            pose.as_deref().unwrap_or(&fallback),
                            body.anatomy.capacity(Region::LeftLeg),
                            body.anatomy.capacity(Region::RightLeg),
                        )
                    });
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
                let cell_id = voxels.get(hit_entity).ok().map(|(_, cell, _)| cell.id);
                if cell_id.is_none() && !rubble.contains(hit_entity) {
                    world_state.impact_on_cell(
                        hit.point,
                        hit.normal,
                        impact_mark_size_mm(projectile.weapon),
                        None,
                    );
                }
                let Ok((surface, collider, surface_transform)) = surfaces.get(hit_entity) else {
                    stopped = true;
                    break;
                };
                let collider = voxels.get(hit_entity).map_or_else(
                    |_| collider.clone(),
                    |(_, cell, _)| cover_before.get(&hit_entity).unwrap_or(&cell.shape).clone(),
                );
                let structure =
                    cell_id.map(|id| common::map::barrier_cells()[id as usize].structure);
                if let Some(structure) = structure
                    && damaged_structures.insert(structure)
                {
                    if cover_before.is_empty() {
                        for (entity, cell, _) in &voxels {
                            if cell.health > 0.0 {
                                cover_before.insert(entity, cell.shape.clone());
                            }
                        }
                    }
                    super::destruction::damage(
                        &mut commands,
                        &mut world_state,
                        &mut voxels,
                        super::destruction::Hit {
                            entity: hit_entity,
                            point: hit.point,
                            normal: hit.normal,
                            direction,
                        },
                        &projectile,
                        (
                            rubble.iter().count(),
                            rubble.iter().map(|r| r.boxes.len()).sum(),
                        ),
                    );
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
                if cell_id.is_none() && !rubble.contains(hit_entity) {
                    world_state.impact_on_cell(
                        exit.point,
                        exit.normal,
                        impact_mark_size_mm(projectile.weapon),
                        None,
                    );
                }
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
