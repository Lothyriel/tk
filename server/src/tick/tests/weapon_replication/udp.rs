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
    snapshot_blackout: bool,
    snapshots_dropped: usize,
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
            snapshot_blackout: false,
            snapshots_dropped: 0,
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
            peer.peer.receive_with_snapshot_filter(|| {
                if peer.snapshot_blackout {
                    peer.snapshots_dropped += 1;
                    return false;
                }
                true
            });
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
fn arena_pvp_udp_alternates_lives_rejects_stale_input_and_releases_seat() {
    let mut h = Harness::new();
    let mut a = UdpPeer::new(7, h.address, h.now, false);
    let mut b = UdpPeer::new(8, h.address, h.now, false);
    let mut waiting = UdpPeer::new(9, h.address, h.now, false);
    h.pump(&mut [&mut a, &mut b, &mut waiting], 64);
    for peer in [&mut a, &mut b] {
        peer.input.mode = MatchMode::ArenaPvp;
        peer.input.lobby_sequence = 1;
    }
    h.pump(&mut [&mut a, &mut b, &mut waiting], 32);
    waiting.input.mode = MatchMode::ArenaPvp;
    waiting.input.lobby_sequence = 1;
    h.pump(&mut [&mut a, &mut b, &mut waiting], 390);
    let raid_id = a.peer.latest.as_ref().unwrap().raid.raid_id;
    let get = |h: &Harness, id: u64| h.app.world().resource::<Lobby>().players[&id];
    let first = get(&h, 7);
    let second = get(&h, 8);
    assert_ne!(
        h.app.world().get::<Transform>(first).unwrap().translation,
        h.app.world().get::<Transform>(second).unwrap().translation
    );

    for victim_id in [7, 8] {
        let survivor_id = if victim_id == 7 { 8 } else { 7 };
        let old = get(&h, victim_id);
        let survivor = get(&h, survivor_id);
        let before = h.app.world().get::<BodyHealth>(survivor).unwrap().parts;
        h.app.world_mut().get_mut::<BodyHealth>(old).unwrap().parts[1] = 0.0;
        h.pump(&mut [&mut a, &mut b, &mut waiting], 32);
        for peer in [&a, &b] {
            let snapshot = peer.peer.latest.as_ref().unwrap();
            assert_eq!(snapshot.raid.mode, MatchMode::ArenaPvp);
            assert_eq!(snapshot.raid.phase, RaidPhase::Active);
            assert_eq!(snapshot.raid.raid_id, raid_id);
            assert_eq!(snapshot.players.len(), 3);
            assert!(snapshot.players.iter().all(|p| !p.bot));
            let victim = snapshot.players.iter().find(|p| p.id == victim_id).unwrap();
            assert_eq!(victim.participant.status, RaidStatus::Dead);
            assert!((1..384).contains(&victim.respawn_ticks));
            let extra = snapshot.players.iter().find(|p| p.id == 9).unwrap();
            assert_eq!(extra.participant.status, RaidStatus::Waiting);
            assert!(!extra.alive);
        }
        let victim = if victim_id == 7 { &mut a } else { &mut b };
        victim.input.arena_life = Some((raid_id, 0));
        victim.input.arena_leave = true;
        victim.input.fire = true;
        victim.input.fire_pressed_sequence = 1;
        // Emulate a packet from life 0 arriving after the body replacement.
        // Let the old tagged input queue only after the server has respawned.
        steps(&mut h.app, 360);
        let next = get(&h, victim_id);
        assert_ne!(next, old);
        let before_shots = h.app.world().resource::<WorldState>().next_projectile_id;
        victim.input.lobby_sequence = 2;
        h.pump(&mut [&mut a, &mut b, &mut waiting], 16);
        assert_eq!(
            h.app.world().resource::<WorldState>().next_projectile_id,
            before_shots
        );
        assert_eq!(get(&h, victim_id), next);
        assert_eq!(get(&h, survivor_id), survivor);
        assert_eq!(
            h.app.world().get::<BodyHealth>(survivor).unwrap().parts,
            before
        );
        for peer in [&a, &b] {
            let snapshot = peer.peer.latest.as_ref().unwrap();
            assert_eq!(snapshot.raid.phase, RaidPhase::Active);
            let p = snapshot.players.iter().find(|p| p.id == victim_id).unwrap();
            assert!(p.alive);
            assert_eq!(p.life, 1);
            assert_eq!(p.respawn_ticks, 0);
        }
        let victim = if victim_id == 7 { &mut a } else { &mut b };
        victim.input.arena_life = Some((raid_id, 1));
        victim.input.lobby_sequence = 1;
        victim.input.arena_leave = false;
        victim.input.forward = true;
        // Point away from the other player while verifying fire and control.
        victim.input.camera.yaw = if victim_id == 7 {
            std::f32::consts::PI
        } else {
            0.0
        };
        let start = h.app.world().get::<Transform>(next).unwrap().translation;
        h.pump(&mut [&mut a, &mut b, &mut waiting], 24);
        assert!(h.app.world().resource::<WorldState>().next_projectile_id > before_shots);
        assert!(
            h.app
                .world()
                .get::<Transform>(next)
                .unwrap()
                .translation
                .distance(start)
                > 0.1
        );
        let victim = if victim_id == 7 { &mut a } else { &mut b };
        victim.input.fire = false;
        victim.input.forward = false;
    }
    h.app
        .world_mut()
        .resource_mut::<RenetServer>()
        .disconnect(8);
    h.pump(&mut [&mut a, &mut waiting], 32);
    let snapshot = a.peer.latest.as_ref().unwrap();
    assert_eq!(snapshot.raid.phase, RaidPhase::Lobby);
    assert_eq!(snapshot.players.len(), 2);
    assert!(
        snapshot
            .players
            .iter()
            .all(|p| !p.alive && !p.participant.ready && p.id != 8)
    );
    a.input.arena_life = None;
    a.input.lobby_sequence = 2;
    waiting.input.lobby_sequence = 2;
    h.pump(&mut [&mut a, &mut waiting], 420);
    let snapshot = a.peer.latest.as_ref().unwrap();
    assert_eq!(snapshot.raid.raid_id, raid_id + 1);
    assert!(snapshot.players.iter().all(|p| p.alive && p.life == 0));
}

#[test]
#[ignore = "real UDP acceptance: run explicitly with --ignored --nocapture"]
fn fictional_fault_udp_reconnect_retires_old_actor() {
    let mut harness = Harness::new();
    harness
        .app
        .world_mut()
        .resource_mut::<WorldState>()
        .action_fault_fixture = true;
    let mut first = UdpPeer::new(7, harness.address, harness.now, false);
    let mut observer = UdpPeer::new(8, harness.address, harness.now, true);
    harness.pump(&mut [&mut first, &mut observer], 192);
    first.input.lobby_sequence = 1;
    observer.input.lobby_sequence = 1;
    for _ in 0..640 {
        harness.step(&mut [&mut first, &mut observer]);
        if observer
            .peer
            .latest
            .as_ref()
            .is_some_and(|snapshot| snapshot.raid.phase == RaidPhase::Active)
        {
            break;
        }
    }
    assert_eq!(
        observer.peer.latest.as_ref().unwrap().raid.phase,
        RaidPhase::Active
    );
    let ids = |h: &Harness| {
        let lobby = h.app.world().resource::<Lobby>();
        (
            lobby.players.get(&7).copied(),
            lobby.players.get(&9).copied(),
        )
    };
    let old_entity = ids(&harness).0.unwrap();
    let world = harness.app.world_mut();
    let bots: Vec<_> = world
        .query_filtered::<Entity, With<Bot>>()
        .iter(world)
        .collect();
    for bot in bots {
        world.entity_mut(bot).despawn();
    }
    let next_shot = |h: &Harness| h.app.world().resource::<WorldState>().next_projectile_id;
    let baseline = next_shot(&harness);
    first.input.fire = true;
    first.input.fire_pressed_sequence = 1;
    harness.pump(&mut [&mut first, &mut observer], 48);
    let observed = observer.peer.latest.as_ref().unwrap();
    let old = observed.players.iter().find(|p| p.id == 7).unwrap();
    assert!(old.mechanics.action_unavailable());
    let observer_shots = (
        observer.peer.presented_ids.len(),
        observer.peer.reliable_ids.len(),
    );
    first.transport.disconnect();
    harness.pump(&mut [&mut observer], 64);
    assert!(ids(&harness).0.is_none());
    assert!(harness.app.world().get_entity(old_entity).is_err());

    // The game's reconnect creates a new transport ID and fresh input state.
    let mut fresh = UdpPeer::new(9, harness.address, harness.now, false);
    harness.pump(&mut [&mut fresh, &mut observer], 192);
    assert!(fresh.transport.is_connected());
    assert_ne!(ids(&harness).1.unwrap(), old_entity);
    for peer in [&fresh, &observer] {
        let snapshot = peer.peer.latest.as_ref().unwrap();
        assert_eq!(snapshot.raid.phase, RaidPhase::Active);
        assert!(!snapshot.players.iter().any(|p| p.id == 7));
        let player = snapshot.players.iter().find(|p| p.id == 9).unwrap();
        assert_eq!(player.participant.status, RaidStatus::Waiting);
        assert!(!player.alive && !player.mechanics.action_unavailable());
        assert_eq!(player.mechanics.recovery_ms(), 0);
    }
    assert_eq!(next_shot(&harness), baseline);
    assert_eq!(
        (
            observer.peer.presented_ids.len(),
            observer.peer.reliable_ids.len()
        ),
        observer_shots
    );
    assert!(fresh.peer.reliable_ids.is_empty() && fresh.peer.presented_ids.is_empty());
    let dropped = observer.dropped.load(Ordering::Relaxed);
    assert!(dropped > 0);
    eprintln!("reconnect retired the faulted actor with {dropped} discarded observer datagrams");
}

