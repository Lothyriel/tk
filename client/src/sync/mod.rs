use bevy::prelude::*;
use bevy_renet2::prelude::{DefaultChannel, RenetClient, client_connected};
use common::{
    ImpactMarkData, Lobby, PlayerId, PlayerVisualState, ProjectileData, ServerMessage,
    WeaponKind, WorldSnapshot, data,
};

use crate::render::{ImpactMarkVisual, ProjectileVisual, player_body_mesh};

pub struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WeaponAudio>()
            .add_systems(Update, (send_input, recv_players_pos, recv_connectivity).run_if(client_connected));
    }
}

#[derive(Debug, Resource)]
struct WeaponAudio {
    rifle: Handle<AudioSource>,
    pistol: Handle<AudioSource>,
}

impl FromWorld for WeaponAudio {
    fn from_world(world: &mut World) -> Self {
        let asset_server = world.resource::<AssetServer>();

        Self {
            rifle: asset_server.load("sound/rifle-shot.ogg"),
            pistol: asset_server.load("sound/pistol-shot.ogg"),
        }
    }
}

fn send_input(player_input: Res<common::ClientInput>, mut client: ResMut<RenetClient>) {
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
        let event: ServerMessage = data::decode(&message);

        match event {
            ServerMessage::ClientConnected { id } => {
                info!("Player {} connected.", id);

                if id == player_id.0 {
                    commands
                        .get_entity(lobby.players[&id])
                        .expect("Player to be in lobby")
                        .insert(PlayerVisualState::default());
                } else {
                    let client = commands.spawn((
                        PlayerVisualState::default(),
                        children![player_body_mesh(
                            meshes.add(Cuboid::from_size(Vec3::splat(1.0))),
                            materials.add(Color::srgb(0.8, 0.7, 0.6)),
                        )],
                    ));

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

fn recv_players_pos(
    mut commands: Commands,
    mut client: ResMut<RenetClient>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    lobby: Res<Lobby>,
    projectile_visuals: Query<(Entity, &ProjectileVisual)>,
    impact_visuals: Query<(Entity, &ImpactMarkVisual)>,
    player_id: Res<PlayerId>,
    weapon_audio: Res<WeaponAudio>,
) {
    while let Some(message) = client.receive_message(DefaultChannel::Unreliable) {
        let snapshot: WorldSnapshot = data::decode(&message);

        for player in snapshot.players.iter() {
            if let Some(player_entity) = lobby.players.get(&player.id) {
                commands.entity(*player_entity).insert((
                    Transform {
                        translation: player.pos.into(),
                        rotation: (&player.rot).into(),
                        ..Default::default()
                    },
                    PlayerVisualState {
                        alive: player.alive,
                        crouched: player.crouched,
                        health: player.health,
                        weapon: player.weapon,
                        ammo_in_mag: player.ammo_in_mag,
                    },
                ));
            }
        }

        let local_weapon = snapshot
            .players
            .iter()
            .find(|player| Some(player.id) == Some(player_id.0))
            .map(|player| player.weapon)
            .unwrap_or(WeaponKind::Rifle);

        for _projectile_id in snapshot.fired_projectile_ids.iter() {
            commands.spawn((
                AudioPlayer::new(match local_weapon {
                    WeaponKind::Rifle => weapon_audio.rifle.clone(),
                    WeaponKind::Pistol => weapon_audio.pistol.clone(),
                }),
                PlaybackSettings::DESPAWN,
            ));
        }

        sync_projectile_visuals(
            &mut commands,
            &mut meshes,
            &mut materials,
            &projectile_visuals,
            &snapshot.projectiles,
        );
        sync_impact_visuals(
            &mut commands,
            &mut meshes,
            &mut materials,
            &impact_visuals,
            &snapshot.impact_marks,
        );
    }
}

fn sync_projectile_visuals(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    projectile_visuals: &Query<(Entity, &ProjectileVisual)>,
    projectiles: &[ProjectileData],
) {
    let mut seen_ids = Vec::with_capacity(projectiles.len());

    for projectile in projectiles {
        seen_ids.push(projectile.id);

        let mut existing = None;
        for (entity, visual) in projectile_visuals.iter() {
            if visual.id == projectile.id {
                existing = Some(entity);
                break;
            }
        }

        let velocity: Vec3 = projectile.vel.into();
        let mut transform = Transform::from_translation(projectile.pos.into());
        if velocity.length_squared() > 0.0 {
            transform.look_to(velocity.normalize(), Vec3::Y);
        }
        transform.scale = Vec3::new(0.03, 0.03, 0.45);

        if let Some(entity) = existing {
            commands.entity(entity).insert(transform);
        } else {
            commands.spawn((
                ProjectileVisual { id: projectile.id },
                Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(1.0, 0.7, 0.2),
                    emissive: LinearRgba::rgb(8.0, 4.0, 0.5),
                    ..default()
                })),
                transform,
            ));
        }
    }

    for (entity, visual) in projectile_visuals.iter() {
        if !seen_ids.contains(&visual.id) {
            commands.entity(entity).despawn();
        }
    }
}

fn sync_impact_visuals(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    impact_visuals: &Query<(Entity, &ImpactMarkVisual)>,
    impacts: &[ImpactMarkData],
) {
    for impact in impacts {
        let mut exists = false;

        for (_, visual) in impact_visuals.iter() {
            if visual.id == impact.id {
                exists = true;
                break;
            }
        }

        if exists {
            continue;
        }

        let normal: Vec3 = impact.normal.into();
        let mut transform = Transform::from_translation(Vec3::from(impact.pos) + normal * 0.01);
        transform.look_to(normal, Vec3::Y);
        transform.scale = Vec3::new(0.18, 0.18, 0.01);

        commands.spawn((
            ImpactMarkVisual { id: impact.id },
            Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
            MeshMaterial3d(materials.add(Color::srgb(0.08, 0.08, 0.08))),
            transform,
        ));
    }
}
