use bevy::prelude::*;
use bevy_renet2::prelude::{DefaultChannel, RenetClient, client_connected};
use common::{ClientData, ClientInput, Lobby, PlayerId, ServerMessage, data};

pub struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        let sync_services = (send_input, recv_players_pos, recv_connectivity);

        app.add_systems(Update, sync_services.run_if(client_connected));
    }
}

fn send_input(player_input: Res<ClientInput>, mut client: ResMut<RenetClient>) {
    let input_message = data::encode(&*player_input);

    client.send_message(DefaultChannel::ReliableOrdered, input_message);
}

fn recv_connectivity(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut client: ResMut<RenetClient>,
    mut lobby: ResMut<Lobby>,
    player_id: Res<PlayerId>,
) {
    while let Some(message) = client.receive_message(DefaultChannel::ReliableOrdered) {
        let event = data::decode(&message);

        match event {
            ServerMessage::ClientConnected { id } => {
                info!("Player {} connected.", id);

                let client_bundle = (
                    Mesh3d(meshes.add(Cuboid::from_size(Vec3::splat(1.0)))),
                    MeshMaterial3d(materials.add(Color::srgb(0.8, 0.7, 0.6))),
                    Transform::from_xyz(0.0, 0.5, 0.0),
                );

                // this probably needs to change, shadows are weird
                // (prob not considering the camera)
                if id == player_id.0 {
                    commands
                        .get_entity(lobby.players[&id])
                        .expect("Player to be in lobby")
                        .insert(client_bundle);
                } else {
                    let client = commands.spawn(client_bundle);

                    lobby.players.insert(id, client.id());
                }
            }
            ServerMessage::ClientDisconnected { id } => {
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
        let players: Vec<ClientData> = data::decode(&message);

        for ClientData { id, pos, rot } in players.iter() {
            if let Some(player_entity) = lobby.players.get(id) {
                let transform = Transform {
                    translation: (*pos).into(),
                    rotation: rot.into(),
                    ..Default::default()
                };

                commands.entity(*player_entity).insert(transform);
            }
        }
    }
}