#[test]
#[ignore = "real UDP acceptance: run explicitly with --ignored --nocapture"]
fn fictional_fault_udp_late_observer_converges_without_replaying_actions() {
    fn actor(peer: &UdpPeer) -> &ClientData {
        peer.peer
            .latest
            .as_ref()
            .expect("accepted snapshot")
            .players
            .iter()
            .find(|actor| actor.id == 7)
            .expect("confirmed actor")
    }

    let mut harness = Harness::new();
    harness
        .app
        .world_mut()
        .resource_mut::<WorldState>()
        .action_fault_fixture = true;
    let mut first = UdpPeer::new(7, harness.address, harness.now, false);
    harness.pump(&mut [&mut first], 192);
    assert!(first.transport.is_connected());
    first.input.lobby_sequence = 1;
    harness.pump(&mut [&mut first], 430);
    assert_eq!(
        first.peer.latest.as_ref().unwrap().raid.phase,
        RaidPhase::Active
    );
    // Keep the real transport and raid schedule, but exclude unrelated bot
    // effects so the shot-stream assertions isolate the fixture.
    let world = harness.app.world_mut();
    let bots: Vec<_> = world
        .query_filtered::<Entity, With<Bot>>()
        .iter(world)
        .collect();
    for bot in bots {
        world.entity_mut(bot).despawn();
    }
    let first_shots = (
        first.peer.presented_ids.len(),
        first.peer.reliable_ids.len(),
    );
    let (instance, loaded, reserve) = {
        let initial = actor(&first);
        assert!(initial.alive && !initial.mechanics.action_unavailable());
        (
            initial.weapon_instance,
            initial.ammo_in_mag,
            initial.reserve_ammo,
        )
    };
    let shot_id = harness
        .app
        .world()
        .resource::<WorldState>()
        .next_projectile_id;
    first.input.fire = true;
    first.input.fire_pressed_sequence = 1;
    harness.pump(&mut [&mut first], 40);
    first.input.fire = false;
    let unavailable = actor(&first).mechanics;
    assert!(unavailable.action_unavailable());
    assert_eq!(unavailable.recovery_ms(), 0);

    let mut late = UdpPeer::new(9, harness.address, harness.now, true);
    harness.pump(&mut [&mut first, &mut late], 230);
    assert!(late.transport.is_connected());
    let late_snapshot = late.peer.latest.as_ref().unwrap();
    assert_eq!(late_snapshot.raid.phase, RaidPhase::Active);
    assert!(
        late.peer
            .receiver
            .cache
            .matches(late_snapshot.configuration_epoch, actor(&late))
    );
    for peer in [&first, &late] {
        let player = actor(peer);
        assert_eq!(player.mechanics, unavailable);
        assert_eq!(
            (
                player.weapon_instance,
                player.ammo_in_mag,
                player.reserve_ammo
            ),
            (instance, loaded, reserve)
        );
    }

    first.input.reload_sequence = 1;
    let mut saw_recovery = false;
    for _ in 0..240 {
        harness.step(&mut [&mut first, &mut late]);
        let observed = actor(&late).mechanics;
        saw_recovery |= observed.action_unavailable() && observed.recovery_ms() > 0;
        if !observed.action_unavailable() && !actor(&first).mechanics.action_unavailable() {
            break;
        }
    }
    assert!(saw_recovery, "late observer missed the confirmed recovery");
    assert_eq!(actor(&first).mechanics, actor(&late).mechanics);
    for peer in [&first, &late] {
        let player = actor(peer);
        assert!(!player.mechanics.action_unavailable());
        assert_eq!(
            (
                player.weapon_instance,
                player.ammo_in_mag,
                player.reserve_ammo
            ),
            (instance, loaded, reserve)
        );
    }
    assert_eq!(
        (
            first.peer.presented_ids.len(),
            first.peer.reliable_ids.len()
        ),
        first_shots
    );
    assert!(late.peer.presented_ids.is_empty() && late.peer.reliable_ids.is_empty());
    assert_eq!(
        harness
            .app
            .world()
            .resource::<WorldState>()
            .next_projectile_id,
        shot_id
    );
    let dropped = late.dropped.load(Ordering::Relaxed);
    assert!(dropped > 0);
    eprintln!("late observer converged after {dropped} discarded loopback datagrams");
}

