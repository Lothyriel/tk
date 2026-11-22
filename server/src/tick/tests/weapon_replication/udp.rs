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
fn inventory_udp_ground_bootstrap_late_join_and_cleanup() {
    use common::{
        catalog::{
            ItemInstanceId,
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
        && deltas[0].items[0].item.is_some()));
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
    assert_eq!(displayed(&first).carried, 1);
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
    let [update] = public.as_slice() else {
        panic!("expected one public ground update: {public:?}")
    };
    let UpdateBody::Ground(dropped) = &update.body else {
        panic!("expected ground update")
    };
    assert_eq!(dropped.len(), items.len());
    let dropped = &dropped[0];
    assert_eq!((dropped.id, dropped.seed), (item.id, item.seed));
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
    assert_eq!(displayed(&first).carried, 1);
    assert!(
        drain(&mut first)
            .iter()
            .any(|update| matches!(&update.body, UpdateBody::Delta(deltas)
        if deltas[0].version.revision == 3 && deltas[0].items[0].id == item.id))
    );
    assert_eq!(drain(&mut late).len(), 1);

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
    assert_eq!(displayed(&first).secured, 0);
    harness.pump(&mut [&mut first, &mut late], 256);
    assert_eq!(displayed(&first).status, RaidStatus::Extracted);
    assert_eq!(displayed(&first).carried, 0);
    assert_eq!(displayed(&first).secured, 1);
    harness.pump(&mut [&mut first, &mut late], 128);
    assert_eq!(displayed(&first).secured, 1);
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

    // Transfer the surviving stack out of the shared view. Only its new
    // owner may see the destination; the other peer sees just the removal.
    let own = CompartmentId(7);
    let session = views[0].0;
    let transfer = InventoryOperation {
        session,
        id: 2,
        expected: [snapshot.version, views[0].1.get(own).unwrap().version],
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
