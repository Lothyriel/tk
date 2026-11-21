use super::*;
use bevy_renet2::prelude::RenetClient;
use common::replication::shots::*;
use common::{catalog::*, replication::*};
mod kits;
mod udp;

struct Peer {
    id: u64,
    network: RenetClient,
    receiver: ConfigurationReceiver,
    latest: Option<WorldSnapshot>,
    configurations: usize,
    packets: usize,
    shots: ShotReceiver,
    reliable_ids: std::collections::BTreeSet<u64>,
    presented_ids: std::collections::BTreeSet<u64>,
}

impl Peer {
    fn connect(app: &mut App, id: u64) -> Self {
        app.world_mut()
            .resource_mut::<RenetServer>()
            .add_connection(id, false);
        let mut peer = Self::new(id);
        peer.network.set_connected();
        peer
    }

    fn new(id: u64) -> Self {
        Self {
            id,
            network: RenetClient::new(data::renet_config(), false),
            receiver: Default::default(),
            latest: None,
            configurations: 0,
            packets: 0,
            shots: Default::default(),
            reliable_ids: Default::default(),
            presented_ids: Default::default(),
        }
    }

    fn exchange(&mut self, app: &mut App, loss: bool) {
        let dt = Duration::from_secs_f64(1.0 / 128.0);
        self.network.update(dt);
        for packet in self.network.get_packets_to_send() {
            app.world_mut()
                .resource_mut::<RenetServer>()
                .process_packet_from(packet, self.id)
                .unwrap();
        }
        for packet in app
            .world_mut()
            .resource_mut::<RenetServer>()
            .get_packets_to_send(self.id)
            .unwrap()
        {
            self.packets += 1;
            if !loss || !self.packets.is_multiple_of(5) {
                self.network.process_packet(packet);
            }
        }
        self.receive();
    }

    fn receive(&mut self) {
        // Deliberately consume unreliable data before reliable configuration.
        while let Some(bytes) = self.network.receive_message(DefaultChannel::Unreliable) {
            self.receiver.snapshot(data::decode(&bytes)).unwrap();
        }
        while let Some(bytes) = self.network.receive_message(CONFIGURATION_CHANNEL) {
            self.configurations += usize::from(matches!(
                ConfigurationMessage::decode(&bytes).unwrap(),
                ConfigurationMessage::Configuration(_)
            ));
            self.receiver
                .message(&bytes, catalog(), catalog_fingerprint(CATALOG_SOURCE))
                .unwrap();
        }
        while let Some(bytes) = self.network.receive_message(SHOT_CHANNEL) {
            if let ShotMessage::Batch { shots, .. } = ShotMessage::decode(&bytes).unwrap() {
                for shot in shots {
                    assert!(
                        self.reliable_ids.insert(shot.id),
                        "Renet reliable message replay"
                    );
                }
            }
            self.shots.message(&bytes).unwrap();
        }
        if let Some(snapshot) = self.receiver.take_ready().unwrap() {
            for shot in self.shots.snapshot(
                &snapshot.fired_projectiles,
                snapshot.simulation_time,
                snapshot.raid.phase == RaidPhase::Active,
            ) {
                assert!(
                    self.presented_ids.insert(shot.id),
                    "snapshot replayed an effect"
                );
            }
            self.latest = Some(snapshot);
        }
        if let Some(snapshot) = &self.latest {
            for shot in self.shots.drain(
                snapshot.raid.raid_id,
                snapshot.simulation_time,
                snapshot.raid.phase == RaidPhase::Active,
            ) {
                assert!(
                    self.presented_ids.insert(shot.id),
                    "cross-channel replayed an effect"
                );
            }
        }
    }
}

fn inject_shots(app: &mut App, count: u64) -> std::ops::Range<u64> {
    let now = app.world().resource::<Time>().elapsed_secs();
    let mut state = app.world_mut().resource_mut::<WorldState>();
    let start = state.next_projectile_id;
    for id in start..start + count {
        state.fired_projectiles.push(FiredProjectileData {
            id,
            weapon: WeaponId::M4A1,
            shooter: 7,
            time: now,
            muzzle: [0.0; 3],
            direction: [0.0, 0.0, -1.0],
            ejection: None,
            audio_signature: 100,
        });
    }
    state.next_projectile_id += count;
    start..start + count
}

#[test]
fn shots_production_overflow_is_delivered_reliably_and_late_join_never_replays_history() {
    let (mut app, _, _) = physical_fixture();
    let mut first = Peer::connect(&mut app, 7);
    pump(&mut app, &mut [&mut first], 32, false);
    let ids = inject_shots(&mut app, 2000);
    pump(&mut app, &mut [&mut first], 8, false);
    assert!(app.world().resource::<WorldState>().fired_projectiles.len() == 2000);
    let snapshot = first.latest.as_ref().unwrap();
    assert!(
        snapshot.fired_projectiles.len() < 2000,
        "exercise the actual overflow path"
    );
    assert!(data::encode(snapshot).len() < MAX_SNAPSHOT_BYTES);
    let mut late = Peer::connect(&mut app, 9);
    pump(&mut app, &mut [&mut first, &mut late], 96, true);
    assert_eq!(
        first.reliable_ids,
        ids.clone().collect(),
        "every displaced event reaches the peer"
    );
    assert!(
        late.reliable_ids.is_empty(),
        "late join does not receive historical batches"
    );
    assert!(
        late.presented_ids.is_empty(),
        "fast snapshot history is below the bootstrap floor"
    );
    let new = inject_shots(&mut app, 1);
    pump(&mut app, &mut [&mut first, &mut late], 48, true);
    assert_eq!(late.reliable_ids, new.clone().collect());
    assert!(first.reliable_ids.contains(&new.start));
    assert!(
        first
            .presented_ids
            .iter()
            .all(|id| ids.contains(id) || new.contains(id))
    );
}