#[test]
#[ignore = "real UDP acceptance: run explicitly with --ignored --nocapture"]
fn inventory_udp_ground_bootstrap_late_join_and_cleanup() {
    use common::{
        catalog::{
            CompartmentId, ItemInstanceId,
            item_grid::{Placement, Rotation},
            item_inventory::{
                GridLocation, InventoryAction, InventoryOperation, ItemContents, OperationResult,
                VersionedCompartment,
            },
            item_manifest::{S1_SEEDS, item_visuals, planned_items},
        },
        replication::inventory::{CHANNEL, InventoryRequest, InventoryUpdate, UpdateBody},
    };

    let drain = |peer: &mut UdpPeer| {
        std::iter::from_fn(|| peer.peer.network.receive_message(CHANNEL))
            .map(|bytes| InventoryUpdate::decode(&bytes).unwrap())
            .collect::<Vec<_>>()
    };
    let mut harness = Harness::new();
    let mut first = UdpPeer::new(7, harness.address, harness.now, true);
    harness.pump(&mut [&mut first], 192);
    assert_eq!(drain(&mut first).len(), 2);
    first.input.lobby_sequence = 1;
    harness.pump(&mut [&mut first], 512);
    assert_eq!(
        harness.app.world().resource::<RaidState>().phase,
        RaidPhase::Active
    );
    let initial = drain(&mut first);
    assert_eq!(initial.len(), 3);
    assert!(matches!(initial[0].body, UpdateBody::Begin));
    let UpdateBody::Ground(items) = &initial[2].body else {
        panic!("expected public ground list after bootstrap");
    };
    assert_eq!(initial[0].session, initial[2].session);
    assert_eq!(items.len(), S1_SEEDS.len());
    for (index, item) in items.iter().enumerate() {
        assert_eq!(usize::from(item.seed), index);
        assert_eq!(item.id, ItemInstanceId(index as u64 + 1));
        assert!(item.root.iter().all(|x| x.is_finite()));
    }

    let mut late = UdpPeer::new(8, harness.address, harness.now, true);
    harness.pump(&mut [&mut first, &mut late], 192);
    assert!(drain(&mut first).is_empty());
    let joined = drain(&mut late);
    assert_eq!(joined.len(), 3);
    assert_ne!(joined[2].session, initial[2].session);
    assert_eq!(joined[2].body, initial[2].body);

    // Stage the player near the bandage; aim and interact through normal UDP
    // input so the endpoint still has to confirm focus and commit the pickup.
    let item = &items[0];
    let actor = harness.app.world().resource::<Lobby>().players[&7];
    let center = Vec3::from_array(item.root)
        + Vec3::from_array(item_visuals()[S1_SEEDS[0]].collider_center());
    let start = Vec3::new(center.x, 1.02, center.z + 1.0);
    harness
        .app
        .world_mut()
        .get_mut::<Transform>(actor)
        .unwrap()
        .translation = start;
    let ray = center - (start + Vec3::Y * PLAYER_VIEW_HEIGHT);
    first.input.camera.pitch = ray.y.atan2(-ray.z);
    harness.pump(&mut [&mut first, &mut late], 48);
    assert!(
        drain(&mut first)
            .iter()
            .any(|update| update.body == UpdateBody::GroundFocus(Some(item.id)))
    );
    assert!(drain(&mut late).is_empty());

    let epoch = u32::try_from(initial[0].session.raid_epoch).unwrap();
    first.input.interact = true;
    first.input.pickup_focus = Some((epoch - 1, item.id));
    harness.pump(&mut [&mut first, &mut late], 48);
    assert!(drain(&mut first).is_empty(), "stale epoch must not collect");
    first.input.interact = false;
    first.input.pickup_focus = None;
    harness.pump(&mut [&mut first, &mut late], 32);
    first.input.interact = true;
    first.input.pickup_focus = Some((epoch, item.id));
    harness.pump(&mut [&mut first, &mut late], 96);
    let collected = drain(&mut first);
    assert_eq!(collected.len(), 3);
    assert!(matches!(&collected[0].body, UpdateBody::Delta(deltas)
        if deltas.len() == 1 && deltas[0].version.id == CompartmentId(7)
        && deltas[0].items.len() == 1 && deltas[0].items[0].id == item.id
        && deltas[0].items[0].item.as_ref().is_some_and(|entry| entry.quantity == 2)));
    assert_eq!(collected[1].body, UpdateBody::Ground(items[1..].to_vec()));
    assert_eq!(collected[2].body, UpdateBody::GroundFocus(None));
    let public = drain(&mut late);
    assert_eq!(public.len(), 1);
    assert_eq!(public[0].body, collected[1].body);
    assert_eq!(
        harness.app.world().resource::<RaidState>().loot_available,
        [true; 3]
    );

    let displayed = |peer: &UdpPeer| {
        peer.peer
            .latest
            .as_ref()
            .unwrap()
            .players
            .iter()
            .find(|p| p.id == 7)
            .unwrap()
            .participant
            .clone()
    };
    assert_eq!(displayed(&first).carried, 2);
    assert_eq!(displayed(&first).secured, 0);
    assert_eq!(
        harness
            .app
            .world()
            .get::<Participant>(actor)
            .unwrap()
            .carried,
        0
    );

    first.input.interact = false;
    first.input.pickup_focus = None;
    for revision in [0, 1, 1] {
        first.peer.network.send_message(
            CHANNEL,
            InventoryRequest::Drop {
                session: initial[0].session,
                item: item.id,
                revision,
            }
            .encode()
            .unwrap(),
        );
    }
    harness.pump(&mut [&mut first, &mut late], 96);
    let updates = drain(&mut first);
    let removals: Vec<_> = updates
        .iter()
        .filter_map(|update| match &update.body {
            UpdateBody::Delta(deltas) => Some(deltas),
            _ => None,
        })
        .collect();
    assert_eq!(removals.len(), 1);
    assert_eq!(removals[0][0].version.revision, 2);
    assert_eq!(removals[0][0].items[0].id, item.id);
    assert!(removals[0][0].items[0].item.is_none());
    let public = drain(&mut late);
    let trajectory: Vec<_> = public
        .iter()
        .map(|update| {
            let UpdateBody::Ground(dropped) = &update.body else {
                panic!("expected only public ground updates");
            };
            assert_eq!(dropped.len(), items.len());
            assert_eq!((dropped[0].id, dropped[0].seed), (item.id, item.seed));
            &dropped[0]
        })
        .collect();
    assert!((3..=15).contains(&trajectory.len()));
    let dropped = trajectory.last().unwrap();
    assert!(trajectory[0].root[1] - dropped.root[1] > 0.8);
    assert!(
        trajectory
            .windows(2)
            .all(|pair| pair[0].root[1] > pair[1].root[1])
    );
    assert_eq!(displayed(&first).carried, 0);
    let visual = &item_visuals()[S1_SEEDS[0]];
    let center = Vec3::from_array(dropped.root) + Vec3::from_array(visual.collider_center());
    assert!((center.y - visual.collider_half_extents()[1] - 0.1).abs() < 0.01);
    let ray = center - (start + Vec3::Y * PLAYER_VIEW_HEIGHT);
    first.input.camera.pitch = ray.y.atan2(-ray.z);
    harness.pump(&mut [&mut first, &mut late], 32);
    let _ = drain(&mut first);
    first.input.interact = true;
    first.input.pickup_focus = Some((epoch, item.id));
    harness.pump(&mut [&mut first, &mut late], 96);
    assert_eq!(displayed(&first).carried, 2);
    assert!(
        drain(&mut first)
            .iter()
            .any(|update| matches!(&update.body, UpdateBody::Delta(deltas)
        if deltas[0].version.revision == 3 && deltas[0].items[0].id == item.id))
    );
    assert_eq!(drain(&mut late).len(), 1);

    first.input.interact = false;
    first.input.pickup_focus = None;
    let key = initial[0].session;
    let own = CompartmentId(7);
    let operate = |peer: &mut UdpPeer, id, revision, action| {
        peer.peer.network.send_message(
            CHANNEL,
            InventoryRequest::Operate(InventoryOperation {
                session: key,
                id,
                expected: [VersionedCompartment { id: own, revision }; 2],
                action,
            })
            .encode()
            .unwrap(),
        );
    };
    let committed = |peer: &mut UdpPeer, id, revision| {
        let [delta, result]: [InventoryUpdate; 2] = drain(peer).try_into().unwrap();
        assert_eq!((delta.session, result.session), (key, key));
        assert_eq!(
            result.body,
            UpdateBody::Result {
                id,
                result: OperationResult::Committed
            }
        );
        let UpdateBody::Delta(mut changes) = delta.body else {
            panic!("expected private inventory delta");
        };
        assert_eq!(changes.len(), 1);
        let change = changes.pop().unwrap();
        assert_eq!(change.version, VersionedCompartment { id: own, revision });
        change.items
    };
    operate(
        &mut first,
        1,
        3,
        InventoryAction::Split {
            item: item.id,
            quantity: 1,
            destination: GridLocation {
                compartment: own,
                placement: Placement {
                    x: 1,
                    y: 0,
                    rotation: Rotation::Upright,
                },
            },
        },
    );
    harness.pump(&mut [&mut first, &mut late], 160);
    let split = committed(&mut first, 1, 4);
    assert_eq!(split.len(), 2);
    assert!(split.iter().all(|change| {
        change
            .item
            .as_ref()
            .is_some_and(|stack| stack.quantity == 1)
    }));
    let child = split.iter().find(|change| change.id != item.id).unwrap().id;
    let original_half = split
        .iter()
        .find(|change| change.id == item.id)
        .unwrap()
        .item
        .as_ref()
        .unwrap();
    operate(
        &mut first,
        2,
        4,
        InventoryAction::Merge {
            source: item.id,
            target: child,
        },
    );
    harness.pump(&mut [&mut first, &mut late], 160);
    let merged = committed(&mut first, 2, 5);
    assert_eq!(merged.len(), 2);
    assert!(
        merged
            .iter()
            .any(|change| change.id == item.id && change.item.is_none())
    );
    let reunited = merged
        .iter()
        .find(|change| change.id == child)
        .unwrap()
        .item
        .as_ref()
        .unwrap();
    assert_eq!(reunited.definition, "med.bandage");
    assert_eq!(reunited.quantity, 2);
    assert_eq!(reunited.contents, original_half.contents);
    assert_eq!(reunited.condition, original_half.condition);
    assert_eq!(
        u32::from(reunited.quantity) * planned_items()[reunited.definition.as_str()].mass_g,
        70
    );
    assert_eq!(displayed(&first).carried, 2);
    assert!(
        drain(&mut late).is_empty(),
        "private split/merge leaked to the observer"
    );

    // Collect the separate empty magazine as ordinary ground loot. Its
    // contents must not change when extraction credits the inventory.
    let magazine = &items[1];
    assert_eq!(S1_SEEDS[usize::from(magazine.seed)], "ammo.mag_ar30");
    let center = Vec3::from_array(magazine.root)
        + Vec3::from_array(item_visuals()[S1_SEEDS[1]].collider_center());
    let start = Vec3::new(center.x, 1.02, center.z + 1.0);
    harness
        .app
        .world_mut()
        .get_mut::<Transform>(actor)
        .unwrap()
        .translation = start;
    let ray = center - (start + Vec3::Y * PLAYER_VIEW_HEIGHT);
    first.input.camera.pitch = ray.y.atan2(-ray.z);
    harness.pump(&mut [&mut first, &mut late], 48);
    assert!(
        drain(&mut first)
            .iter()
            .any(|update| update.body == UpdateBody::GroundFocus(Some(magazine.id)))
    );
    first.input.interact = true;
    first.input.pickup_focus = Some((epoch, magazine.id));
    harness.pump(&mut [&mut first, &mut late], 96);
    let updates = drain(&mut first);
    let empty_magazine = updates
        .iter()
        .find_map(|update| match &update.body {
            UpdateBody::Delta(deltas) => {
                assert_eq!(update.session, key);
                assert_eq!(deltas.len(), 1);
                assert_eq!(
                    deltas[0].version,
                    VersionedCompartment {
                        id: own,
                        revision: 6
                    }
                );
                assert_eq!(deltas[0].items.len(), 1);
                assert_eq!(deltas[0].items[0].id, magazine.id);
                deltas[0].items[0].item.as_ref()
            }
            _ => None,
        })
        .expect("confirmed magazine pickup");
    assert_eq!(empty_magazine.definition, "ammo.mag_ar30");
    assert_eq!(empty_magazine.quantity, 1);
    assert_eq!(empty_magazine.contents, ItemContents::EmptyMagazine);
    let public = drain(&mut late);
    assert_eq!(public.len(), 1);
    assert_eq!(public[0].body, UpdateBody::Ground(items[2..].to_vec()));

    // Stage the player in the exit but keep the normal hold duration and
    // production credit/debrief transitions. No legacy crate was collected.
    let [x, _, z] = common::map::EXTRACTION_CENTER;
    harness
        .app
        .world_mut()
        .get_mut::<Transform>(actor)
        .unwrap()
        .translation = Vec3::new(x, 1.02, z);
    first.input.interact = false;
    first.input.pickup_focus = None;
    harness.pump(&mut [&mut first, &mut late], 32);
    first.input.interact = true;
    harness.pump(&mut [&mut first, &mut late], 512);
    assert_eq!(displayed(&first).status, RaidStatus::Active);
    assert_eq!(displayed(&first).carried, 3);
    assert_eq!(displayed(&first).secured, 0);
    harness.pump(&mut [&mut first, &mut late], 256);
    assert_eq!(displayed(&first).status, RaidStatus::Extracted);
    assert_eq!(displayed(&first).carried, 0);
    assert_eq!(displayed(&first).secured, 3);
    harness.pump(&mut [&mut first, &mut late], 128);
    assert_eq!(displayed(&first).secured, 3);
    assert_eq!(
        harness.app.world().resource::<RaidState>().phase,
        RaidPhase::Debrief
    );
    for (peer, key) in [
        (&mut first, initial[2].session),
        (&mut late, joined[2].session),
    ] {
        let updates = drain(peer);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].session, key);
        assert_eq!(updates[0].body, UpdateBody::Ground(vec![]));
        assert!(peer.dropped.load(Ordering::Relaxed) > 0);
    }
    let authoritative = harness
        .app
        .world()
        .resource::<WorldState>()
        .inventory
        .fixture_snapshot(own);
    assert_eq!(authoritative.version.revision, 6);
    let mut expected = [
        (child, reunited.clone()),
        (magazine.id, empty_magazine.clone()),
    ];
    expected.sort_unstable_by_key(|(id, _)| *id);
    assert_eq!(authoritative.items.as_slice(), &expected);

    first.input.interact = false;
    first.input.pickup_focus = None;
    first.input.lobby_sequence = 2;
    harness.pump(&mut [&mut first, &mut late], 96);
    assert_eq!(
        harness.app.world().resource::<RaidState>().phase,
        RaidPhase::Lobby
    );
    first.input.lobby_sequence = 3;
    late.input.lobby_sequence = 1;
    harness.pump(&mut [&mut first, &mut late], 512);
    assert_eq!(
        harness.app.world().resource::<RaidState>().phase,
        RaidPhase::Active
    );
    for peer in [&mut first, &mut late] {
        let updates = drain(peer);
        let next = updates
            .iter()
            .find_map(|update| match &update.body {
                UpdateBody::Ground(ground) if ground.len() == S1_SEEDS.len() => {
                    assert_eq!(update.session.raid_epoch, u64::from(epoch) + 1);
                    Some(ground)
                }
                _ => None,
            })
            .expect("fresh ground publication");
        for (old, new) in items.iter().zip(next) {
            assert_eq!(old.seed, new.seed);
            assert!(items.iter().all(|old| old.id != new.id));
        }
    }
}

