use bevy::{platform::collections::HashMap, prelude::*};

use bevy_renet2::prelude::{ClientId, DefaultChannel, RenetServer, ServerEvent};
use common::*;

pub struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, update)
            .add_systems(Update, move_players)
            .add_systems(Update, sync_players);
    }
}

fn move_players(mut query: Query<(&mut Transform, &PlayerInput)>, time: Res<Time>) {
    for (mut transform, input) in query.iter_mut() {
        let x = (input.right as i8 - input.left as i8) as f32;
        let y = (input.down as i8 - input.up as i8) as f32;
        let adjustment = if x != 0. && y != 0. { 0.7 } else { 1.0 };
        transform.translation.x += x * PLAYER_MOVE_SPEED * adjustment * time.delta().as_secs_f32();
        transform.translation.z += y * PLAYER_MOVE_SPEED * adjustment * time.delta().as_secs_f32();
    }
}

fn sync_players(mut server: ResMut<RenetServer>, query: Query<(&Transform, &Player)>) {
    let mut players: HashMap<ClientId, [f32; 3]> = HashMap::new();
    for (transform, player) in query.iter() {
        players.insert(player.id, transform.translation.into());
    }

    let sync_message = data::encode(players);

    server.broadcast_message(DefaultChannel::Unreliable, sync_message);
}

fn update(
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
                    .spawn((Transform::from_xyz(0.0, 0.5, 0.0),))
                    .insert(PlayerInput::default())
                    .insert(Player { id: *client_id })
                    .id();

                // We could send an InitState with all the players id and positions for the client
                // but this is easier to do.
                for &player_id in lobby.players.keys() {
                    let message = data::encode(ServerMessage::PlayerConnected { id: player_id });
                    server.send_message(*client_id, DefaultChannel::ReliableOrdered, message);
                }

                lobby.players.insert(*client_id, player_entity);

                let message = data::encode(ServerMessage::PlayerConnected { id: *client_id });

                server.broadcast_message(DefaultChannel::ReliableOrdered, message);
            }
            ServerEvent::ClientDisconnected { client_id, reason } => {
                info!("Player {} disconnected: {}", client_id, reason);
                if let Some(player_entity) = lobby.players.remove(client_id) {
                    commands.entity(player_entity).despawn();
                }

                let message = data::encode(ServerMessage::PlayerDisconnected { id: *client_id });

                server.broadcast_message(DefaultChannel::ReliableOrdered, message);
            }
        }
    }

    for client_id in server.clients_id() {
        while let Some(message) = server.receive_message(client_id, DefaultChannel::ReliableOrdered)
        {
            let player_input: PlayerInput = data::decode(&message);

            if let Some(player_entity) = lobby.players.get(&client_id) {
                commands.entity(*player_entity).insert(player_input);
            }
        }
    }
}
