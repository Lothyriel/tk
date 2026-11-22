use std::collections::BTreeMap;

use bevy::prelude::*;
use bevy_renet2::prelude::RenetServer;
use common::{
    Lobby,
    catalog::{
        CompartmentId,
        item_grid::Footprint,
        item_inventory::{InventoryAuthority, OperationSession, SessionKey},
    },
    raid::{Participant, RaidPhase, RaidState, RaidStatus},
    replication::inventory::{CHANNEL, InventoryRequest, InventoryUpdate, UpdateBody},
};

// Four requests per 125 ms per peer at 128 Hz. Leave excess in Renet's bounded
// queue rather than dropping an ordered operation and creating a sequence gap.
const REQUESTS_PER_WINDOW: u8 = 4;
const WINDOW_TICKS: u8 = 16;

struct Peer {
    entity: Entity,
    key: SessionKey,
    operations: OperationSession,
    remaining: u8,
}

#[derive(Default)]
pub(super) struct Endpoint {
    authority: Option<InventoryAuthority>,
    raid: Option<u32>,
    session_epoch: u64,
    window_tick: u8,
    peers: BTreeMap<u64, Peer>,
}

fn send(server: &mut RenetServer, client: u64, session: SessionKey, body: UpdateBody) {
    match (InventoryUpdate { session, body }).encode() {
        Ok(bytes) if server.can_send_message(client, CHANNEL, bytes.len()) => {
            server.send_message(client, CHANNEL, bytes);
        }
        // Never silently skip a confirmed delta or close; disconnecting clears
        // the client view and a fresh stream starts with a snapshot.
        _ => server.disconnect(client),
    }
}

pub(super) fn receive(
    mut server: ResMut<RenetServer>,
    lobby: Res<Lobby>,
    raid: Res<RaidState>,
    participants: Query<&Participant>,
    mut state: Local<Endpoint>,
) {
    process(&mut server, &lobby, &raid, &participants, &mut state);
}

fn process(
    server: &mut RenetServer,
    lobby: &Lobby,
    raid: &RaidState,
    participants: &Query<&Participant>,
    state: &mut Endpoint,
) {
    if state.raid != Some(raid.raid_id) {
        state.raid = Some(raid.raid_id);
        state.authority = Some(InventoryAuthority::new(
            u64::from(raid.raid_id),
            Default::default(),
        ));
        state.peers.clear();
    }
    let authority = state
        .authority
        .as_mut()
        .expect("initialized raid authority");
    state.peers.retain(|&client, _| {
        if server.is_connected(client) && lobby.players.contains_key(&client) {
            true
        } else {
            authority.remove_empty_compartment(CompartmentId(client));
            false
        }
    });
    let refill = state.window_tick == 0;
    state.window_tick = (state.window_tick + 1) % WINDOW_TICKS;
    for client in server.clients_id() {
        let Some(&entity) = lobby.players.get(&client) else {
            continue;
        };
        let own = CompartmentId(client);
        let owns = |id| id == own;
        if state
            .peers
            .get(&client)
            .is_none_or(|peer| peer.entity != entity)
        {
            if authority.revision(own).is_none() {
                authority
                    .register_compartment(own, Footprint::new(4, 4).unwrap())
                    .expect("new compartment");
            }
            state.session_epoch = state.session_epoch.checked_add(1).expect("session epoch");
            let key = SessionKey {
                raid_epoch: u64::from(raid.raid_id),
                session_epoch: state.session_epoch,
            };
            state.peers.insert(
                client,
                Peer {
                    entity,
                    key,
                    operations: OperationSession::new(key),
                    remaining: REQUESTS_PER_WINDOW,
                },
            );
            send(server, client, key, UpdateBody::Begin);
            if server.is_connected(client) {
                send_snapshot(authority, server, client, key, own);
            }
        }
        let peer = state.peers.get_mut(&client).expect("connected peer");
        if refill {
            peer.remaining = REQUESTS_PER_WINDOW;
        }
        while peer.remaining > 0 && server.is_connected(client) {
            let Some(bytes) = server.receive_message(client, CHANNEL) else {
                break;
            };
            peer.remaining -= 1;
            match InventoryRequest::decode(&bytes) {
                Ok(InventoryRequest::Resync {
                    session,
                    compartment,
                }) if session == peer.key => {
                    send_snapshot(authority, server, client, peer.key, compartment);
                }
                Ok(InventoryRequest::Operate(request)) if request.session == peer.key => {
                    let active = raid.phase == RaidPhase::Active
                        && participants
                            .get(entity)
                            .is_ok_and(|p| p.status == RaidStatus::Active);
                    let reply =
                        authority.apply(&mut peer.operations, request, |id| active && owns(id));
                    if let Some(commit) = reply.commit {
                        let deltas = commit.project(peer.key, owns);
                        send(server, client, peer.key, UpdateBody::Delta(deltas));
                    }
                    if server.is_connected(client) {
                        send(
                            server,
                            client,
                            peer.key,
                            UpdateBody::Result {
                                id: request.id,
                                result: reply.result,
                            },
                        );
                    }
                }
                _ => {}
            }
        }
    }
}