#[test]
#[ignore = "real UDP acceptance: run explicitly with --ignored --nocapture"]
fn inventory_udp_private_resync_under_loss() {
    use common::{
        catalog::{CompartmentId, item_inventory::SessionKey},
        replication::inventory::{CHANNEL, InventoryRequest, InventoryUpdate, UpdateBody},
    };

    let mut harness = Harness::new();
    let mut first = UdpPeer::new(7, harness.address, harness.now, true);
    let mut second = UdpPeer::new(8, harness.address, harness.now, true);
    let drain = |peer: &mut UdpPeer| {
        std::iter::from_fn(|| peer.peer.network.receive_message(CHANNEL))
            .map(|bytes| InventoryUpdate::decode(&bytes).unwrap())
            .collect::<Vec<_>>()
    };
    let bootstrap = |peer: &mut UdpPeer, own: u64| -> SessionKey {
        assert!(peer.transport.is_connected());
        let updates = drain(peer);
        assert_eq!(updates.len(), 2);
        let session = updates[0].session;
        assert_eq!(updates[0].body, UpdateBody::Begin);
        assert_eq!(updates[1].session, session);
        assert!(matches!(&updates[1].body, UpdateBody::Snapshot(snapshot)
            if snapshot.version.id == CompartmentId(own) && snapshot.items.is_empty()));
        session
    };
    harness.pump(&mut [&mut first, &mut second], 192);
    let key_first = bootstrap(&mut first, 7);
    let key_second = bootstrap(&mut second, 8);
    assert_ne!(key_first, key_second);

    for (peer, session, own, other) in [
        (&mut first, key_first, 7, 8),
        (&mut second, key_second, 8, 7),
    ] {
        for (key, compartment) in [(session, other), (session, own)] {
            let request = InventoryRequest::Resync {
                session: key,
                compartment: CompartmentId(compartment),
            };
            peer.peer
                .network
                .send_message(CHANNEL, request.encode().unwrap());
        }
    }
    first.input.camera.yaw = 0.25;
    harness.pump(&mut [&mut first, &mut second], 160);
    for (peer, session, own, other) in [
        (&mut first, key_first, 7, 8),
        (&mut second, key_second, 8, 7),
    ] {
        let updates = drain(peer);
        assert_eq!(updates.len(), 2);
        assert!(updates.iter().all(|update| update.session == session));
        assert_eq!(updates[0].body, UpdateBody::Closed(CompartmentId(other)));
        assert!(matches!(&updates[1].body, UpdateBody::Snapshot(snapshot)
            if snapshot.version.id == CompartmentId(own) && snapshot.version.revision == 0));
        let dropped = peer.dropped.load(Ordering::Relaxed);
        assert!(dropped > 0);
        println!("TK INVENTORY UDP own={own} dropped={dropped} private_resync=ok");
    }
    let world = harness.app.world();
    let entity = world.resource::<Lobby>().players[&7];
    assert_eq!(world.get::<ClientInput>(entity).unwrap().camera.yaw, 0.25);
}

