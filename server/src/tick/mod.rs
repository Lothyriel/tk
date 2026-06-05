use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use bevy_renet2::prelude::{DefaultChannel, RenetServer, ServerEvent};
use common::*;

pub struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(128.0))
            .add_systems(Startup, spawn_world_colliders)
            .add_systems(FixedUpdate, recv_connectivity)
            .add_systems(FixedUpdate, recv_players_input)
            .add_systems(FixedUpdate, physx_tick)
            .add_systems(PostUpdate, sync_ground_state)
            .add_systems(FixedUpdate, send_players_pos);
    }
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

fn physx_tick(
    mut query: Query<(
        &ClientInput,
        &mut MovementState,
        &mut KinematicCharacterController,
        Option<&KinematicCharacterControllerOutput>,
        &mut Transform,
    )>,
    time: Res<Time>,
) {
    let delta = time.delta_secs();

    for (input, mut movement, mut controller, output, mut transform) in query.iter_mut() {
        let x = (input.right as i8 - input.left as i8) as f32;
        let z = (input.backward as i8 - input.forward as i8) as f32;

        let local_input = Vec3::new(x, 0.0, z).normalize_or_zero();
        let yaw_rotation = Quat::from_rotation_y(input.camera.yaw);
        let wish_dir = yaw_rotation * local_input;

        let horizontal_velocity = Vec3::new(movement.velocity.x, 0.0, movement.velocity.z);
        let target_speed = if input.run { PLAYER_RUN_SPEED } else { PLAYER_WALK_SPEED };
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
        let next_horizontal_velocity = horizontal_velocity + horizontal_delta * accel_factor * air_control;

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

fn sync_ground_state(
    mut query: Query<(&mut MovementState, &KinematicCharacterControllerOutput)>,
) {
    for (mut movement, output) in query.iter_mut() {
        movement.grounded = output.grounded;

        if output.grounded && movement.velocity.y < 0.0 {
            movement.velocity.y = 0.0;
        }
    }
}

fn send_players_pos(mut server: ResMut<RenetServer>, query: Query<(&Transform, &Client)>) {
    let players: Vec<_> = query
        .iter()
        .map(|(transform, client)| ClientData {
            id: client.id,
            pos: transform.translation.into(),
            rot: transform.rotation.into(),
        })
        .collect();

    let sync_message = data::encode_message(&players);

    server.broadcast_message(DefaultChannel::Unreliable, sync_message);
}

fn recv_players_input(
    mut commands: Commands,
    mut server: ResMut<RenetServer>,
    lobby: ResMut<Lobby>,
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

                // Spawn player cube
                let player_entity = commands
                    .spawn(Client { id: *client_id })
                    .insert(ClientInput::default())
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
                    .insert(Transform::from_xyz(0.0, 1.5, 0.0))
                    .id();

                // We could send an InitState with all the players id and positions for the client
                // but this is easier to do.
                for &player_id in lobby.players.keys() {
                    let message = data::encode_message(&ServerMessage::ClientConnected { id: player_id });
                    server.send_message(*client_id, DefaultChannel::ReliableOrdered, message);
                }

                lobby.players.insert(*client_id, player_entity);

                let message = data::encode_message(&ServerMessage::ClientConnected { id: *client_id });

                server.broadcast_message(DefaultChannel::ReliableOrdered, message);
            }
            ServerEvent::ClientDisconnected { client_id, reason } => {
                info!("Player {} disconnected: {}", client_id, reason);

                if let Some(player_entity) = lobby.players.remove(client_id) {
                    commands.entity(player_entity).despawn();
                }

                let message = data::encode_message(&ServerMessage::ClientDisconnected { id: *client_id });

                server.broadcast_message(DefaultChannel::ReliableOrdered, message);
            }
        }
    }
}
