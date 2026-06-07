use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use bevy_renet2::prelude::{DefaultChannel, RenetServer, ServerEvent};
use common::*;

pub struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(128.0))
            .init_resource::<WorldState>()
            .add_systems(Startup, spawn_world_colliders)
            .add_systems(FixedUpdate, recv_connectivity)
            .add_systems(FixedUpdate, recv_players_input)
            .add_systems(FixedUpdate, respawn_tick.after(recv_players_input))
            .add_systems(FixedUpdate, physx_tick.after(respawn_tick))
            .add_systems(FixedUpdate, weapons_tick.after(physx_tick))
            .add_systems(FixedUpdate, projectiles_tick.after(weapons_tick))
            .add_systems(PostUpdate, sync_ground_state)
            .add_systems(FixedUpdate, send_world_snapshot.after(projectiles_tick));
    }
}

#[derive(Debug, Default, Resource)]
struct WorldState {
    next_projectile_id: u64,
    next_mark_id: u64,
    impact_marks: Vec<ImpactMarkData>,
    fired_projectile_ids: Vec<u64>,
}

#[derive(Debug, Component)]
struct Health {
    current: f32,
}

#[derive(Debug, Component)]
struct Arsenal {
    magazines: [u32; 2],
    active_weapon: WeaponKind,
    last_fire_pressed_sequence: u32,
    last_reload_sequence: u32,
    last_respawn_sequence: u32,
    reload_timer: f32,
    reload_weapon: Option<WeaponKind>,
    last_shot_at: f32,
}

impl Default for Arsenal {
    fn default() -> Self {
        Self {
            magazines: [
                WeaponKind::Rifle.spec().magazine_size,
                WeaponKind::Pistol.spec().magazine_size,
            ],
            active_weapon: WeaponKind::Rifle,
            last_fire_pressed_sequence: 0,
            last_reload_sequence: 0,
            last_respawn_sequence: 0,
            reload_timer: 0.0,
            reload_weapon: None,
            last_shot_at: f32::NEG_INFINITY,
        }
    }
}

#[derive(Debug, Component)]
struct Projectile {
    id: u64,
    velocity: Vec3,
    damage: f32,
    lifetime: f32,
    owner_entity: Entity,
}

fn spawn_world_colliders(mut commands: Commands) {
    commands.spawn(Collider::cuboid(10.0, 0.1, 10.0));

    commands.spawn((
        Collider::cuboid(1.0, 0.25, 0.5),
        Transform::from_xyz(0.0, 0.25, -3.0),
    ));

    commands.spawn((
        Collider::cuboid(1.0, 0.25, 0.5),
        Transform::from_xyz(0.75, 1.75, 0.0),
    ));
}

fn respawn_tick(
    mut query: Query<(
        &ClientInput,
        &mut Health,
        &mut Arsenal,
        &mut MovementState,
        &mut Collider,
        &mut Transform,
    )>,
) {
    for (input, mut health, mut arsenal, mut movement, mut collider, mut transform) in query.iter_mut() {
        if health.current > 0.0 {
            arsenal.last_respawn_sequence = input.respawn_sequence;
            continue;
        }

        if input.respawn_sequence == arsenal.last_respawn_sequence {
            continue;
        }

        arsenal.last_respawn_sequence = input.respawn_sequence;
        health.current = PLAYER_MAX_HEALTH;
        arsenal.magazines = [
            WeaponKind::Rifle.spec().magazine_size,
            WeaponKind::Pistol.spec().magazine_size,
        ];
        arsenal.reload_timer = 0.0;
        arsenal.reload_weapon = None;
        movement.velocity = Vec3::ZERO;

        set_crouched_state(&mut movement, &mut collider, &mut transform, false);
        transform.translation = Vec3::new(0.0, PLAYER_RESPAWN_HEIGHT, 0.0);
    }
}