#[test]
#[ignore = "real UDP acceptance: run explicitly with --ignored --nocapture"]
fn inventory_udp_contested_ground_pickup_with_snapshot_loss_and_disconnect() {
    use common::{
        catalog::{
            item_inventory::{Condition, ItemContents},
            item_manifest::{S1_SEEDS, item_visuals},
        },
        replication::inventory::{CHANNEL, InventoryRequest, InventoryUpdate, UpdateBody},
    };

    let drain = |peer: &mut UdpPeer| {
        std::iter::from_fn(|| peer.peer.network.receive_message(CHANNEL))
            .map(|bytes| InventoryUpdate::decode(&bytes).unwrap())
            .collect::<Vec<_>>()
    };
    let mut harness = Harness::new();
    // Lose snapshots at the receiving channel, not whole datagrams. Both
    // reliable pickup inputs must reach the *same* authoritative tick.
    let mut first = UdpPeer::new(7, harness.address, harness.now, false);
    let mut second = UdpPeer::new(8, harness.address, harness.now, false);
    harness.pump(&mut [&mut first, &mut second], 192);
    for peer in [&mut first, &mut second] {
        drain(peer);
        peer.input.lobby_sequence = 1;
    }
    harness.pump(&mut [&mut first, &mut second], 512);
    let bootstrap = [&mut first, &mut second].map(drain);
    let [first_view, second_view] = &bootstrap;
    let [_, _, ground] = first_view.as_slice() else {
        panic!("expected fresh raid bootstrap, got {first_view:?}");
    };
    let UpdateBody::Ground(items) = &ground.body else {
        panic!("expected public ground list");
    };
    assert_eq!(second_view[2].body, ground.body);
    let item = &items[0];
    let expected_ground = UpdateBody::Ground(items[1..].to_vec());
    let epoch = u32::try_from(ground.session.raid_epoch).unwrap();
    assert!(epoch > 0);
    let center = Vec3::from_array(item.root)
        + Vec3::from_array(item_visuals()[S1_SEEDS[0]].collider_center());
    let actors = [7, 8].map(|id| harness.app.world().resource::<Lobby>().players[&id]);
    for ((peer, entity), dx) in [&mut first, &mut second]
        .into_iter()
        .zip(actors)
        .zip([-0.5, 0.5])
    {
        let world = harness.app.world_mut();
        assert_eq!(
            world.get::<Participant>(entity).unwrap().status,
            RaidStatus::Active
        );
        // Two non-overlapping approaches; focus itself comes from server sight.
        let start = Vec3::new(center.x + dx, 1.02, center.z + 1.05);
        world.get_mut::<Transform>(entity).unwrap().translation = start;
        let ray = center - (start + Vec3::Y * PLAYER_VIEW_HEIGHT);
        peer.input.camera.yaw = (-ray.x).atan2(-ray.z);
        peer.input.camera.pitch = ray.y.atan2(ray.x.hypot(ray.z));
    }
    harness.pump(&mut [&mut first, &mut second], 48);
    let snapshot_sequences = [&first, &second].map(|p| p.peer.latest.as_ref().unwrap().sequence);
    for peer in [&mut first, &mut second] {
        assert!(
            drain(peer)
                .iter()
                .any(|update| update.body == UpdateBody::GroundFocus(Some(item.id)))
        );
        peer.snapshot_blackout = true;
        peer.input.interact = true;
        peer.input.pickup_focus = Some((epoch - 1, item.id));
    }
    harness.pump(&mut [&mut first, &mut second], 16);
    for peer in [&mut first, &mut second] {
        assert!(
            drain(peer).is_empty(),
            "stale epoch changed a ready-to-pick item"
        );
        peer.input.interact = false;
    }
    harness.step(&mut [&mut first, &mut second]);
    for peer in [&mut first, &mut second] {
        peer.input.interact = true;
        peer.input.pickup_focus = Some((epoch, item.id));
    }
    harness.step(&mut [&mut first, &mut second]);
    for actor in actors {
        let input = harness.app.world().get::<ClientInput>(actor).unwrap();
        assert!(
            input.interact && input.pickup_focus == Some((epoch, item.id)),
            "both pickup requests must reach the same tick"
        );
    }
    let world = harness.app.world();
    let state = world.resource::<WorldState>();
    let raid = world.resource::<RaidState>();
    let carried = [7, 8].map(|id| state.inventory.carried(raid, id));
    assert_eq!(
        carried.iter().sum::<u32>(),
        2,
        "exactly one two-unit stack commits in the collision tick"
    );
    harness.pump(&mut [&mut first, &mut second], 32);
    let mut award = None;
    for (index, peer) in [&mut first, &mut second].into_iter().enumerate() {
        assert!(peer.snapshots_dropped > 0);
        assert_eq!(
            peer.peer.latest.as_ref().unwrap().sequence,
            snapshot_sequences[index]
        );
        let mut tombstones = 0;
        let mut focus_clears = 0;
        for update in drain(peer) {
            assert_eq!(update.session, bootstrap[index][0].session);
            match update.body {
                UpdateBody::Delta(mut deltas) => {
                    assert_eq!(deltas.len(), 1);
                    let delta = deltas.pop().unwrap();
                    assert_eq!(delta.version.id, CompartmentId(peer.peer.id));
                    assert_eq!(delta.version.revision, 1);
                    assert_eq!(delta.items.len(), 1);
                    assert_eq!(delta.items[0].id, item.id);
                    let granted = delta.items[0].item.clone().expect("winner gets the item");
                    assert_eq!(granted.definition, S1_SEEDS[0]);
                    assert_eq!(granted.condition, Condition::Intact);
                    assert_eq!(granted.quantity, 2);
                    assert_eq!(granted.contents, ItemContents::Dressing { sealed: true });
                    assert!(
                        award.replace((index, granted)).is_none(),
                        "two pickup commits"
                    );
                }
                UpdateBody::Ground(_) => {
                    assert_eq!(update.body, expected_ground);
                    tombstones += 1;
                }
                UpdateBody::GroundFocus(None) => focus_clears += 1,
                other => panic!("unexpected pickup update: {other:?}"),
            }
        }
        assert_eq!((tombstones, focus_clears), (1, 1));
        peer.snapshot_blackout = false;
    }
    let (winner_index, granted) = award.expect("one pickup must commit");
    assert_eq!(
        carried[winner_index], 2,
        "the private award goes to the authoritative owner"
    );
    let (winner, loser) = if winner_index == 0 {
        (&mut first, &mut second)
    } else {
        (&mut second, &mut first)
    };
    let winner_id = winner.peer.id;
    winner.transport.disconnect();
    loser.input.interact = false;
    harness.pump(&mut [loser], 16);
    assert!(
        harness
            .app
            .world()
            .get_entity(actors[winner_index])
            .is_err()
    );
    let loser_index = 1 - winner_index;
    assert!(loser.peer.latest.as_ref().unwrap().sequence > snapshot_sequences[loser_index]);
    // Retap the consumed ID after disconnect; do not infer ownership from the
    // lack of a delta alone. Ask for the loser's authoritative private view.
    loser.input.interact = true;
    loser.peer.network.send_message(
        CHANNEL,
        InventoryRequest::Resync {
            session: bootstrap[loser_index][0].session,
            compartment: CompartmentId(loser.peer.id),
        }
        .encode()
        .unwrap(),
    );
    let mut fresh = UdpPeer::new(winner_id, harness.address, harness.now, false);
    harness.pump(&mut [&mut fresh, loser], 128);
    let updates = drain(loser);
    assert_eq!(
        updates.len(),
        1,
        "stale pickup must not give loot to the remaining player"
    );
    assert!(matches!(&updates[0].body, UpdateBody::Snapshot(view)
        if view.version.id == CompartmentId(loser.peer.id)
        && view.version.revision == 0 && view.items.is_empty()));
    let updates = drain(&mut fresh);
    assert_eq!(updates.len(), 3);
    let fresh_key = updates[0].session;
    assert_eq!(updates[0].body, UpdateBody::Begin);
    let old_key = bootstrap[winner_index][0].session;
    assert_eq!(fresh_key.raid_epoch, old_key.raid_epoch);
    assert!(fresh_key.session_epoch > old_key.session_epoch);
    assert!(updates.iter().all(|update| update.session == fresh_key));
    assert!(matches!(&updates[1].body, UpdateBody::Snapshot(view)
        if view.version.id == CompartmentId(winner_id)
        && view.version.revision == 1 && view.items == [(item.id, granted)]));
    assert_eq!(updates[2].body, expected_ground);
    println!(
        "TK GROUND PICKUP UDP id={:?} winner={winner_id} snapshot_loss={}/{} stale_and_reconnect=ok",
        item.id, first.snapshots_dropped, second.snapshots_dropped
    );
}

#[derive(Default)]
struct ApplicationPayload {
    messages: usize,
    bytes: usize,
    max_message_bytes: usize,
}

impl ApplicationPayload {
    fn record(&mut self, bytes: usize) {
        self.messages += 1;
        self.bytes += bytes;
        self.max_message_bytes = self.max_message_bytes.max(bytes);
    }

