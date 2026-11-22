use std::collections::BTreeMap;

use bevy::{ecs::system::SystemParam, prelude::*};
use bevy_rapier3d::prelude::*;
use bevy_renet2::prelude::RenetServer;
use common::{
    CameraInput, ClientInput, Lobby, MovementState,
    catalog::{
        CompartmentId, ItemInstanceId,
        item_grid::{Footprint, Placement, Rotation},
        item_inventory::{
            Condition, GridLocation, Inventory, InventoryAuthority, OperationSession, SessionKey,
        },
        item_manifest::{S1_SEEDS, item_visuals, planned_items},
    },
    combat::{Arsenal, BodyHealth},
    humanoid::{PhysicalMode, PoseState, aim_lean_offset},
    raid::{
        Participant, RaidPhase, RaidState, RaidStatus,
        item_target::{GroundTarget, PICKUP_REACH_M, first_ground_target},
    },
    replication::inventory::{CHANNEL, GroundItem, InventoryRequest, InventoryUpdate, UpdateBody},
};

// Four requests per 125 ms per peer at 128 Hz. Leave excess in Renet's bounded
// queue rather than dropping an ordered operation and creating a sequence gap.
const REQUESTS_PER_WINDOW: u8 = 4;
const WINDOW_TICKS: u8 = 16;
const WORLD_SLOT_START: u64 = u64::MAX - S1_SEEDS.len() as u64 + 1;

#[derive(Debug)]
pub(super) struct WorldItem {
    pub id: ItemInstanceId,
    pub source: CompartmentId,
    /// Static interaction volume. Keep it out of the physics broad phase
    /// until a dropped item actually needs rigid-body simulation.
    pub half_extents: Vec3,
    pub center: Vec3,
}

/// Granted by authoritative interaction code, never by an inventory packet.
/// Shared compartments occupy the namespace above player connection IDs.
#[derive(Component)]
pub(super) struct OpenContainer {
    pub raid_id: u32,
    pub id: CompartmentId,
    pub size: Footprint,
}

struct Peer {
    entity: Entity,
    key: SessionKey,
    operations: OperationSession,
    remaining: u8,
    open: Option<CompartmentId>,
    ground_count: usize,
    focus: Option<ItemInstanceId>,
}

#[derive(Default)]
pub(super) struct Endpoint {
    authority: Option<InventoryAuthority>,
    raid: Option<u32>,
    session_epoch: u64,
    window_tick: u8,
    peers: BTreeMap<u64, Peer>,
    world_items: Vec<WorldItem>,
}

fn seed_world(items: &mut Vec<WorldItem>) -> Inventory {
    let mut inventory = Inventory::default();
    for (index, seed) in S1_SEEDS.into_iter().enumerate() {
        let source = CompartmentId(WORLD_SLOT_START + index as u64);
        let visual = &item_visuals()[seed];
        inventory
            .register_compartment(source, planned_items()[seed].footprint, None)
            .expect("distinct ground slot");
        let id = inventory
            .insert_seed(
                seed,
                Condition::Intact,
                1,
                GridLocation {
                    compartment: source,
                    placement: Placement {
                        x: 0,
                        y: 0,
                        rotation: Rotation::Upright,
                    },
                },
            )
            .expect("seed fits ground slot");
        let [x, _, z] = common::map::LOOT_POSITIONS[index / 2];
        let half_extents = Vec3::from_array(visual.collider_half_extents());
        let terrain_top = common::map::BLOCKS[0].center[1] + common::map::BLOCKS[0].half_size[1];
        // Two seeds flank each legacy crate at the authored floor height.
        let center = Vec3::new(
            x + if index % 2 == 0 { -0.8 } else { 0.8 },
            terrain_top + half_extents.y,
            z,
        );
        let item = WorldItem {
            id,
            source,
            half_extents,
            center,
        };
        debug!(item = ?item.id, slot = ?item.source, bounds = ?item.half_extents, center = ?item.center, seed, "placed raid item");
        items.push(item);
    }
    inventory
}

