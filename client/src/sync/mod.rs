use bevy::{platform::collections::HashMap, prelude::*};
use bevy_renet2::prelude::{ClientId, DefaultChannel, RenetClient, client_connected};
use common::{Lobby, PlayerInput, ServerMessage, data};

pub struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        let sync_services = (send_input, recv_players_pos, recv_connectivity);

        app.add_systems(Update, sync_services.run_if(client_connected));
    }
}

fn send_input(player_input: Res<PlayerInput>, mut client: ResMut<RenetClient>) {
    let input_message = data::encode(&*player_input);

    client.send_message(DefaultChannel::ReliableOrdered, input_message);
}

fn recv_connectivity(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut client: ResMut<RenetClient>,
    mut lobby: ResMut<Lobby>,
) {
    while let Some(message) = client.receive_message(DefaultChannel::ReliableOrdered) {
        let server_message = data::decode(&message);

        match server_message {
            ServerMessage::PlayerConnected { id } => {
                info!("Player {} connected.", id);
                let player_entity = commands
                    .spawn((
                        Mesh3d(meshes.add(Cuboid::from_size(Vec3::splat(1.0)))),
                        MeshMaterial3d(materials.add(Color::srgb(0.8, 0.7, 0.6))),
                        Transform::from_xyz(0.0, 0.5, 0.0),
                    ))
                    .id();

                lobby.players.insert(id, player_entity);
            }
            ServerMessage::PlayerDisconnected { id } => {
                info!("Player {} disconnected.", id);
                if let Some(player_entity) = lobby.players.remove(&id) {
                    commands.entity(player_entity).despawn();
                }
            }
        }
    }
}

fn recv_players_pos(mut commands: Commands, mut client: ResMut<RenetClient>, lobby: ResMut<Lobby>) {
    while let Some(message) = client.receive_message(DefaultChannel::Unreliable) {
        let players: HashMap<ClientId, [f32; 3]> = data::decode(&message);

        for (player_id, translation) in players.iter() {
            if let Some(player_entity) = lobby.players.get(player_id) {
                let transform = Transform {
                    translation: (*translation).into(),
                    ..Default::default()
                };

                commands.entity(*player_entity).insert(transform);
            }
        }
    }
}