    fn report(&self, phase: &str, direction: &str, expected: usize, limit: usize) {
        assert_eq!(self.messages, expected, "{phase} {direction}");
        assert!(self.max_message_bytes <= limit, "{phase} {direction}");
        println!(
            "TK INVENTORY UDP PAYLOAD phase={phase} direction={direction} messages={} bytes={} max_message_bytes={}",
            self.messages, self.bytes, self.max_message_bytes
        );
    }
}

#[test]
#[ignore = "real UDP acceptance: run explicitly with --ignored --nocapture"]
fn inventory_udp_parallel_private_moves_commit_and_reject_in_one_tick() {
    use super::super::super::inventory::UdpInventoryFixture;
    use common::{
        catalog::{
            CompartmentId,
            item_grid::{Footprint, Placement, PlacementError, Rotation},
            item_inventory::{
                Condition, GridLocation, Inventory, InventoryAction, InventoryError,
                InventoryOperation, InventoryReplica, OperationResult, ReplicaUpdate,
                TransactionError,
            },
        },
        replication::inventory::{
            CHANNEL, InventoryRequest, InventoryUpdate, MAX_REQUEST_BYTES, MAX_UPDATE_BYTES,
            UpdateBody,
        },
    };

    let mut harness = Harness::new();
    let mut first = UdpPeer::new(7, harness.address, harness.now, false);
    let mut second = UdpPeer::new(8, harness.address, harness.now, false);
    harness.pump(&mut [&mut first, &mut second], 192);

    let own = [CompartmentId(7), CompartmentId(8)];
    let place = |x| Placement {
        x,
        y: 0,
        rotation: Rotation::Upright,
    };
    let mut inventory = Inventory::default();
    let [first_item, second_item] = own.map(|compartment| {
        inventory
            .register_compartment(compartment, Footprint::new(4, 4).unwrap(), None)
            .unwrap();
        inventory
            .insert_seed(
                "med.bandage",
                Condition::Intact,
                2,
                GridLocation {
                    compartment,
                    placement: place(0),
                },
            )
            .unwrap()
    });
    inventory
        .insert_seed(
            "med.bandage",
            Condition::Intact,
            1,
            GridLocation {
                compartment: own[1],
                placement: place(1),
            },
        )
        .unwrap();
    harness
        .app
        .insert_resource(UdpInventoryFixture(1, Some(inventory)));
    first.input.lobby_sequence = 1;
    second.input.lobby_sequence = 1;
    harness.pump(&mut [&mut first, &mut second], 600);
    assert_eq!(
        harness.app.world().resource::<RaidState>().phase,
        RaidPhase::Active
    );

    // Count serialized application messages at the peers, not UDP datagrams.
    // Filter the lobby epoch out of bootstrap; no loss or replay is injected.
    let drain = |peer: &mut UdpPeer, payload: &mut ApplicationPayload| {
        std::iter::from_fn(|| peer.peer.network.receive_message(CHANNEL))
            .filter_map(|bytes| {
                let update = InventoryUpdate::decode(&bytes).unwrap();
                if update.session.raid_epoch != 1 {
                    return None;
                }
                payload.record(bytes.len());
                Some(update)
            })
            .collect::<Vec<_>>()
    };
    let send = |peer: &mut UdpPeer, request: InventoryRequest, payload: &mut ApplicationPayload| {
        let bytes = request.encode().unwrap();
        assert!(peer.peer.network.can_send_message(CHANNEL, bytes.len()));
        payload.record(bytes.len());
        peer.peer.network.send_message(CHANNEL, bytes);
    };
    let mut bootstrap = ApplicationPayload::default();
    let mut move_requests = ApplicationPayload::default();
    let mut move_updates = ApplicationPayload::default();
    let mut resync_requests = ApplicationPayload::default();
    let mut resync_updates = ApplicationPayload::default();
    let mut views = Vec::new();
    for (index, peer) in [&mut first, &mut second].into_iter().enumerate() {
        let compartment = own[index];
        let [begin, initial]: [InventoryUpdate; 2] =
            drain(peer, &mut bootstrap).try_into().unwrap();
        let session = begin.session;
        assert_eq!(begin.body, UpdateBody::Begin);
        assert_eq!(initial.session, session);
        let UpdateBody::Snapshot(snapshot) = initial.body else {
            panic!("missing private snapshot");
        };
        assert_eq!(snapshot.version.id, compartment);
        let item = [first_item, second_item][index];
        assert!(
            snapshot
                .items
                .iter()
                .any(|(id, stack)| *id == item && stack.placement == place(0))
        );
        let expected_revision = snapshot.version;
        let mut expected_items = snapshot.items.clone();
        if index == 0 {
            expected_items
                .iter_mut()
                .find(|(id, _)| *id == item)
                .unwrap()
                .1
                .placement = place(1);
        }
        let mut replica = InventoryReplica::new(session);
        assert_eq!(
            replica.apply_snapshot(session, snapshot),
            ReplicaUpdate::Applied
        );
        send(
            peer,
            InventoryRequest::Operate(InventoryOperation {
                session,
                id: 1,
                expected: [expected_revision; 2],
                action: InventoryAction::Move {
                    item,
                    destination: GridLocation {
                        compartment,
                        placement: place(1),
                    },
                },
            }),
            &mut move_requests,
        );
        views.push((session, replica, expected_revision, expected_items));
    }
    assert_ne!(views[0].0, views[1].0);
    let elapsed = |h: &Harness| h.app.world().resource::<Time<Fixed>>().elapsed();
    let before = elapsed(&harness);
    harness.step(&mut [&mut first, &mut second]);
    let fixed = harness.app.world().resource::<Time<Fixed>>();
    assert_eq!(fixed.elapsed() - before, fixed.timestep());

    // The next step's client transport receive precedes its server tick.
    // Therefore both replies read below came from the single preceding tick.
    harness.step(&mut [&mut first, &mut second]);
    for (index, (peer, (session, replica, revision, items))) in [&mut first, &mut second]
        .into_iter()
        .zip(&mut views)
        .enumerate()
    {
        let mut updates = drain(peer, &mut move_updates).into_iter();
        let committed = index == 0;
        if committed {
            let update = updates.next().expect("committed move must publish a delta");
            assert_eq!(update.session, *session);
            let UpdateBody::Delta(deltas) = update.body else {
                panic!("expected delta")
            };
            assert_eq!(deltas.len(), 1);
            assert_eq!(deltas[0].version.id, own[index]);
            assert_eq!(
                replica.apply_deltas(*session, deltas),
                ReplicaUpdate::Applied
            );
        }
        let result = if committed {
            OperationResult::Committed
        } else {
            OperationResult::Rejected(TransactionError::Rule(InventoryError::Placement(
                PlacementError::Occupied,
            )))
        };
        let reply = updates
            .next()
            .expect("both moves must reply in the same tick");
        assert_eq!(reply.session, *session);
        assert_eq!(reply.body, UpdateBody::Result { id: 1, result });
        assert!(updates.next().is_none());
        let confirmed = replica.get(own[index]).unwrap();
        assert_eq!(
            confirmed.version.revision,
            revision.revision + u64::from(committed)
        );
        assert_eq!(&confirmed.items, items);
        assert!(replica.get(own[1 - index]).is_none());
        send(
            peer,
            InventoryRequest::Resync {
                session: *session,
                compartment: own[index],
            },
            &mut resync_requests,
        );
    }
    harness.pump(&mut [&mut first, &mut second], 8);
    for (index, (peer, (session, replica, _, _))) in [&mut first, &mut second]
        .into_iter()
        .zip(&views)
        .enumerate()
    {
        let [update]: [InventoryUpdate; 1] = drain(peer, &mut resync_updates).try_into().unwrap();
        assert_eq!(update.session, *session);
        let UpdateBody::Snapshot(snapshot) = update.body else {
            panic!("expected confirmed snapshot")
        };
        assert_eq!(replica.get(own[index]), Some(&snapshot));
    }
    println!(
        "TK INVENTORY UDP same fixed tick: private grid 7 committed (+1 revision), grid 8 rejected occupied (unchanged); both resync snapshots matched"
    );
    bootstrap.report("active_bootstrap", "received", 4, MAX_UPDATE_BYTES);
    move_requests.report("same_tick_move", "submitted", 2, MAX_REQUEST_BYTES);
    move_updates.report("same_tick_move", "received", 3, MAX_UPDATE_BYTES);
    resync_requests.report("resync", "submitted", 2, MAX_REQUEST_BYTES);
    resync_updates.report("resync", "received", 2, MAX_UPDATE_BYTES);
}

