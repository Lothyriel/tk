use bevy::prelude::*;
use bevy_renet2::prelude::{DefaultChannel, RenetClient, client_connected};
use common::{
    ClientData, ImpactMarkData, Lobby, PlayerId, PlayerVisualState, ProjectileData, ServerMessage,
    WorldSnapshot, data,
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
    pub physics_pieces: Vec<common::PhysicsPieceData>,
    pub simulation_time: f32,
    pub shots: Vec<common::FiredProjectileData>,
    pub casing_shots: Vec<common::FiredProjectileData>,
    pub last_shot: Option<u64>,
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

#[derive(Resource)]
struct SnapshotLoss {
    every: u64,
    dropped: u64,
}

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ClientSession>()
            .insert_resource(SnapshotLoss {
                every: std::env::var("TK_DROP_SNAPSHOT_EVERY")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
                dropped: 0,
            })
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
    mut session: ResMut<ClientSession>,
    time: Res<Time<Real>>,
    mut loss: ResMut<SnapshotLoss>,
) {
    let mut latest: Option<WorldSnapshot> = None;
    while let Some(message) = client.receive_message(DefaultChannel::Unreliable) {
        let Ok(snapshot) = data::try_decode::<WorldSnapshot>(&message) else {
            continue;
        };
        if loss.every > 0 && snapshot.sequence % loss.every == 0 {
            loss.dropped += 1;
            if loss.dropped % 60 == 1 {
                info!(
                    "SNAPSHOT LOSS dropped={} sequence={}",
                    loss.dropped, snapshot.sequence
                );
            }
            continue;
        }
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
    let new_raid = session.raid.raid_id != snapshot.raid.raid_id
        || session.raid.phase != snapshot.raid.phase && snapshot.raid.phase == RaidPhase::Lobby;
    if new_raid {
        session.shots.clear();
        session.casing_shots.clear();
    }

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
                    common::Client { id: player.id },
                    PlayerVisualState::default(),
                    Transform::default(),
                    Visibility::Inherited,
                    children![player_body_mesh(&mut meshes, &mut materials, player.bot)],
                ))
                .id();
            lobby.players.insert(player.id, entity);
        }
        if let Some(player_entity) = lobby.players.get(&player.id) {
            if player.id == player_id.0 {
                commands.entity(*player_entity).insert(Transform {
                    translation: player.pos.into(),
                    rotation: Quat::from_rotation_y(player.rot.yaw),
                    ..default()
                });
            } else {
                commands
                    .entity(*player_entity)
                    .insert(crate::render::actors::RemoteTarget {
                        position: player.pos.into(),
                        rotation: Quat::from_rotation_y(player.rot.yaw),
                        snap: new_raid
                            || session
                                .local(player.id)
                                .is_some_and(|previous| previous.pose.mode != player.pose.mode),
                    });
            }
            commands.entity(*player_entity).insert((PlayerVisualState {
                alive: player.alive,
                crouched: player.crouched,
                health: player.health,
                weapon: player.weapon,
                ammo_in_mag: player.ammo_in_mag,
            },));
        }
    }

    let (shots, casing_shots) = fresh_shot_streams(
        &snapshot.fired_projectiles,
        &mut session.last_shot,
        snapshot.simulation_time,
    );
    session.shots = shots;
    session.casing_shots = casing_shots;

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
    session.physics_pieces = snapshot.physics_pieces;
    session.simulation_time = snapshot.simulation_time;
}

/// Consume IDs even when too old to present. Recovery never stacks delayed recoil in one frame.
fn fresh_shot_streams(
    shots: &[common::FiredProjectileData],
    last: &mut Option<u64>,
    now: f32,
) -> (
    Vec<common::FiredProjectileData>,
    Vec<common::FiredProjectileData>,
) {
    let mut coalesced = Vec::new();
    let mut casing_shots = Vec::new();
    for shot in shots {
        if last.is_none_or(|id| shot.id > id) {
            *last = Some(shot.id);
            if now - shot.time < 0.15 {
                casing_shots.push(shot.clone());
                if let Some(index) = coalesced
                    .iter()
                    .position(|s: &common::FiredProjectileData| s.shooter == shot.shooter)
                {
                    coalesced[index] = shot.clone();
                } else {
                    coalesced.push(shot.clone());
                }
            }
        }
    }
    (coalesced, casing_shots)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impact_mark_scale_applies_serialized_diameter() {
        assert_eq!(impact_mark_scale(25), Vec3::new(0.025, 0.025, 0.002));
        assert_eq!(impact_mark_scale(35), Vec3::new(0.035, 0.035, 0.002));
        assert_eq!(impact_mark_scale(50), Vec3::new(0.05, 0.05, 0.002));
    }

    #[test]
    fn snapshot_repetition_and_loss_never_stack_delayed_shots() {
        let shots: Vec<_> = (0..10)
            .map(|id| common::FiredProjectileData {
                id,
                weapon: common::WeaponKind::Smg,
                shooter: 7,
                time: id as f32 * 0.1,
                muzzle: [0.0; 3],
                direction: [0.0, 0.0, -1.0],
            })
            .collect();
        let mut last = None;
        let (fresh, _) = fresh_shot_streams(&shots, &mut last, 1.0);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].id, 9);
        assert_eq!(last, Some(9));
        assert!(fresh_shot_streams(&shots, &mut last, 1.1).0.is_empty());
        let mut last = None;
        assert!(fresh_shot_streams(&shots, &mut last, 2.0).0.is_empty());
        assert_eq!(last, Some(9));
    }

    #[test]
    fn casing_stream_preserves_each_fresh_shot_while_recoil_stays_coalesced() {
        let shots: Vec<_> = (1..=3)
            .map(|id| common::FiredProjectileData {
                id,
                weapon: common::WeaponKind::Rifle,
                shooter: 7,
                time: 1.0 - id as f32 * 0.01,
                muzzle: [0.0; 3],
                direction: [0.0, 0.0, -1.0],
            })
            .collect();
        let mut last = None;
        let (recoil, casings) = fresh_shot_streams(&shots, &mut last, 1.0);
        assert_eq!(recoil.len(), 1);
        assert_eq!(recoil[0].id, 3);
        assert_eq!(
            casings.iter().map(|shot| shot.id).collect::<Vec<_>>(),
            [1, 2, 3]
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
        let mut transform = Transform::from_translation(Vec3::from(impact.pos) + normal * 0.001);
        transform.look_to(normal, Vec3::Y);
        transform.scale = impact_mark_scale(impact.size_mm);

        commands.spawn((
            ImpactMarkVisual { id: impact.id },
            Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
            MeshMaterial3d(materials.add(Color::srgb(0.08, 0.08, 0.08))),
            transform,
        ));
    }
}

fn impact_mark_scale(size_mm: u8) -> Vec3 {
    let diameter = size_mm as f32 / 1000.0;
    Vec3::new(diameter, diameter, 0.002)
}