#[test]
fn shots_production_backpressure_disconnects_instead_of_silently_losing_events() {
    let (mut app, _, _) = physical_fixture();
    let mut peer = Peer::connect(&mut app, 7);
    pump(&mut app, &mut [&mut peer], 32, false);
    // Fill the reliable send queue without extracting/acknowledging packets.
    {
        let mut server = app.world_mut().resource_mut::<RenetServer>();
        let available = server.channel_available_memory(7, SHOT_CHANNEL);
        server.send_message(7, SHOT_CHANNEL, vec![0; available - 1]);
    }
    inject_shots(&mut app, 1);
    steps(&mut app, 5);
    let server = app.world().resource::<RenetServer>();
    assert!(!server.is_connected(7));
    assert!(server.disconnect_reason(7).is_some());
}

fn pump(app: &mut App, peers: &mut [&mut Peer], ticks: usize, loss: bool) {
    for _ in 0..ticks {
        app.world_mut()
            .resource_mut::<RenetServer>()
            .update(Duration::from_secs_f64(1.0 / 128.0));
        steps(app, 1);
        for peer in peers.iter_mut() {
            peer.exchange(app, loss);
        }
    }
}

#[test]
fn replication_production_renet_bootstrap_commands_loss_late_join_and_epoch_reset() {
    let mut app = app();
    let actor = add_player(&mut app, 7, Loadout::default());
    let mut first = Peer::connect(&mut app, 7);
    pump(&mut app, &mut [&mut first], 64, true);
    let snapshot = first
        .latest
        .as_ref()
        .expect("reliable bootstrap eventually unblocks snapshots");
    assert_eq!(snapshot.players.len(), 1);
    assert_eq!(first.configurations, 2, "both owned slots published once");
    let instance = app
        .world()
        .get::<Arsenal>(actor)
        .unwrap()
        .current()
        .instance();
    let mut configuration = instance.configuration.clone();
    configuration.components[0].node = 30;
    let command = ConfigurationCommand {
        epoch: snapshot.configuration_epoch,
        request: 1,
        slot: EquipmentSlot::Primary,
        instance: instance.id,
        expected_revision: 0,
        weapon: configuration,
        reserves: None,
    };
    let mechanical_before = data::encode(&instance.mechanics);
    first
        .network
        .send_message(CONFIGURATION_CHANNEL, data::encode(&command));
    first
        .network
        .send_message(CONFIGURATION_CHANNEL, data::encode(&command));
    // Configuration and continuous input use independent real channels.
    first.network.send_message(
        DefaultChannel::ReliableOrdered,
        data::encode(&ClientInput {
            camera: CameraInput {
                yaw: 0.3,
                ..default()
            },
            ..default()
        }),
    );
    pump(&mut app, &mut [&mut first], 64, true);
    assert_eq!(
        first.receiver.last_result.as_ref().unwrap().outcome,
        CommandOutcome::Applied
    );
    assert_eq!(
        app.world()
            .get::<Arsenal>(actor)
            .unwrap()
            .current()
            .instance()
            .revision,
        1
    );
    assert_eq!(
        app.world().get::<ClientInput>(actor).unwrap().camera.yaw,
        0.3
    );
    assert_eq!(
        data::encode(
            app.world()
                .get::<Arsenal>(actor)
                .unwrap()
                .current()
                .mechanics()
        ),
        mechanical_before
    );
    assert_eq!(
        first.configurations, 3,
        "duplicates do not publish new revisions"
    );
    let current = first.latest.as_ref().unwrap();
    let cached = first
        .receiver
        .cache
        .get(
            current.configuration_epoch,
            current.players[0].configuration,
        )
        .unwrap();
    assert_eq!(cached.packet.revision, 1);
    assert_eq!(
        cached
            .packet
            .weapon
            .components
            .iter()
            .find(|n| n.definition.0 == 101)
            .unwrap()
            .node,
        30
    );

    let mut late = Peer::connect(&mut app, 9);
    pump(&mut app, &mut [&mut first, &mut late], 64, true);
    let snapshot = late.latest.as_ref().unwrap();
    assert_eq!(snapshot.players[0].weapon_instance, command.instance);
    assert!(
        late.receiver
            .cache
            .matches(snapshot.configuration_epoch, &snapshot.players[0])
    );
    assert_eq!(
        late.configurations, 3,
        "late join receives complete bounded configuration history"
    );
    assert_eq!(first.configurations, 3, "snapshots do not resend trees");

    // A new raid clears incompatible configurations even though the peer stays.
    app.world_mut().resource_mut::<RaidState>().raid_id += 1;
    let old_epoch = first.receiver.cache.epoch();
    first.network.send_message(
        CONFIGURATION_CHANNEL,
        data::encode(&ConfigurationCommand {
            request: 2,
            ..command
        }),
    );
    pump(&mut app, &mut [&mut first, &mut late], 64, true);
    assert!(first.receiver.cache.epoch() > old_epoch);
    assert!(first.receiver.cache.get(old_epoch, 1).is_none());
    let snapshot = first.latest.as_ref().unwrap();
    assert_eq!(snapshot.configuration_epoch, first.receiver.cache.epoch());
    assert!(
        first
            .receiver
            .cache
            .matches(snapshot.configuration_epoch, &snapshot.players[0])
    );
    assert_eq!(
        app.world()
            .get::<Arsenal>(actor)
            .unwrap()
            .current()
            .instance()
            .revision,
        1
    );
}