#[test]
#[ignore = "real UDP acceptance: run explicitly with --ignored --nocapture"]
fn inventory_udp_reconnect_resyncs_committed_split_without_replay() {
    use super::super::super::inventory::UdpInventoryFixture;
    use common::{
        catalog::{
            CompartmentId,
            item_grid::{Footprint, Placement, Rotation},
            item_inventory::{
                Condition, GridLocation, Inventory, InventoryAction, InventoryOperation,
            },
        },
        replication::inventory::{CHANNEL, InventoryRequest, InventoryUpdate, UpdateBody},
    };

    let mut harness = Harness::new();
    let mut peer = UdpPeer::new(7, harness.address, harness.now, false);
    harness.pump(&mut [&mut peer], 192);
    let own = CompartmentId(7);
    let at = |x| GridLocation {
        compartment: own,
        placement: Placement {
            x,
            y: 0,
            rotation: Rotation::Upright,
        },
    };
    let mut inventory = Inventory::default();
    inventory
        .register_compartment(own, Footprint::new(4, 4).unwrap(), None)
        .unwrap();
    let item = inventory
        .insert_seed("med.bandage", Condition::Intact, 3, at(0))
        .unwrap();
    harness
        .app
        .insert_resource(UdpInventoryFixture(1, Some(inventory)));
    peer.input.lobby_sequence = 1;
    harness.pump(&mut [&mut peer], 600);
    assert_eq!(
        harness.app.world().resource::<RaidState>().phase,
        RaidPhase::Active
    );

    let drain = |peer: &mut UdpPeer| {
        std::iter::from_fn(|| peer.peer.network.receive_message(CHANNEL))
            .map(|bytes| InventoryUpdate::decode(&bytes).unwrap())
            .collect::<Vec<_>>()
    };
    let [begin, update]: [InventoryUpdate; 2] = drain(&mut peer)
        .into_iter()
        .filter(|update| update.session.raid_epoch == 1)
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let old_session = begin.session;
    assert_eq!(begin.body, UpdateBody::Begin);
    assert_eq!(update.session, old_session);
    let UpdateBody::Snapshot(initial) = update.body else {
        panic!("expected private snapshot")
    };
    assert_eq!(initial.version.id, own);
    assert_eq!(initial.items.len(), 1);
    let (source_id, source) = &initial.items[0];
    assert_eq!(
        (*source_id, source.quantity, source.placement),
        (item, 3, at(0).placement)
    );
    let request = InventoryRequest::Operate(InventoryOperation {
        session: old_session,
        id: 1,
        expected: [initial.version; 2],
        action: InventoryAction::Split {
            item,
            quantity: 1,
            destination: at(1),
        },
    });
    peer.peer
        .network
        .send_message(CHANNEL, request.encode().unwrap());
    harness.step(&mut [&mut peer]);

    // The harness sends the server's replies after its tick, and doesn't
    // receive them at the peer until another step. Disconnect at that boundary.
    let snapshot = |harness: &Harness| {
        harness
            .app
            .world()
            .resource::<WorldState>()
            .inventory
            .fixture_snapshot(own)
    };
    let committed = snapshot(&harness);
    assert_eq!(committed.version.revision, initial.version.revision + 1);
    assert_eq!(committed.items.len(), 2);
    for (id, stack) in &committed.items {
        let mut expected = source.clone();
        expected.quantity = if *id == item { 2 } else { 1 };
        expected.placement = at(u8::from(*id != item)).placement;
        assert_eq!(stack, &expected);
    }
    assert!(committed.items.iter().any(|(id, _)| *id == item));
    assert!(
        drain(&mut peer).is_empty(),
        "old peer has not received its reply"
    );
    peer.transport.disconnect();
    drop(peer);
    harness.pump(&mut [], 16);
    assert!(
        !harness
            .app
            .world()
            .resource::<Lobby>()
            .players
            .contains_key(&7)
    );

    let mut fresh = UdpPeer::new(7, harness.address, harness.now, false);
    harness.pump(&mut [&mut fresh], 128);
    assert!(fresh.transport.is_connected());
    let [begin, update]: [InventoryUpdate; 2] = drain(&mut fresh).try_into().unwrap();
    let session = begin.session;
    assert_eq!(begin.body, UpdateBody::Begin);
    assert_eq!(session.raid_epoch, old_session.raid_epoch);
    assert!(session.session_epoch > old_session.session_epoch);
    assert_eq!(update.session, session);
    assert_eq!(update.body, UpdateBody::Snapshot(snapshot(&harness)));

    fresh.peer.network.send_message(
        CHANNEL,
        InventoryRequest::Resync {
            session,
            compartment: own,
        }
        .encode()
        .unwrap(),
    );
    harness.pump(&mut [&mut fresh], 16);
    let [update]: [InventoryUpdate; 1] = drain(&mut fresh).try_into().unwrap();
    assert_eq!(update.session, session);
    assert_eq!(snapshot(&harness), committed);
    assert_eq!(update.body, UpdateBody::Snapshot(committed));
    println!(
        "TK INVENTORY UDP split committed before disconnect; same-ID reconnect resynced 2+1 units without replay"
    );
}