#[cfg(test)]
#[derive(Resource)]
pub(super) struct UdpInventoryFixture(
    pub u32,
    pub Option<common::catalog::item_inventory::Inventory>,
);

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
    participants: Query<(&Participant, Option<&OpenContainer>)>,
    sight: TargetSight,
    mut state: Local<Endpoint>,
    #[cfg(test)] fixture: Option<ResMut<UdpInventoryFixture>>,
) {
    #[cfg(test)]
    if let Some(mut fixture) = fixture
        && fixture.0 == raid.raid_id
        && let Some(inventory) = fixture.1.take()
    {
        assert_ne!(state.raid, Some(raid.raid_id));
        state.raid = Some(raid.raid_id);
        state.authority = Some(InventoryAuthority::new(u64::from(raid.raid_id), inventory));
        state.peers.clear();
    }
    state.sync_world(&raid);
    process(
        &mut server,
        &lobby,
        &raid,
        &participants,
        &sight,
        &mut state,
    );
}

#[derive(SystemParam)]
pub(super) struct TargetSight<'w, 's> {
    context: ReadRapierContext<'w, 's>,
    actors: Query<
        'w,
        's,
        (
            &'static ClientInput,
            &'static Transform,
            &'static MovementState,
            &'static Participant,
            &'static PoseState,
            &'static BodyHealth,
            &'static Arsenal,
        ),
    >,
}

impl TargetSight<'_, '_> {
    pub(super) fn focus(&self, entity: Entity, items: &[WorldItem]) -> Option<ItemInstanceId> {
        if items.is_empty() {
            return None;
        }
        let (input, root, movement, participant, pose, body, arsenal) =
            self.actors.get(entity).ok()?;
        if participant.status != RaidStatus::Active
            || !body.alive()
            || !matches!(pose.mode, PhysicalMode::Standing | PhysicalMode::Stumbling)
        {
            return None;
        }
        let camera = arsenal.recoil.camera(CameraInput {
            roll: pose.aim_roll,
            ..input.camera
        });
        let direction = Dir3::new(Quat::from(&camera) * Vec3::NEG_Z).ok()?;
        let eye = super::simulation::neutral_eye(root, movement)
            + root.rotation * aim_lean_offset(pose.aim_roll);
        if !eye.is_finite() {
            return None;
        }
        let obstruction = self
            .context
            .single()
            .ok()?
            .cast_ray(
                eye,
                *direction,
                PICKUP_REACH_M,
                true,
                QueryFilter::new()
                    .groups(CollisionGroups::new(
                        Group::ALL,
                        Group::GROUP_1 | Group::GROUP_4,
                    ))
                    .exclude_collider(entity)
                    .exclude_sensors(),
            )
            .map(|(_, distance)| distance);
        first_ground_target(
            eye,
            direction,
            obstruction,
            items.iter().map(|item| GroundTarget {
                id: item.id,
                center: item.center,
                half_extents: item.half_extents,
            }),
        )
    }
}

impl Endpoint {
    fn sync_world(&mut self, raid: &RaidState) {
        if self.raid != Some(raid.raid_id) || raid.phase != RaidPhase::Active {
            self.world_items.clear();
        }
        if raid.phase == RaidPhase::Active && self.raid != Some(raid.raid_id) {
            let inventory = seed_world(&mut self.world_items);
            self.raid = Some(raid.raid_id);
            self.authority = Some(InventoryAuthority::new(u64::from(raid.raid_id), inventory));
            self.peers.clear();
        }
    }
}

