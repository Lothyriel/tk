use bevy::prelude::*;

use bevy_renet2::prelude::{DefaultChannel, RenetServer, ServerEvent};
use common::*;

pub struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(128.0))
            .add_systems(FixedUpdate, recv_connectivity)
            .add_systems(FixedUpdate, recv_players_input)
            .add_systems(FixedUpdate, physx_tick)
            .add_systems(FixedUpdate, send_players_pos);
    }
}

fn physx_tick(mut query: Query<(&mut Transform, &ClientInput)>, time: Res<Time>) {
    for (mut transform, input) in query.iter_mut() {
        let x = (input.right as i8 - input.left as i8) as f32;
        let z = (input.backward as i8 - input.forward as i8) as f32;

        let input_dir = Vec3::new(x, 0.0, z);

        // to avoid speeding up diagonally
        let adjustment = if input_dir.x != 0.0 && input_dir.z != 0.0 {
            0.7
        } else {
            1.0
        };

        // Extract yaw rotation
        let yaw_rotation = Quat::from_rotation_y(input.camera.yaw);

        // Rotate the input direction by the player's yaw
        let movement = yaw_rotation * input_dir.normalize_or_zero();

        // Apply movement
        transform.translation +=
            movement * PLAYER_MOVE_SPEED * adjustment * time.delta().as_secs_f32();

        let CameraInput { yaw, pitch, roll } = input.camera;
        transform.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll);
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

    let sync_message = data::encode(players);

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
    mut server_events: EventReader<ServerEvent>,
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
                    .insert(Transform::from_xyz(0.0, 0.5, 0.0))
                    .id();

                // We could send an InitState with all the players id and positions for the client
                // but this is easier to do.
                for &player_id in lobby.players.keys() {
                    let message = data::encode(ServerMessage::ClientConnected { id: player_id });
                    server.send_message(*client_id, DefaultChannel::ReliableOrdered, message);
                }

                lobby.players.insert(*client_id, player_entity);

                let message = data::encode(ServerMessage::ClientConnected { id: *client_id });

                server.broadcast_message(DefaultChannel::ReliableOrdered, message);
            }
            ServerEvent::ClientDisconnected { client_id, reason } => {
                info!("Player {} disconnected: {}", client_id, reason);

                if let Some(player_entity) = lobby.players.remove(client_id) {
                    commands.entity(player_entity).despawn();
                }

                let message = data::encode(ServerMessage::ClientDisconnected { id: *client_id });

                server.broadcast_message(DefaultChannel::ReliableOrdered, message);
            }
        }
    }
}