#[test]
#[ignore = "real UDP acceptance: run explicitly with --ignored --nocapture"]
fn inventory_udp_contested_item_converges_under_loss() {
    use super::super::super::inventory::{OpenContainer, UdpInventoryFixture};
    use common::{
        catalog::{
            CompartmentId,
            item_grid::{Footprint, Placement, Rotation},
            item_inventory::{
                Condition, GridLocation, Inventory, InventoryAction, InventoryOperation,
                InventoryReplica, OperationResult, ReplicaUpdate, VersionedCompartment,
            },
        },
        replication::inventory::{CHANNEL, InventoryRequest, InventoryUpdate, UpdateBody},
    };
    let mut harness = Harness::new();
    let mut first = UdpPeer::new(7, harness.address, harness.now, true);
    let mut second = UdpPeer::new(8, harness.address, harness.now, true);
    harness.pump(&mut [&mut first, &mut second], 192);
    let shared = CompartmentId(1 << 63);
    let size = Footprint::new(4, 4).unwrap();
    let placement = Placement {
        x: 0,
        y: 0,
        rotation: Rotation::Upright,
    };
    let mut inventory = Inventory::default();
    inventory.register_compartment(shared, size, None).unwrap();
    let item = inventory
        .insert_seed(
            "med.bandage",
            Condition::Damaged,
            2,
            GridLocation {
                compartment: shared,
                placement,
            },
        )
        .unwrap();
    harness
        .app
        .insert_resource(UdpInventoryFixture(1, Some(inventory)));
    for client in [7, 8] {
        let entity = harness.app.world().resource::<Lobby>().players[&client];
        harness
            .app
            .world_mut()
            .entity_mut(entity)
            .insert(OpenContainer {
                raid_id: 1,
                id: shared,
                size,
            });
    }
    first.input.lobby_sequence = 1;
    second.input.lobby_sequence = 1;
    harness.pump(&mut [&mut first, &mut second], 600);
    assert_eq!(
        harness.app.world().resource::<RaidState>().phase,
        RaidPhase::Active
    );
    let drain = |peer: &mut UdpPeer| {
        std::iter::from_fn(|| peer.peer.network.receive_message(CHANNEL))
            .map(|bytes| InventoryUpdate::decode(&bytes).unwrap())
            .collect::<Vec<_>>()
    };
    let mut views = Vec::new();
    for (peer, own) in [
        (&mut first, CompartmentId(7)),
        (&mut second, CompartmentId(8)),
    ] {
        let updates: Vec<_> = drain(peer)
            .into_iter()
            .filter(|u| u.session.raid_epoch == 1)
            .collect();
        assert_eq!(updates.len(), 3);
        let session = updates[0].session;
        assert_eq!(updates[0].body, UpdateBody::Begin);
        let mut replica = InventoryReplica::new(session);
        for update in updates.into_iter().skip(1) {
            let UpdateBody::Snapshot(snapshot) = update.body else {
                panic!("expected initial snapshot");
            };
            assert_eq!(
                replica.apply_snapshot(session, snapshot),
                ReplicaUpdate::Applied
            );
        }
        assert_eq!(replica.get(shared).unwrap().items[0].0, item);
        assert!(replica.get(own).unwrap().items.is_empty());
        let expected = [VersionedCompartment {
            id: shared,
            revision: 0,
        }; 2];
        let operation = InventoryOperation {
            session,
            id: 1,
            expected,
            action: InventoryAction::Move {
                item,
                destination: GridLocation {
                    compartment: shared,
                    placement: Placement {
                        x: own.0 as u8 - 6,
                        ..placement
                    },
                },
            },
        };
        for _ in 0..2 {
            peer.peer.network.send_message(
                CHANNEL,
                InventoryRequest::Operate(operation).encode().unwrap(),
            );
        }
        views.push((session, replica));
    }
    harness.pump(&mut [&mut first, &mut second], 160);
    let mut winners = 0;
    for (peer, (session, replica)) in [&mut first, &mut second].into_iter().zip(&mut views) {
        let mut deltas = 0;
        let mut results = Vec::new();
        for update in drain(peer) {
            assert_eq!(update.session, *session);
            match update.body {
                UpdateBody::Delta(delta) => {
                    deltas += 1;
                    assert_eq!(
                        replica.apply_deltas(*session, delta),
                        ReplicaUpdate::Applied
                    );
                }
                UpdateBody::Result { id: 1, result } => results.push(result),
                body => panic!("unexpected update: {body:?}"),
            }
        }
        assert_eq!(deltas, 1);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0], results[1]);
        match results[0] {
            OperationResult::Committed => winners += 1,
            OperationResult::Conflict => {}
            other => panic!("unexpected outcome: {other:?}"),
        }
        assert!(peer.dropped.load(Ordering::Relaxed) > 0);
    }
    assert_eq!(winners, 1);
    let snapshot = views[0].1.get(shared).unwrap();
    assert_eq!(views[1].1.get(shared), Some(snapshot));
    assert_eq!(snapshot.version.revision, 1);
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(
        (
            snapshot.items[0].0,
            snapshot.items[0].1.quantity,
            snapshot.items[0].1.condition
        ),
        (item, 2, Condition::Damaged)
    );

    // The second peer now tries a cross-compartment transfer using the
    // shared revision from before the contested move. The destination is
    // otherwise valid and empty; neither inventory may change on Conflict.
    let second_own = CompartmentId(8);
    let second_key = views[1].0;
    let second_before = views[1].1.get(second_own).unwrap();
    let shared_before = snapshot;
    let stale_transfer = InventoryOperation {
        session: second_key,
        id: 2,
        expected: [
            VersionedCompartment {
                id: shared,
                revision: 0,
            },
            second_before.version,
        ],
        action: InventoryAction::Move {
            item,
            destination: GridLocation {
                compartment: second_own,
                placement,
            },
        },
    };
    for _ in 0..2 {
        second.peer.network.send_message(
            CHANNEL,
            InventoryRequest::Operate(stale_transfer).encode().unwrap(),
        );
    }
    harness.pump(&mut [&mut first, &mut second], 160);
    assert!(drain(&mut first).is_empty());
    let stale_results = drain(&mut second);
    assert_eq!(stale_results.len(), 2);
    assert!(stale_results.iter().all(|update| {
        update.session == second_key
            && update.body
                == UpdateBody::Result {
                    id: 2,
                    result: OperationResult::Conflict,
                }
    }));
    for compartment in [shared, second_own] {
        second.peer.network.send_message(
            CHANNEL,
            InventoryRequest::Resync {
                session: second_key,
                compartment,
            }
            .encode()
            .unwrap(),
        );
    }
    harness.pump(&mut [&mut first, &mut second], 160);
    assert!(drain(&mut first).is_empty());
    let snapshots = drain(&mut second);
    assert_eq!(snapshots.len(), 2);
    for (update, expected) in snapshots.iter().zip([shared_before, second_before]) {
        assert_eq!(update.session, second_key);
        assert!(matches!(&update.body, UpdateBody::Snapshot(actual) if actual == expected));
    }

    // Transfer the surviving stack out of the shared view with fresh
    // revisions. Only the new owner may see the destination.
    let own = CompartmentId(7);
    let session = views[0].0;
    let transfer = InventoryOperation {
        session,
        id: 2,
        expected: [shared_before.version, views[0].1.get(own).unwrap().version],
        action: InventoryAction::Move {
            item,
            destination: GridLocation {
                compartment: own,
                placement,
            },
        },
    };
    first.peer.network.send_message(
        CHANNEL,
        InventoryRequest::Operate(transfer).encode().unwrap(),
    );
    harness.pump(&mut [&mut first, &mut second], 160);
    for (index, (peer, (session, replica))) in [&mut first, &mut second]
        .into_iter()
        .zip(&mut views)
        .enumerate()
    {
        let mut updates = drain(peer).into_iter();
        let update = updates.next().unwrap();
        assert_eq!(update.session, *session);
        let UpdateBody::Delta(delta) = update.body else {
            panic!("expected transfer delta");
        };
        assert_eq!(delta.len(), if index == 0 { 2 } else { 1 });
        assert_eq!(
            replica.apply_deltas(*session, delta),
            ReplicaUpdate::Applied
        );
        let shared_view = replica.get(shared).unwrap();
        assert_eq!(shared_view.version.revision, 2);
        assert!(shared_view.items.is_empty());
        if index == 0 {
            let (_, destination_item) = &replica.get(own).unwrap().items[0];
            assert_eq!(replica.get(own).unwrap().items[0].0, item);
            assert_eq!(
                (destination_item.quantity, destination_item.condition),
                (2, Condition::Damaged)
            );
            assert_eq!(
                updates.next().unwrap().body,
                UpdateBody::Result {
                    id: 2,
                    result: OperationResult::Committed
                }
            );
        } else {
            assert!(replica.get(own).is_none());
        }
        assert!(updates.next().is_none());
    }

    harness
        .app
        .world_mut()
        .resource_mut::<RenetServer>()
        .disconnect(7);
    harness.pump(&mut [&mut second], 64);
    assert!(
        !harness
            .app
            .world()
            .resource::<Lobby>()
            .players
            .contains_key(&7)
    );
    let mut reconnected = UdpPeer::new(7, harness.address, harness.now, true);
    harness.pump(&mut [&mut reconnected, &mut second], 192);
    assert!(reconnected.transport.is_connected());
    let updates = drain(&mut reconnected);
    assert_eq!(updates.len(), 2);
    let new_session = updates[0].session;
    assert_eq!(updates[0].body, UpdateBody::Begin);
    assert_eq!(new_session.raid_epoch, session.raid_epoch);
    assert!(new_session.session_epoch > session.session_epoch);
    assert!(matches!(&updates[1].body, UpdateBody::Snapshot(snapshot)
        if snapshot.version.id == own && snapshot.items.len() == 1 && snapshot.items[0].0 == item));
    for request in [
        InventoryRequest::Operate(transfer),
        InventoryRequest::Resync {
            session: new_session,
            compartment: own,
        },
    ] {
        reconnected
            .peer
            .network
            .send_message(CHANNEL, request.encode().unwrap());
    }
    harness.pump(&mut [&mut reconnected, &mut second], 128);
    let updates = drain(&mut reconnected);
    assert_eq!(
        updates.len(),
        1,
        "old operation must not produce a result or delta"
    );
    assert!(matches!(&updates[0].body, UpdateBody::Snapshot(snapshot)
        if snapshot.version.id == own && snapshot.items.len() == 1 && snapshot.items[0].0 == item));

    // Inject the lifecycle boundary, not the packets: the production system
    // must replace both inventories and announce the new epoch over UDP.
    harness.app.world_mut().resource_mut::<RaidState>().raid_id = 2;
    harness.pump(&mut [&mut reconnected, &mut second], 160);
    let mut ground = None;
    for (peer, own) in [(&mut reconnected, own), (&mut second, CompartmentId(8))] {
        let updates = drain(peer);
        assert_eq!(updates.len(), 3);
        assert_eq!(updates[0].body, UpdateBody::Begin);
        let new_key = updates[0].session;
        assert_eq!(new_key.raid_epoch, 2);
        assert!(updates.iter().all(|update| update.session == new_key));
        assert!(matches!(&updates[1].body, UpdateBody::Snapshot(snapshot)
            if snapshot.version.id == own && snapshot.version.revision == 0 && snapshot.items.is_empty()));
        let UpdateBody::Ground(items) = &updates[2].body else {
            panic!("expected freshly seeded ground list in the active raid");
        };
        assert_eq!(items.len(), common::catalog::item_manifest::S1_SEEDS.len());
        if let Some(expected) = &ground {
            assert_eq!(items, expected);
        } else {
            ground = Some(items.clone());
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
