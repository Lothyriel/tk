use bevy::prelude::*;
use bevy_renet2::prelude::{DefaultChannel, RenetClient, client_connected};
use common::{
    ClientData, ImpactMarkData, Lobby, PlayerId, PlayerVisualState, ProjectileData, ServerMessage,
    WorldSnapshot, data,
};

use crate::ClientSystems;
use crate::render::{ImpactMarkVisual, ProjectileVisual, player_body_mesh};
use common::raid::{RaidPhase, RaidState, RaidStatus};

mod motion;

#[derive(Debug, Default, Resource)]
pub(crate) struct ClientSession {
    pub configurations: common::replication::ConfigurationReceiver,
    pub local_motion: motion::LocalMotion,
    pub destroyed_cells: Vec<u16>,
    pub raid: RaidState,
    pub players: Vec<ClientData>,
    pub last_sequence: u64,
    pub last_received: f64,
    pub physics_pieces: Vec<common::PhysicsPieceData>,
    pub simulation_time: f32,
    pub shots: Vec<common::FiredProjectileData>,
    pub casing_shots: Vec<common::FiredProjectileData>,
    pub shot_receiver: common::replication::shots::ShotReceiver,
    pub pending_snapshot_received: f64,
    pub pending_snapshot_sequence: u64,
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
            )
            .add_systems(
                Update,
                motion::present_local_motion.in_set(ClientSystems::Present),
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
    while let Some(message) = client.receive_message(common::replication::shots::SHOT_CHANNEL) {
        if let Err(error) = session.shot_receiver.message(&message) {
            warn!("Rejected shot stream: {error:?}");
            // A bounded receiver cannot silently discard a reliable event.
            client.disconnect();
            return;
        }
    }
    while let Some(message) = client.receive_message(common::replication::CONFIGURATION_CHANNEL) {
        if let Err(error) = session.configurations.message(
            &message,
            common::catalog::catalog(),
            common::replication::catalog_fingerprint(common::catalog::CATALOG_SOURCE),
        ) {
            warn!("Rejected weapon configuration: {error:?}");
        }
    }
    while let Some(message) = client.receive_message(DefaultChannel::Unreliable) {
        let Ok(snapshot) = data::try_decode::<WorldSnapshot>(&message) else {
            continue;
        };
        if !snapshot.catalog_valid() {
            warn!("Rejected snapshot with unknown or inconsistent catalog identity");
            continue;
        }
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
        let sequence = snapshot.sequence;
        if let Err(error) = session.configurations.snapshot(snapshot) {
            warn!("Rejected weapon configuration reference: {error:?}");
        } else if sequence > session.pending_snapshot_sequence {
            session.pending_snapshot_sequence = sequence;
            session.pending_snapshot_received = time.elapsed_secs_f64();
        }
    }
    let snapshot = match session.configurations.take_ready() {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => {
            present_shots(&mut session, time.elapsed_secs_f64(), Vec::new());
            return;
        }
        Err(error) => {
            warn!("Rejected inconsistent weapon configuration reference: {error:?}");
            return;
        }
    };
    if snapshot.sequence <= session.last_sequence {
        return;
    }
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
        if player.id == player_id.0 {
            let reset = new_raid
                || session.local(player.id).is_some_and(|previous| {
                    previous.pose.mode != player.pose.mode
                        || previous.alive != player.alive
                        || previous.participant.status != player.participant.status
                });
            session.local_motion.record(
                snapshot.simulation_time,
                player.pos.into(),
                time.elapsed_secs_f64(),
                reset,
            );
        }
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
            if player.id != player_id.0 {
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

    let age = (time.elapsed_secs_f64() - session.pending_snapshot_received).max(0.0) as f32;
    let fresh = session.shot_receiver.snapshot(
        &snapshot.fired_projectiles,
        snapshot.simulation_time + age,
        snapshot.raid.phase == RaidPhase::Active && age < 0.5,
    );

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
    session.destroyed_cells = common::map::destroyed_ids(&snapshot.destroyed_cells);
    session.last_sequence = snapshot.sequence;
    session.last_received = session.pending_snapshot_received;
    session.raid = snapshot.raid;
    session.players = snapshot.players;
    session.physics_pieces = snapshot.physics_pieces;
    session.simulation_time = snapshot.simulation_time;
    present_shots(&mut session, time.elapsed_secs_f64(), fresh);
}

fn present_shots(
    session: &mut ClientSession,
    received_at: f64,
    mut fresh: Vec<common::FiredProjectileData>,
) {
    let age = (received_at - session.last_received).max(0.0) as f32;
    if session.last_sequence > 0 {
        fresh.extend(session.shot_receiver.drain(
            session.raid.raid_id,
            session.simulation_time + age,
            session.raid.phase == RaidPhase::Active && age < 0.5,
        ));
    }
    (session.shots, session.casing_shots) = shot_streams(fresh);
}

/// Recoil is coalesced to the newest ID per shooter; every fresh casing remains.
fn shot_streams(
    mut shots: Vec<common::FiredProjectileData>,
) -> (
    Vec<common::FiredProjectileData>,
    Vec<common::FiredProjectileData>,
) {
    let mut coalesced = Vec::new();
    shots.sort_by_key(|shot| shot.id);
    for shot in &shots {
        if let Some(index) = coalesced
            .iter()
            .position(|s: &common::FiredProjectileData| s.shooter == shot.shooter)
        {
            coalesced[index] = shot.clone();
        } else {
            coalesced.push(shot.clone());
        }
    }
    (coalesced, shots)
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
                weapon: common::WeaponId::MP5,
                shooter: 7,
                time: id as f32 * 0.1,
                muzzle: [0.0; 3],
                direction: [0.0, 0.0, -1.0],
            })
            .collect();
        use common::replication::shots::*;
        let mut receiver = ShotReceiver::default();
        receiver
            .message(&data::encode(&ShotMessage::Bootstrap { first_id: 0 }))
            .unwrap();
        let (fresh, _) = shot_streams(receiver.snapshot(&shots, 1.0, true));
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].id, 9);
        assert!(receiver.snapshot(&shots, 1.1, true).is_empty());
        let mut receiver = ShotReceiver::default();
        receiver
            .message(&data::encode(&ShotMessage::Bootstrap { first_id: 0 }))
            .unwrap();
        assert!(receiver.snapshot(&shots, 2.0, true).is_empty());
        assert_eq!(receiver.stale, 10);
    }

    #[test]
    fn casing_stream_preserves_each_fresh_shot_while_recoil_stays_coalesced() {
        let shots: Vec<_> = (1..=3)
            .map(|id| common::FiredProjectileData {
                id,
                weapon: common::WeaponId::M4A1,
                shooter: 7,
                time: 1.0 - id as f32 * 0.01,
                muzzle: [0.0; 3],
                direction: [0.0, 0.0, -1.0],
            })
            .collect();
        use common::replication::shots::*;
        let mut receiver = ShotReceiver::default();
        receiver
            .message(&data::encode(&ShotMessage::Bootstrap { first_id: 0 }))
            .unwrap();
        let (recoil, casings) = shot_streams(receiver.snapshot(&shots, 1.0, true));
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

#[cfg(test)]
mod barrier_snapshot_tests {
    use super::*;

    #[test]
    fn configuration_receive_unblocks_pending_snapshot_without_a_new_snapshot() {
        use common::{catalog::*, replication::*};
        let mut app = App::new();
        let mut peer = RenetClient::new(data::renet_config(), false);
        peer.set_connected();
        app.add_plugins(MinimalPlugins)
            .insert_resource(peer)
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Lobby>()
            .init_resource::<ClientSession>()
            .insert_resource(PlayerId(7))
            .insert_resource(SnapshotLoss {
                every: 0,
                dropped: 0,
            })
            .add_systems(Update, recv_players_pos);
        let mut server = bevy_renet2::prelude::RenetServer::new(data::renet_config());
        server.add_connection(7, false);
        let fingerprint = catalog_fingerprint(CATALOG_SOURCE);
        let mut publisher = ConfigurationPublisher::new(1, fingerprint).unwrap();
        let mut weapon = EquippedWeapon::new(
            catalog(),
            common::WeaponInstanceId(41),
            catalog().configuration(common::WeaponId::M4A1).unwrap(),
        )
        .unwrap();
        assert!(weapon.fire());
        assert!(weapon.mechanics_mut().start_reload(2.4));
        weapon.advance(0.7); // chamber retained, magazine already removed
        let handle = publisher
            .register(7, common::EquipmentSlot::Primary, weapon.instance())
            .unwrap();
        let snapshot = WorldSnapshot {
            configuration_epoch: 1,
            sequence: 9,
            simulation_time: 1.0,
            destroyed_cells: Default::default(),
            raid: RaidState {
                phase: RaidPhase::Active,
                raid_id: 1,
                ..Default::default()
            },
            players: vec![ClientData {
                id: 7,
                configuration: handle,
                weapon_instance: weapon.instance().id,
                mechanics: weapon.mechanics().snapshot(),
                ..Default::default()
            }],
            projectiles: vec![],
            impact_marks: vec![],
            fired_projectiles: vec![],
            physics_pieces: vec![],
        };
        server.send_message(7, DefaultChannel::Unreliable, data::encode(&snapshot));
        let deliver = |server: &mut bevy_renet2::prelude::RenetServer, app: &mut App| {
            for packet in server.get_packets_to_send(7).unwrap() {
                app.world_mut()
                    .resource_mut::<RenetClient>()
                    .process_packet(packet);
            }
            app.update();
        };
        deliver(&mut server, &mut app);
        assert!(app.world().resource::<ClientSession>().players.is_empty());
        server.send_message(
            7,
            CONFIGURATION_CHANNEL,
            data::encode(&ConfigurationMessage::Epoch {
                epoch: 1,
                catalog: fingerprint,
            }),
        );
        server.send_message(
            7,
            CONFIGURATION_CHANNEL,
            data::encode(&ConfigurationMessage::Configuration(
                publisher.entries()[0].clone(),
            )),
        );
        deliver(&mut server, &mut app);
        let session = app.world().resource::<ClientSession>();
        assert_eq!(session.last_sequence, 9);
        assert_eq!(session.players.len(), 1);
        assert!(!session.players[0].mechanics.magazine_inserted);
        assert!(session.players[0].mechanics.chambered);
        assert_eq!(
            data::encode(&session.players[0].mechanics),
            data::encode(&weapon.mechanics().snapshot())
        );
        assert!(session.configurations.cache.matches(1, &session.players[0]));
        // The independent reliable stream must also present without a new
        // snapshot, and must clear the per-frame stream on the following frame.
        use common::replication::shots::*;
        server.send_message(
            7,
            SHOT_CHANNEL,
            data::encode(&ShotMessage::Bootstrap { first_id: 2 }),
        );
        server.send_message(
            7,
            SHOT_CHANNEL,
            data::encode(&ShotMessage::Batch {
                raid: 1,
                time: 1.0,
                shots: vec![common::FiredProjectileData {
                    id: 2,
                    shooter: 7,
                    weapon: common::WeaponId::M4A1,
                    time: 1.0,
                    muzzle: [0.0; 3],
                    direction: [0.0, 0.0, -1.0],
                }],
            }),
        );
        deliver(&mut server, &mut app);
        let session = app.world().resource::<ClientSession>();
        assert_eq!(session.last_sequence, 9);
        assert_eq!(
            session
                .casing_shots
                .iter()
                .map(|s| s.id)
                .collect::<Vec<_>>(),
            [2]
        );
        app.update();
        assert!(
            app.world()
                .resource::<ClientSession>()
                .casing_shots
                .is_empty()
        );
        // A configuration delayed beyond the presentation window must not
        // rejuvenate either the snapshot or its reliable shot copy.
        let mut delayed = snapshot;
        delayed.sequence = 10;
        delayed.configuration_epoch = 2;
        delayed.simulation_time = 10.0;
        delayed.fired_projectiles = vec![common::FiredProjectileData {
            id: 3,
            shooter: 7,
            weapon: common::WeaponId::M4A1,
            time: 10.0,
            muzzle: [0.0; 3],
            direction: [0.0, 0.0, -1.0],
        }];
        server.send_message(7, DefaultChannel::Unreliable, data::encode(&delayed));
        server.send_message(
            7,
            SHOT_CHANNEL,
            data::encode(&ShotMessage::Batch {
                raid: 1,
                time: 10.0,
                shots: delayed.fired_projectiles.clone(),
            }),
        );
        deliver(&mut server, &mut app);
        let received_at = app
            .world()
            .resource::<ClientSession>()
            .pending_snapshot_received;
        assert_eq!(app.world().resource::<ClientSession>().last_sequence, 9);
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_millis(600),
        ));
        app.update();
        let mut configuration = publisher.entries()[0].clone();
        configuration.epoch = 2;
        server.send_message(
            7,
            CONFIGURATION_CHANNEL,
            data::encode(&ConfigurationMessage::Epoch {
                epoch: 2,
                catalog: fingerprint,
            }),
        );
        server.send_message(
            7,
            CONFIGURATION_CHANNEL,
            data::encode(&ConfigurationMessage::Configuration(configuration)),
        );
        deliver(&mut server, &mut app);
        let session = app.world().resource::<ClientSession>();
        assert_eq!(session.last_sequence, 10);
        assert_eq!(session.last_received, received_at);
        assert!(session.shots.is_empty() && session.casing_shots.is_empty());
        assert_eq!(
            session.shot_receiver.stale, 1,
            "both copies consume one expired event"
        );
    }

    #[test]
    fn voxel_barriers_receive_replaces_full_state_and_rejects_old_snapshots() {
        let mut app = App::new();
        let mut peer = RenetClient::new(data::renet_config(), false);
        peer.set_connected();
        app.add_plugins(MinimalPlugins)
            .insert_resource(peer)
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Lobby>()
            .init_resource::<ClientSession>()
            .insert_resource(PlayerId(999))
            .insert_resource(SnapshotLoss {
                every: 0,
                dropped: 0,
            })
            .add_systems(Update, recv_players_pos);
        let mut server = bevy_renet2::prelude::RenetServer::new(data::renet_config());
        server.add_connection(999, false);
        server.send_message(
            999,
            common::replication::CONFIGURATION_CHANNEL,
            data::encode(&common::replication::ConfigurationMessage::Epoch {
                epoch: 1,
                catalog: common::replication::catalog_fingerprint(common::catalog::CATALOG_SOURCE),
            }),
        );
        for (sequence, destroyed, expected) in [
            (3, vec![2, 7], vec![2, 7]),
            (2, vec![], vec![2, 7]),
            (4, vec![], vec![]),
        ] {
            let snapshot = WorldSnapshot {
                configuration_epoch: 1,
                sequence,
                destroyed_cells: common::map::destroyed_mask(&destroyed),
                raid: default(),
                players: vec![],
                projectiles: vec![],
                impact_marks: vec![],
                fired_projectiles: vec![],
                physics_pieces: vec![],
                simulation_time: 0.0,
            };
            server.broadcast_message(DefaultChannel::Unreliable, data::encode(&snapshot));
            for packet in server.get_packets_to_send(999).unwrap() {
                app.world_mut()
                    .resource_mut::<RenetClient>()
                    .process_packet(packet);
            }
            app.update();
            assert_eq!(
                app.world().resource::<ClientSession>().destroyed_cells,
                expected
            );
        }
    }
}
