use bevy::prelude::*;
use bevy_renet2::prelude::{DefaultChannel, RenetClient, client_connected};
use common::{
    ClientData, ImpactMarkData, Lobby, PlayerId, PlayerVisualState, ProjectileData, ServerMessage,
    WeaponKind, WorldSnapshot, data,
};

use crate::ClientSystems;
use crate::render::{ImpactMarkVisual, ProjectileVisual, player_body_mesh};
use common::raid::{RaidPhase, RaidState, RaidStatus};

#[derive(Debug, Default, Resource)]
pub(crate) struct ClientSession {
    pub raid: RaidState,
    pub players: Vec<ClientData>,
    pub last_sequence: u64,
    pub last_received: f64,
}

impl ClientSession {
    pub fn local(&self, id: u64) -> Option<&ClientData> {
        self.players.iter().find(|p| p.id == id)
    }
    pub fn playing(&self, id: u64) -> bool {
        self.raid.phase == RaidPhase::Active
            && self
                .local(id)
                .is_some_and(|p| p.participant.status == RaidStatus::Active && p.body.alive())
    }
}

pub struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WeaponAudio>()
            .init_resource::<ClientSession>()
            .add_systems(
                Update,
                (recv_connectivity, recv_players_pos)
                    .chain()
                    .in_set(ClientSystems::Receive)
                    .run_if(client_connected),
            )
            .add_systems(
                Update,
                send_input
                    .in_set(ClientSystems::Send)
                    .run_if(client_connected),
            );
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

fn send_input(player_input: Res<common::LocalInput>, mut client: ResMut<RenetClient>) {
    let input_message = data::encode(&**player_input);

    client.send_message(DefaultChannel::ReliableOrdered, input_message);
}

fn recv_connectivity(
    mut commands: Commands,
    mut client: ResMut<RenetClient>,
    mut lobby: ResMut<Lobby>,
) {
    while let Some(message) = client.receive_message(DefaultChannel::ReliableOrdered) {
        let Ok(event) = data::try_decode::<ServerMessage>(&message) else {
            continue;
        };

        match event {
            ServerMessage::ClientConnected { id } => {
                info!("Player {} connected.", id);

                // A snapshot creates the entity, so channel ordering cannot lose a player.
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

#[allow(clippy::too_many_arguments)] // Independent Bevy system parameters, not positional application arguments.
fn recv_players_pos(
    mut commands: Commands,
    mut client: ResMut<RenetClient>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut lobby: ResMut<Lobby>,
    projectile_visuals: Query<(Entity, &ProjectileVisual)>,
    impact_visuals: Query<(Entity, &ImpactMarkVisual)>,
    player_id: Res<PlayerId>,
    weapon_audio: Res<WeaponAudio>,
    mut session: ResMut<ClientSession>,
    time: Res<Time<Real>>,
) {
    let mut latest: Option<WorldSnapshot> = None;
    while let Some(message) = client.receive_message(DefaultChannel::Unreliable) {
        let Ok(snapshot) = data::try_decode::<WorldSnapshot>(&message) else {
            continue;
        };
        if snapshot.sequence > session.last_sequence
            && latest
                .as_ref()
                .is_none_or(|last| snapshot.sequence > last.sequence)
        {
            latest = Some(snapshot);
        }
    }
    let Some(snapshot) = latest else {
        return;
    };

    let removed: Vec<_> = lobby
        .players
        .iter()
        .filter(|(id, _)| **id != player_id.0 && !snapshot.players.iter().any(|p| p.id == **id))
        .map(|(&id, &entity)| (id, entity))
        .collect();
    for (id, entity) in removed {
        commands.entity(entity).despawn();
        lobby.players.remove(&id);
    }

    for player in snapshot.players.iter() {
        if !lobby.players.contains_key(&player.id) && player.id != player_id.0 {
            let entity = commands
                .spawn((
                    PlayerVisualState::default(),
                    Transform::default(),
                    Visibility::Inherited,
                    children![player_body_mesh(&mut meshes, &mut materials, player.bot)],
                ))
                .id();
            lobby.players.insert(player.id, entity);
        }
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

    for fired_projectile in snapshot.fired_projectiles.iter() {
        commands.spawn((
            AudioPlayer::new(match fired_projectile.weapon {
                WeaponKind::Rifle | WeaponKind::Smg => weapon_audio.rifle.clone(),
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
    session.last_sequence = snapshot.sequence;
    session.last_received = time.elapsed_secs_f64();
    session.raid = snapshot.raid;
    session.players = snapshot.players;
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
    for (entity, visual) in impact_visuals.iter() {
        if !impacts.iter().any(|impact| impact.id == visual.id) {
            commands.entity(entity).despawn();
        }
    }
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