fn physx_tick(
    rapier_context: ReadRapierContext,
    mut query: Query<(
        Entity,
        &ClientInput,
        &Health,
        &mut MovementState,
        &mut Collider,
        &mut KinematicCharacterController,
        Option<&KinematicCharacterControllerOutput>,
        &mut Transform,
    )>,
    time: Res<Time>,
) {
    let rapier_context = rapier_context
        .single()
        .expect("Default Rapier context to exist");
    let delta = time.delta_secs();

    for (entity, input, health, mut movement, mut collider, mut controller, output, mut transform) in
        query.iter_mut()
    {
        if health.current <= 0.0 {
            movement.velocity = Vec3::ZERO;
            controller.translation = Some(Vec3::ZERO);
            continue;
        }

        let wants_to_crouch = input.crouch;

        if wants_to_crouch && !movement.crouched {
            set_crouched_state(&mut movement, &mut collider, &mut transform, true);
        } else if !wants_to_crouch
            && movement.crouched
            && can_stand_up(entity, &rapier_context, transform.translation)
        {
            set_crouched_state(&mut movement, &mut collider, &mut transform, false);
        }

        let x = (input.right as i8 - input.left as i8) as f32;
        let z = (input.backward as i8 - input.forward as i8) as f32;

        let local_input = Vec3::new(x, 0.0, z).normalize_or_zero();
        let yaw_rotation = Quat::from_rotation_y(input.camera.yaw);
        let wish_dir = yaw_rotation * local_input;

        let horizontal_velocity = Vec3::new(movement.velocity.x, 0.0, movement.velocity.z);
        let target_speed = if movement.crouched {
            PLAYER_CROUCH_SPEED
        } else if input.run {
            PLAYER_RUN_SPEED
        } else {
            PLAYER_WALK_SPEED
        };
        let target_horizontal_velocity = wish_dir * target_speed;

        let grounded = output.is_some_and(|output| output.grounded);
        movement.grounded = grounded;

        let horizontal_delta = target_horizontal_velocity - horizontal_velocity;

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

        let accel_factor = (accel * delta).min(1.0);
        let air_control = if grounded { 1.0 } else { PLAYER_AIR_CONTROL };
        let next_horizontal_velocity =
            horizontal_velocity + horizontal_delta * accel_factor * air_control;

        movement.velocity.x = next_horizontal_velocity.x;
        movement.velocity.z = next_horizontal_velocity.z;

        if grounded {
            if movement.velocity.y < 0.0 {
                movement.velocity.y = 0.0;
            }

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

fn weapons_tick(
    mut commands: Commands,
    time: Res<Time>,
    mut world_state: ResMut<WorldState>,
    mut query: Query<(
        Entity,
        &ClientInput,
        &Transform,
        &MovementState,
        &Health,
        &mut Arsenal,
    )>,
) {
    let now = time.elapsed_secs();
    let delta = time.delta_secs();
    world_state.fired_projectile_ids.clear();

    for (entity, input, transform, movement, health, mut arsenal) in query.iter_mut() {
        if health.current <= 0.0 {
            continue;
        }

        arsenal.active_weapon = input.weapon;

        if arsenal.reload_timer > 0.0 {
            arsenal.reload_timer = (arsenal.reload_timer - delta).max(0.0);

            if arsenal.reload_timer == 0.0 {
                if let Some(weapon) = arsenal.reload_weapon.take() {
                    *ammo_for_weapon_mut(&mut arsenal, weapon) = weapon.spec().magazine_size;
                }
            }
        }

        if input.reload_sequence != arsenal.last_reload_sequence {
            arsenal.last_reload_sequence = input.reload_sequence;

            let active_weapon = arsenal.active_weapon;
            let spec = active_weapon.spec();
            if *ammo_for_weapon(&arsenal, active_weapon) < spec.magazine_size && arsenal.reload_timer == 0.0
            {
                arsenal.reload_timer = spec.reload_seconds;
                arsenal.reload_weapon = Some(active_weapon);
            }
        }

        if arsenal.reload_timer > 0.0 {
            continue;
        }

        let active_weapon = arsenal.active_weapon;
        let spec = active_weapon.spec();
        let wants_to_fire = if spec.automatic {
            input.fire
        } else {
            input.fire_pressed_sequence != arsenal.last_fire_pressed_sequence
        };

        if !wants_to_fire {
            continue;
        }

        if now - arsenal.last_shot_at < spec.seconds_per_shot() {
            continue;
        }

        if *ammo_for_weapon(&arsenal, active_weapon) == 0 {
            arsenal.reload_timer = spec.reload_seconds;
            arsenal.reload_weapon = Some(active_weapon);
            arsenal.last_fire_pressed_sequence = input.fire_pressed_sequence;
            continue;
        }

        arsenal.last_shot_at = now;
        arsenal.last_fire_pressed_sequence = input.fire_pressed_sequence;
        *ammo_for_weapon_mut(&mut arsenal, active_weapon) -= 1;

        let muzzle_rotation = Quat::from(&input.camera);
        let muzzle_dir = (muzzle_rotation * -Vec3::Z).normalize_or_zero();
        let crouch_offset = if movement.crouched {
            PLAYER_CROUCH_VIEW_OFFSET
        } else {
            0.0
        };
        let muzzle_origin = transform.translation
            + Vec3::new(0.0, 0.55 + crouch_offset, 0.0)
            + muzzle_dir * 0.7;
        let projectile_id = world_state.next_projectile_id;

        commands.spawn((
            Projectile {
                id: projectile_id,
                velocity: muzzle_dir * spec.muzzle_speed,
                damage: spec.damage,
                lifetime: PROJECTILE_LIFETIME,
                owner_entity: entity,
            },
            Transform::from_translation(muzzle_origin),
        ));

        world_state.fired_projectile_ids.push(projectile_id);
        world_state.next_projectile_id = world_state.next_projectile_id.wrapping_add(1);

        if *ammo_for_weapon(&arsenal, active_weapon) == 0 {
            arsenal.reload_timer = spec.reload_seconds;
            arsenal.reload_weapon = Some(active_weapon);
        }
    }
}

fn projectiles_tick(
    mut commands: Commands,
    mut world_state: ResMut<WorldState>,
    rapier_context: ReadRapierContext,
    time: Res<Time>,
    mut projectiles: Query<(Entity, &mut Projectile, &mut Transform)>,
    mut players: Query<
        (Entity, &mut Health, &mut MovementState, &mut Collider, &mut Transform),
        (With<Client>, Without<Projectile>),
    >,
) {
    const MAX_IMPACT_MARKS: usize = 256;

    let rapier_context = rapier_context
        .single()
        .expect("Default Rapier context to exist");
    let delta = time.delta_secs();

    for (entity, mut projectile, mut transform) in projectiles.iter_mut() {
        let start = transform.translation;
        projectile.velocity.y -= PROJECTILE_GRAVITY * delta;
        let displacement = projectile.velocity * delta;
        let distance = displacement.length();

        if distance > 0.0 {
            let direction = displacement / distance;
            let filter = QueryFilter::new()
                .exclude_collider(projectile.owner_entity)
                .exclude_sensors();

            if let Some((hit_entity, hit)) =
                rapier_context.cast_ray_and_get_normal(start, direction, distance, true, filter)
            {
                let mut hit_player = false;

                if let Ok((_, mut health, mut movement, mut collider, mut player_transform)) =
                    players.get_mut(hit_entity)
                {
                    health.current = (health.current - projectile.damage).max(0.0);

                    if health.current <= 0.0 {
                        movement.velocity = Vec3::ZERO;
                        set_crouched_state(&mut movement, &mut collider, &mut player_transform, false);
                    }

                    hit_player = true;
                }

                if !hit_player {
                    if world_state.impact_marks.len() == MAX_IMPACT_MARKS {
                        world_state.impact_marks.remove(0);
                    }

                    let mark_id = world_state.next_mark_id;
                    world_state.next_mark_id = world_state.next_mark_id.wrapping_add(1);

                    world_state.impact_marks.push(ImpactMarkData {
                        id: mark_id,
                        pos: hit.point.into(),
                        normal: hit.normal.into(),
                    });
                }

                commands.entity(entity).despawn();
                continue;
            }
        }

        transform.translation += displacement;
        projectile.lifetime -= delta;

        if projectile.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

fn sync_ground_state(mut query: Query<(&mut MovementState, &KinematicCharacterControllerOutput)>) {
    for (mut movement, output) in query.iter_mut() {
        movement.grounded = output.grounded;

        if output.grounded && movement.velocity.y < 0.0 {
            movement.velocity.y = 0.0;
        }
    }
}

fn send_world_snapshot(
    mut server: ResMut<RenetServer>,
    world_state: Res<WorldState>,
    players: Query<(&Transform, &Client, &MovementState, &Health, &Arsenal)>,
    projectiles: Query<(&Projectile, &Transform)>,
) {
    let players = players
        .iter()
        .map(|(transform, client, movement, health, arsenal)| ClientData {
            id: client.id,
            pos: transform.translation.into(),
            rot: transform.rotation.into(),
            crouched: movement.crouched,
            alive: health.current > 0.0,
            health: health.current,
            weapon: arsenal.active_weapon,
            ammo_in_mag: *ammo_for_weapon(arsenal, arsenal.active_weapon),
        })
        .collect();

    let projectiles = projectiles
        .iter()
        .map(|(projectile, transform)| ProjectileData {
            id: projectile.id,
            pos: transform.translation.into(),
            vel: projectile.velocity.into(),
        })
        .collect();

    let snapshot = WorldSnapshot {
        players,
        projectiles,
        impact_marks: world_state.impact_marks.clone(),
        fired_projectile_ids: world_state.fired_projectile_ids.clone(),
    };

    let sync_message = data::encode(&snapshot);

    server.broadcast_message(DefaultChannel::Unreliable, sync_message);
}

fn recv_players_input(
    mut commands: Commands,
    mut server: ResMut<RenetServer>,
    lobby: Res<Lobby>,
) {
    for client_id in server.clients_id() {
        while let Some(message) = server.receive_message(client_id, DefaultChannel::ReliableOrdered)
        {
            let player_input: ClientInput = data::decode(&message);

            if let Some(player_entity) = lobby.players.get(&client_id) {
                commands.entity(*player_entity).insert(player_input);
            }
        }
    }
}

fn recv_connectivity(
    mut server_events: MessageReader<ServerEvent>,
    mut commands: Commands,
    mut lobby: ResMut<Lobby>,
    mut server: ResMut<RenetServer>,
) {
    for event in server_events.read() {
        match event {
            ServerEvent::ClientConnected { client_id } => {
                info!("Player {} connected.", client_id);

                let player_entity = commands
                    .spawn(Client { id: *client_id })
                    .insert(ClientInput::default())
                    .insert(Health {
                        current: PLAYER_MAX_HEALTH,
                    })
                    .insert(Arsenal::default())
                    .insert(Collider::capsule_y(
                        PLAYER_COLLIDER_HALF_HEIGHT,
                        PLAYER_COLLIDER_RADIUS,
                    ))
                    .insert(KinematicCharacterController {
                        autostep: Some(CharacterAutostep {
                            max_height: CharacterLength::Absolute(PLAYER_STEP_HEIGHT),
                            min_width: CharacterLength::Absolute(PLAYER_COLLIDER_RADIUS * 2.0),
                            include_dynamic_bodies: false,
                        }),
                        snap_to_ground: Some(CharacterLength::Absolute(0.2)),
                        ..Default::default()
                    })
                    .insert(MovementState {
                        grounded: true,
                        ..Default::default()
                    })
                    .insert(Transform::from_xyz(0.0, PLAYER_RESPAWN_HEIGHT, 0.0))
                    .id();

                for &player_id in lobby.players.keys() {
                    let message = data::encode(&ServerMessage::ClientConnected { id: player_id });
                    server.send_message(*client_id, DefaultChannel::ReliableOrdered, message);
                }

                lobby.players.insert(*client_id, player_entity);

                let message = data::encode(&ServerMessage::ClientConnected { id: *client_id });

                server.broadcast_message(DefaultChannel::ReliableOrdered, message);
            }
            ServerEvent::ClientDisconnected { client_id, reason } => {
                info!("Player {} disconnected: {}", client_id, reason);

                if let Some(player_entity) = lobby.players.remove(client_id) {
                    commands.entity(player_entity).despawn();
                }

                let message = data::encode(&ServerMessage::ClientDisconnected { id: *client_id });

                server.broadcast_message(DefaultChannel::ReliableOrdered, message);
            }
        }
    }
}

fn set_crouched_state(
    movement: &mut MovementState,
    collider: &mut Collider,
    transform: &mut Transform,
    crouched: bool,
) {
    if movement.crouched == crouched {
        return;
    }

    movement.crouched = crouched;
    *collider = Collider::capsule_y(
        current_collider_half_height(crouched),
        PLAYER_COLLIDER_RADIUS,
    );
    transform.translation.y += if crouched {
        crouched_eye_height() - standing_eye_height()
    } else {
        standing_eye_height() - crouched_eye_height()
    };
}

fn can_stand_up(entity: Entity, rapier_context: &RapierContext<'_>, translation: Vec3) -> bool {
    let standing_shape = Collider::capsule_y(PLAYER_COLLIDER_HALF_HEIGHT, PLAYER_COLLIDER_RADIUS);
    let shape_position = translation + Vec3::Y * (standing_eye_height() - crouched_eye_height());
    let filter = QueryFilter::new()
        .exclude_collider(entity)
        .exclude_sensors();

    rapier_context
        .cast_shape(
            shape_position,
            Quat::IDENTITY,
            Vec3::ZERO,
            (&standing_shape).into(),
            ShapeCastOptions {
                max_time_of_impact: 0.0,
                stop_at_penetration: true,
                compute_impact_geometry_on_penetration: false,
                target_distance: 0.0,
            },
            filter,
        )
        .is_none()
}

fn current_collider_half_height(crouched: bool) -> f32 {
    if crouched {
        PLAYER_CROUCH_COLLIDER_HALF_HEIGHT
    } else {
        PLAYER_COLLIDER_HALF_HEIGHT
    }
}

fn standing_eye_height() -> f32 {
    PLAYER_COLLIDER_HALF_HEIGHT + PLAYER_COLLIDER_RADIUS
}

fn crouched_eye_height() -> f32 {
    PLAYER_CROUCH_COLLIDER_HALF_HEIGHT + PLAYER_COLLIDER_RADIUS
}

fn weapon_index(weapon: WeaponKind) -> usize {
    match weapon {
        WeaponKind::Rifle => 0,
        WeaponKind::Pistol => 1,
    }
}

fn ammo_for_weapon(arsenal: &Arsenal, weapon: WeaponKind) -> &u32 {
    &arsenal.magazines[weapon_index(weapon)]
}

fn ammo_for_weapon_mut(arsenal: &mut Arsenal, weapon: WeaponKind) -> &mut u32 {
    &mut arsenal.magazines[weapon_index(weapon)]
}