fn process(
    server: &mut RenetServer,
    lobby: &Lobby,
    raid: &RaidState,
    participants: &Query<(&Participant, Option<&OpenContainer>)>,
    sight: &TargetSight,
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
    let clients = server.clients_id();
    // Reconcile every view before executing any command so fanout uses this
    // tick's grants, independent of connection iteration order.
    for &client in &clients {
        let Some(&entity) = lobby.players.get(&client) else {
            continue;
        };
        let own = CompartmentId(client);
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
                    open: None,
                    ground_count: 0,
                    focus: None,
                },
            );
            send(server, client, key, UpdateBody::Begin);
            if server.is_connected(client) {
                send_snapshot(authority, server, client, key, own, |_| true);
            }
        }
        let peer = state.peers.get_mut(&client).expect("connected peer");
        // Static list changes only on spawn and cleanup. Future moves/drops
        // must track content changes, not just the number of items.
        if peer.ground_count != state.world_items.len() && server.is_connected(client) {
            let items = state
                .world_items
                .iter()
                .map(|item| {
                    let seed = (item.source.0 - WORLD_SLOT_START) as u8;
                    let visual = &item_visuals()[S1_SEEDS[usize::from(seed)]];
                    GroundItem {
                        id: item.id,
                        seed,
                        root: (item.center - Vec3::from_array(visual.collider_center())).to_array(),
                    }
                })
                .collect();
            send(server, client, peer.key, UpdateBody::Ground(items));
            peer.ground_count = state.world_items.len();
        }
        let focus = sight.focus(entity, &state.world_items);
        if peer.focus != focus && server.is_connected(client) {
            send(server, client, peer.key, UpdateBody::GroundFocus(focus));
            peer.focus = focus;
        }
        if refill {
            peer.remaining = REQUESTS_PER_WINDOW;
        }
        let grant = participants
            .get(entity)
            .ok()
            .and_then(|(participant, grant)| {
                grant.filter(|grant| {
                    raid.phase == RaidPhase::Active
                        && participant.status == RaidStatus::Active
                        && grant.raid_id == raid.raid_id
                        && ((1_u64 << 63)..WORLD_SLOT_START).contains(&grant.id.0)
                })
            });
        let open = grant.map(|grant| grant.id);
        if peer.open != open && server.is_connected(client) {
            if let Some(closed) = peer.open {
                send(server, client, peer.key, UpdateBody::Closed(closed));
            }
            peer.open = open;
            if let Some(grant) = grant
                && server.is_connected(client)
            {
                if authority.revision(grant.id).is_none() {
                    authority
                        .register_compartment(grant.id, grant.size)
                        .expect("new shared compartment");
                }
                send_snapshot(authority, server, client, peer.key, grant.id, |_| true);
            }
        }
    }
    for client in clients {
        while server.is_connected(client) {
            let Some(peer) = state
                .peers
                .get_mut(&client)
                .filter(|peer| peer.remaining > 0)
            else {
                break;
            };
            let Some(bytes) = server.receive_message(client, CHANNEL) else {
                break;
            };
            peer.remaining -= 1;
            let key = peer.key;
            let open = peer.open;
            let can_access = |id| id == CompartmentId(client) || open == Some(id);
            match InventoryRequest::decode(&bytes) {
                Ok(InventoryRequest::Resync {
                    session,
                    compartment,
                }) if session == peer.key => {
                    send_snapshot(authority, server, client, key, compartment, can_access);
                }
                Ok(InventoryRequest::Operate(request)) if request.session == peer.key => {
                    let active = raid.phase == RaidPhase::Active
                        && participants
                            .get(peer.entity)
                            .is_ok_and(|(p, _)| p.status == RaidStatus::Active);
                    let reply = authority
                        .apply(&mut peer.operations, request, |id| active && can_access(id));
                    if let Some(commit) = reply.commit {
                        for (&recipient, view) in &state.peers {
                            if !server.is_connected(recipient) {
                                continue;
                            }
                            let deltas = commit.project(view.key, |id| {
                                id == CompartmentId(recipient) || view.open == Some(id)
                            });
                            if !deltas.is_empty() {
                                send(server, recipient, view.key, UpdateBody::Delta(deltas));
                            }
                        }
                    }
                    if server.is_connected(client) {
                        send(
                            server,
                            client,
                            key,
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
    may_read: impl Fn(CompartmentId) -> bool,
) {
    use common::catalog::item_inventory::SnapshotError;
    match authority.snapshot(id, may_read) {
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
            InventoryAction, InventoryOperation, OperationResult, VersionedCompartment,
        },
    };

    #[test]
    fn world_item_seeds_spawn_once_per_raid_and_leave_on_debrief() {
        let mut endpoint = Endpoint::default();
        let mut raid = RaidState {
            raid_id: 1,
            phase: RaidPhase::Active,
            ..default()
        };
        endpoint.sync_world(&raid);
        assert_eq!(endpoint.world_items.len(), S1_SEEDS.len());
        let authority = endpoint.authority.as_ref().unwrap();
        for (index, item) in endpoint.world_items.iter().enumerate() {
            assert_eq!(item.source.0, WORLD_SLOT_START + index as u64);
            assert_eq!(item.id.0, index as u64 + 1);
            let seed = S1_SEEDS[index];
            let entry = authority.inventory().get(item.id).unwrap();
            assert_eq!(entry.definition.id, seed);
            assert_eq!(entry.location.compartment, item.source);
            let visual = &item_visuals()[seed];
            assert!(
                item.half_extents
                    .abs_diff_eq(Vec3::from_array(visual.collider_half_extents()), 1e-6)
            );
            assert!((item.center.y - item.half_extents.y - 0.1).abs() < 1e-6);
        }
        endpoint.sync_world(&raid);
        assert_eq!(endpoint.world_items.len(), S1_SEEDS.len());
        raid.phase = RaidPhase::Debrief;
        endpoint.sync_world(&raid);
        assert!(endpoint.world_items.is_empty());
        raid.raid_id = 2;
        raid.phase = RaidPhase::Active;
        endpoint.sync_world(&raid);
        assert_eq!(endpoint.raid, Some(2));
        assert_eq!(endpoint.world_items.len(), S1_SEEDS.len());
    }

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
                  participants: Query<(&Participant, Option<&OpenContainer>)>,
                  sight: TargetSight| {
                process(
                    &mut server,
                    &lobby,
                    &raid,
                    &participants,
                    &sight,
                    &mut endpoint,
                );
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

        let shared = CompartmentId(1 << 63);
        let entities = [7, 8].map(|client| app.world().resource::<Lobby>().players[&client]);
        for entity in entities {
            app.world_mut().entity_mut(entity).insert(OpenContainer {
                raid_id: 2,
                id: shared,
                size: Footprint::new(4, 4).unwrap(),
            });
        }
        app.update();
        for updates in [pull(&mut app, 7, &mut owner), pull(&mut app, 8, &mut other)] {
            assert!(
                matches!(&updates[0].body, UpdateBody::Snapshot(s) if s.version.id == shared && s.items.is_empty())
            );
        }
        let shared_location = GridLocation {
            compartment: shared,
            ..location
        };
        let transfer = InventoryOperation {
            id: 2,
            expected: [
                VersionedCompartment { id, revision: 1 },
                VersionedCompartment {
                    id: shared,
                    revision: 0,
                },
            ],
            action: InventoryAction::Move {
                item,
                destination: shared_location,
            },
            ..request
        };
        push(
            &mut app,
            7,
            &mut owner,
            &[InventoryRequest::Operate(transfer)],
        );
        app.update();
        let updates = pull(&mut app, 7, &mut owner);
        assert!(matches!(&updates[0].body, UpdateBody::Delta(d) if d.len() == 2));
        let updates = pull(&mut app, 8, &mut other);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].session, initial_other[0].session);
        assert!(matches!(&updates[0].body, UpdateBody::Delta(d)
            if d.len() == 1 && d[0].version.id == shared && d[0].items[0].id == item));

        app.world_mut()
            .entity_mut(entities[1])
            .remove::<OpenContainer>();
        push(
            &mut app,
            7,
            &mut owner,
            &[InventoryRequest::Operate(InventoryOperation {
                id: 3,
                expected: [VersionedCompartment {
                    id: shared,
                    revision: 1,
                }; 2],
                action: InventoryAction::Move {
                    item,
                    destination: GridLocation {
                        placement: destination.placement,
                        ..shared_location
                    },
                },
                ..request
            })],
        );
        app.update();
        let updates = pull(&mut app, 7, &mut owner);
        assert!(matches!(&updates[0].body, UpdateBody::Delta(d) if d[0].version.revision == 2));
        let updates = pull(&mut app, 8, &mut other);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].body, UpdateBody::Closed(shared));
    }
}
