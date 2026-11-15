//! Explicit real-socket acceptance. The default unit suite remains headless and
//! socket-free; invoke this ignored gate with --ignored --nocapture.
use super::*;
use bevy_renet2::netcode::{
    ClientAuthentication, ClientSocket, NativeSocket, NetcodeClientTransport,
    NetcodeServerTransport, NetcodeTransportError, ServerAuthentication, ServerSetupConfig,
};
use std::{
    net::{SocketAddr, UdpSocket},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

/// Real kernel UDP in both directions. Discard every fifth datagram before the
/// netcode layer; encryption, handshakes, fragmentation and retries remain real.
#[derive(Debug)]
struct LossSocket {
    socket: NativeSocket,
    loss: bool,
    sent: usize,
    received: usize,
    dropped: Arc<AtomicUsize>,
}

impl ClientSocket for LossSocket {
    fn is_encrypted(&self) -> bool {
        false
    }
    fn is_reliable(&self) -> bool {
        false
    }
    fn addr(&self) -> std::io::Result<SocketAddr> {
        ClientSocket::addr(&self.socket)
    }
    fn is_closed(&mut self) -> bool {
        false
    }
    fn close(&mut self) {
        ClientSocket::close(&mut self.socket);
    }
    fn preupdate(&mut self) {
        ClientSocket::preupdate(&mut self.socket);
    }
    fn postupdate(&mut self) {
        ClientSocket::postupdate(&mut self.socket);
    }
    fn try_recv(&mut self, buffer: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        loop {
            let packet = ClientSocket::try_recv(&mut self.socket, buffer)?;
            self.received += 1;
            if !self.loss || !self.received.is_multiple_of(5) {
                return Ok(packet);
            }
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
    fn send(&mut self, addr: SocketAddr, packet: &[u8]) -> Result<(), NetcodeTransportError> {
        self.sent += 1;
        if self.loss && self.sent.is_multiple_of(5) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
        ClientSocket::send(&mut self.socket, addr, packet)
    }
}

struct UdpPeer {
    peer: Peer,
    transport: NetcodeClientTransport,
    input: ClientInput,
    dropped: Arc<AtomicUsize>,
}

impl UdpPeer {
    fn new(id: u64, server_addr: SocketAddr, now: Duration, loss: bool) -> Self {
        let dropped = Arc::new(AtomicUsize::new(0));
        let socket = LossSocket {
            socket: NativeSocket::new(UdpSocket::bind("127.0.0.1:0").unwrap()).unwrap(),
            loss,
            sent: 0,
            received: 0,
            dropped: dropped.clone(),
        };
        let transport = NetcodeClientTransport::new(
            now,
            ClientAuthentication::Unsecure {
                client_id: id,
                protocol_id: PROTOCOL_ID,
                socket_id: 0,
                server_addr,
                user_data: None,
            },
            socket,
        )
        .unwrap();
        Self {
            peer: Peer::new(id),
            transport,
            input: ClientInput::default(),
            dropped,
        }
    }
}

struct Harness {
    app: App,
    transport: NetcodeServerTransport,
    address: SocketAddr,
    now: Duration,
}

impl Harness {
    fn new() -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = socket.local_addr().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        let transport = NetcodeServerTransport::new(
            ServerSetupConfig {
                current_time: now,
                max_clients: 32,
                max_token_lifetime: 30,
                protocol_id: PROTOCOL_ID,
                socket_addresses: vec![vec![address]],
                authentication: ServerAuthentication::Unsecure,
            },
            NativeSocket::new(socket).unwrap(),
        )
        .unwrap();
        Self {
            app: app(),
            transport,
            address,
            now,
        }
    }

    fn step(&mut self, peers: &mut [&mut UdpPeer]) {
        let dt = Duration::from_secs_f64(1.0 / 128.0);
        self.now += dt;
        for peer in peers.iter_mut() {
            peer.peer.network.update(dt);
            peer.transport.update(dt, &mut peer.peer.network).unwrap();
            peer.peer.receive();
            if peer.peer.network.is_connected() {
                peer.peer
                    .network
                    .send_message(DefaultChannel::ReliableOrdered, data::encode(&peer.input));
            }
            peer.transport.send_packets(&mut peer.peer.network).unwrap();
        }
        let mut events = Vec::new();
        {
            let mut server = self.app.world_mut().resource_mut::<RenetServer>();
            server.update(dt);
            self.transport.update(dt, &mut server).unwrap();
            while let Some(event) = server.get_event() {
                events.push(event);
            }
        }
        for event in events {
            self.app.world_mut().write_message(event);
        }
        steps(&mut self.app, 1);
        self.transport
            .send_packets(&mut self.app.world_mut().resource_mut::<RenetServer>());
    }

    fn pump(&mut self, peers: &mut [&mut UdpPeer], ticks: usize) {
        for _ in 0..ticks {
            self.step(peers);
        }
    }
}

#[test]
#[ignore = "real UDP acceptance: run explicitly with --ignored --nocapture"]
fn replication_udp_solo_dual_loss_and_late_join() {
    for loss in [false, true] {
        let mut harness = Harness::new();
        let mut first = UdpPeer::new(7, harness.address, harness.now, loss);
        if loss {
            first.input.loadout.primary = WeaponId::MP5;
        }
        harness.pump(&mut [&mut first], 192);
        assert!(first.transport.is_connected());
        let snapshot = first.peer.latest.as_ref().expect("solo UDP bootstrap");
        let local = snapshot.players.iter().find(|p| p.id == 7).unwrap();
        assert!(
            first
                .peer
                .receiver
                .cache
                .matches(snapshot.configuration_epoch, local)
        );
        let mut configuration = catalog().configuration(local.weapon_id).unwrap();
        configuration.components[0].node = 30;
        let request = ConfigurationCommand {
            epoch: snapshot.configuration_epoch,
            request: 1,
            slot: EquipmentSlot::Primary,
            instance: local.weapon_instance,
            expected_revision: 0,
            weapon: configuration,
            reserves: None,
        };
        for _ in 0..2 {
            first
                .peer
                .network
                .send_message(CONFIGURATION_CHANNEL, data::encode(&request));
        }
        first.input.camera.yaw = 0.3;
        harness.pump(&mut [&mut first], 96);
        assert_eq!(
            first.peer.receiver.last_result.as_ref().unwrap().outcome,
            CommandOutcome::Applied
        );
        let entity = harness.app.world().resource::<Lobby>().players[&7];
        assert_eq!(
            harness
                .app
                .world()
                .get::<Arsenal>(entity)
                .unwrap()
                .current()
                .instance()
                .revision,
            1
        );
        assert_eq!(
            harness
                .app
                .world()
                .get::<ClientInput>(entity)
                .unwrap()
                .camera
                .yaw,
            0.3
        );

        // Ready and fire through ordinary client packets, not injected events.
        first.input.lobby_sequence = 1;
        for _ in 0..600 {
            harness.step(&mut [&mut first]);
            if first
                .peer
                .latest
                .as_ref()
                .is_some_and(|s| s.raid.phase == RaidPhase::Active)
            {
                break;
            }
        }
        assert_eq!(
            harness.app.world().resource::<RaidState>().phase,
            RaidPhase::Active
        );
        first.input.camera.pitch = 0.8; // clear sky, no target needed
        first.input.fire = true;
        harness.pump(&mut [&mut first], 32);
        first.input.fire = false;
        harness.pump(&mut [&mut first], 24);
        assert!(
            !first.peer.reliable_ids.is_empty(),
            "ordinary fire must reach the reliable UDP stream"
        );
        let before_join = harness
            .app
            .world()
            .resource::<WorldState>()
            .next_projectile_id;

        first.input.reload_sequence = 1;
        let mut late = UdpPeer::new(9, harness.address, harness.now, loss);
        let mut removed_seen = false;
        let mut reload_seen = false;
        for _ in 0..256 {
            harness.step(&mut [&mut first, &mut late]);
            if let Some(snapshot) = &late.peer.latest {
                let actor = snapshot.players.iter().find(|p| p.id == 7).unwrap();
                assert!(
                    late.peer
                        .receiver
                        .cache
                        .matches(snapshot.configuration_epoch, actor)
                );
                reload_seen |= actor.mechanics.kind == Some(ManipulationKind::TacticalReload);
                removed_seen |= !actor.mechanics.magazine_inserted;
            }
        }
        assert!(late.transport.is_connected());
        assert!(
            reload_seen && removed_seen,
            "late join reconstructs current manipulation commits"
        );
        assert!(
            late.peer.presented_ids.iter().all(|id| *id >= before_join),
            "no pre-join effect replay"
        );
        let snapshot = late.peer.latest.as_ref().unwrap();
        let target = snapshot.players.iter().find(|p| p.id == 7).unwrap();
        let forged = ConfigurationCommand {
            epoch: snapshot.configuration_epoch,
            request: 1,
            slot: EquipmentSlot::Primary,
            instance: target.weapon_instance,
            expected_revision: 0,
            weapon: catalog().configuration(target.weapon_id).unwrap(),
            reserves: None,
        };
        late.peer
            .network
            .send_message(CONFIGURATION_CHANNEL, data::encode(&forged));
        harness.pump(&mut [&mut first, &mut late], 64);
        assert_eq!(
            late.peer.receiver.last_result.as_ref().unwrap().outcome,
            CommandOutcome::NotOwned
        );
        let drops = first.dropped.load(Ordering::Relaxed) + late.dropped.load(Ordering::Relaxed);
        assert_eq!(drops > 0, loss);
        println!(
            "TK UDP PASS address={} solo_then_dual=true loss={} dropped={} first_shots={} late_sequence={} reload_seen={} magazine_removed={}",
            harness.address,
            loss,
            drops,
            first.peer.reliable_ids.len(),
            late.peer.latest.as_ref().unwrap().sequence,
            reload_seen,
            removed_seen
        );
    }
}
