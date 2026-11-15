use super::*;
use bevy_renet2::prelude::RenetClient;
use common::{catalog::*, replication::*};

struct Peer {
    id: u64,
    network: RenetClient,
    receiver: ConfigurationReceiver,
    latest: Option<WorldSnapshot>,
    configurations: usize,
    packets: usize,
}

impl Peer {
    fn connect(app: &mut App, id: u64) -> Self {
        app.world_mut()
            .resource_mut::<RenetServer>()
            .add_connection(id, false);
        let mut network = RenetClient::new(data::renet_config(), false);
        network.set_connected();
        Self {
            id,
            network,
            receiver: Default::default(),
            latest: None,
            configurations: 0,
            packets: 0,
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
        if let Some(snapshot) = self.receiver.take_ready().unwrap() {
            self.latest = Some(snapshot);
        }
    }
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
    configuration.components[0].node = 2;
    let command = ConfigurationCommand {
        epoch: snapshot.configuration_epoch,
        request: 1,
        slot: EquipmentSlot::Primary,
        instance: instance.id,
        expected_revision: 0,
        weapon: configuration,
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
    assert_eq!(cached.packet.weapon.components[0].node, 2);

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