fn send_snapshot(
    authority: &InventoryAuthority,
    server: &mut RenetServer,
    client: u64,
    key: SessionKey,
    id: CompartmentId,
) {
    use common::catalog::item_inventory::SnapshotError;
    match authority.snapshot(id, |id| id == CompartmentId(client)) {
        Ok(snapshot) => send(server, client, key, UpdateBody::Snapshot(snapshot)),
        Err(SnapshotError::Unavailable) => send(server, client, key, UpdateBody::Closed(id)),
        Err(SnapshotError::TooManyItems) => server.disconnect(client),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_renet2::prelude::RenetClient;
    use common::catalog::{
        item_grid::{Placement, Rotation},
        item_inventory::{
            Condition, GridLocation, Inventory, InventoryAction, InventoryOperation,
            OperationResult, VersionedCompartment,
        },
    };

    #[test]
    fn inventory_endpoint_bootstraps_and_commits_only_for_the_owner() {
        let config = common::data::renet_config();
        let mut server = RenetServer::new(config.clone());
        let mut owner = RenetClient::new(config.clone(), false);
        let mut other = RenetClient::new(config, false);
        owner.set_connected();
        other.set_connected();
        let id = CompartmentId(7);
        let location = GridLocation {
            compartment: id,
            placement: Placement {
                x: 0,
                y: 0,
                rotation: Rotation::Upright,
            },
        };
        let mut inventory = Inventory::default();
        inventory
            .register_compartment(id, Footprint::new(4, 4).unwrap(), None)
            .unwrap();
        let item = inventory
            .insert_seed("med.bandage", Condition::Intact, 1, location)
            .unwrap();
        let mut app = App::new();
        let mut lobby = Lobby::default();
        for client in [7, 8] {
            server.add_connection(client, false);
            let entity = app
                .world_mut()
                .spawn(Participant {
                    status: RaidStatus::Active,
                    ..Default::default()
                })
                .id();
            lobby.players.insert(client, entity);
        }
        app.insert_resource(server)
            .insert_resource(lobby)
            .insert_resource(RaidState {
                raid_id: 2,
                phase: RaidPhase::Active,
                ..Default::default()
            });
        let mut endpoint = Endpoint {
            authority: Some(InventoryAuthority::new(2, inventory)),
            raid: Some(2),
            ..Default::default()
        };
        app.add_systems(
            Update,
            move |mut server: ResMut<RenetServer>,
                  lobby: Res<Lobby>,
                  raid: Res<RaidState>,
                  participants: Query<&Participant>| {
                process(&mut server, &lobby, &raid, &participants, &mut endpoint);
            },
        );
        let pull = |app: &mut App, client: u64, peer: &mut RenetClient| {
            let mut server = app.world_mut().resource_mut::<RenetServer>();
            let packets = server.get_packets_to_send(client).unwrap();
            for packet in packets {
                peer.process_packet(packet);
            }
            std::iter::from_fn(|| peer.receive_message(CHANNEL))
                .map(|bytes| InventoryUpdate::decode(&bytes).unwrap())
                .collect::<Vec<_>>()
        };
        let push =
            |app: &mut App, client: u64, peer: &mut RenetClient, requests: &[InventoryRequest]| {
                for request in requests {
                    peer.send_message(CHANNEL, request.encode().unwrap());
                }
                let mut server = app.world_mut().resource_mut::<RenetServer>();
                for packet in peer.get_packets_to_send() {
                    server.process_packet_from(packet, client).unwrap();
                }
            };
        app.update();
        let initial = pull(&mut app, 7, &mut owner);
        let key = initial[0].session;
        assert_eq!(initial[0].body, UpdateBody::Begin);
        assert!(matches!(&initial[1].body, UpdateBody::Snapshot(s) if s.items[0].0 == item));
        let initial_other = pull(&mut app, 8, &mut other);
        assert!(
            matches!(&initial_other[1].body, UpdateBody::Snapshot(s) if s.items.is_empty() && s.version.id == CompartmentId(8))
        );
        push(
            &mut app,
            8,
            &mut other,
            &[InventoryRequest::Resync {
                session: initial_other[0].session,
                compartment: id,
            }],
        );
        let mut destination = location;
        destination.placement.x = 1;
        let request = InventoryOperation {
            session: key,
            id: 1,
            expected: [VersionedCompartment { id, revision: 0 }; 2],
            action: InventoryAction::Move { item, destination },
        };
        push(
            &mut app,
            7,
            &mut owner,
            &[
                InventoryRequest::Operate(request),
                InventoryRequest::Operate(request),
            ],
        );
        app.update();
        let updates = pull(&mut app, 7, &mut owner);
        assert_eq!(updates.len(), 3);
        assert!(matches!(&updates[0].body, UpdateBody::Delta(d)
            if d[0].version.revision == 1 && d[0].items[0].item.as_ref().unwrap().placement == destination.placement));
        for update in &updates[1..] {
            assert_eq!(
                update.body,
                UpdateBody::Result {
                    id: 1,
                    result: OperationResult::Committed
                }
            );
        }
        let other_updates = pull(&mut app, 8, &mut other);
        assert_eq!(other_updates.len(), 1);
        assert_eq!(other_updates[0].body, UpdateBody::Closed(id));
    }
}
