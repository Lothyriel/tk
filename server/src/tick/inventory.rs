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
            Condition, GridLocation, Inventory, InventoryAction, InventoryAuthority,
            InventoryOperation, OperationReply, OperationSession, SessionKey, VersionedCommit,
            VersionedCompartment,
        },
        item_manifest::{S1_SEEDS, item_visuals, planned_items},
    },
    combat::{Arsenal, BodyHealth},
    humanoid::{PhysicalMode, PoseState, aim_lean_offset},
    raid::{
        Participant, RaidState, RaidStatus, in_extraction_zone,
        item_target::{GroundTarget, PICKUP_REACH_M, first_ground_target},
    },
    replication::inventory::{CHANNEL, GroundItem, InventoryRequest, InventoryUpdate, UpdateBody},
};

// Four requests per 125 ms per peer at 128 Hz. Leave excess in Renet's bounded
// queue rather than dropping an ordered operation and creating a sequence gap.
const REQUESTS_PER_WINDOW: u8 = 4;
const WINDOW_TICKS: u8 = 16;
const WORLD_SLOT_START: u64 = u64::MAX - S1_SEEDS.len() as u64 + 1;
const BANDAGE_SPARE_SLOT: CompartmentId = CompartmentId(WORLD_SLOT_START - 1);

#[derive(Debug)]
pub(super) struct WorldItem {
    pub id: ItemInstanceId,
    pub source: CompartmentId,
    pub(super) seed: usize,
    /// Query-only volume. Drops use swept gravity without adding solver bodies.
    pub half_extents: Vec3,
    pub center: Vec3,
    pub fall_speed: Option<f32>,
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
    ground: Vec<GroundItem>,
    focus: Option<ItemInstanceId>,
    pickup_held: bool,
}

#[derive(Default)]
pub(super) struct Endpoint {
    authority: Option<InventoryAuthority>,
    raid: Option<u32>,
    session_epoch: u64,
    window_tick: u8,
    peers: BTreeMap<u64, Peer>,
    world_items: Vec<WorldItem>,
    // Seed lineage is independent of location and stack identity. We do not
    // merge different provenance classes until attribution can track units.
    loot_ids: BTreeMap<ItemInstanceId, usize>,
}

#[cfg(test)]
impl Endpoint {
    pub(super) fn fixture_snapshot(
        &self,
        id: CompartmentId,
    ) -> common::catalog::item_inventory::InventorySnapshot {
        self.authority
            .as_ref()
            .unwrap()
            .snapshot(id, |candidate| candidate == id)
            .unwrap()
    }
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InventoryEndpoint")
            .field("raid", &self.raid)
            .field("world_items", &self.world_items)
            .finish_non_exhaustive()
    }
}

fn seed_world(inventory: &mut Inventory, items: &mut Vec<WorldItem>, bandages: u16) {
    inventory
        .register_compartment(
            BANDAGE_SPARE_SLOT,
            planned_items()["med.bandage"].footprint,
            None,
        )
        .expect("spare drop slot");
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
                if seed == "med.bandage" { bandages } else { 1 },
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
        // Keep both seeds clear of the crate silhouette and its cast shadow
        // from the natural approach, without changing the raid lighting.
        let center = Vec3::new(
            x + if index % 2 == 0 { -1.45 } else { 1.45 },
            terrain_top + half_extents.y,
            z + 0.35,
        );
        let item = WorldItem {
            id,
            source,
            seed: index,
            half_extents,
            center,
            fall_speed: None,
        };
        debug!(item = ?item.id, slot = ?item.source, bounds = ?item.half_extents, center = ?item.center, seed, "placed raid item");
        items.push(item);
    }
}

fn seed_partial_merge(inventory: &mut Inventory, items: &mut Vec<WorldItem>) {
    let bandage = &items[0];
    let id = inventory
        .insert_seed(
            "med.bandage",
            Condition::Intact,
            2,
            GridLocation {
                compartment: BANDAGE_SPARE_SLOT,
                placement: Placement {
                    x: 0,
                    y: 0,
                    rotation: Rotation::Upright,
                },
            },
        )
        .expect("second QA bandage stack fits spare slot");
    items.push(WorldItem {
        id,
        source: BANDAGE_SPARE_SLOT,
        seed: bandage.seed,
        center: bandage.center - Vec3::X * 1.5,
        half_extents: bandage.half_extents,
        fall_speed: None,
    });
}

#[cfg(test)]
#[derive(Resource)]
pub(super) struct UdpInventoryFixture(
    pub u32,
    pub Option<common::catalog::item_inventory::Inventory>,
);

/// Installed only by the explicit loopback timing fixture.
#[cfg(test)]
#[derive(Resource, Default)]
pub(super) struct ReceiveTiming(pub Vec<ReceiveSample>);

#[cfg(test)]
pub(super) struct ReceiveSample {
    pub active: bool,
    pub elapsed: std::time::Duration,
    pub requests: usize,
    pub updates: usize,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct PayloadTotals {
    request_bytes: usize,
    request_messages: usize,
    update_bytes: usize,
    update_messages: usize,
}

pub(super) struct PayloadTelemetry(Option<PayloadTotals>);

impl FromWorld for PayloadTelemetry {
    fn from_world(_: &mut World) -> Self {
        Self(std::env::var_os("TK_PLAYTEST_INVENTORY_TELEMETRY").map(|_| PayloadTotals::default()))
    }
}

fn send(
    server: &mut RenetServer,
    telemetry: &mut PayloadTelemetry,
    client: u64,
    session: SessionKey,
    body: UpdateBody,
) {
    match (InventoryUpdate { session, body }).encode() {
        Ok(bytes) if server.can_send_message(client, CHANNEL, bytes.len()) => {
            if let Some(totals) = &mut telemetry.0 {
                totals.update_bytes += bytes.len();
                totals.update_messages += 1;
            }
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
    (mut world, mut telemetry): (ResMut<super::WorldState>, Local<PayloadTelemetry>),
    #[cfg(test)] (fixture, timing): (
        Option<ResMut<UdpInventoryFixture>>,
        Option<ResMut<ReceiveTiming>>,
    ),
) {
    let state = &mut world.inventory;
    #[cfg(test)]
    if raid.mode == common::raid::MatchMode::Raid
        && let Some(mut fixture) = fixture
        && fixture.0 == raid.raid_id
        && let Some(inventory) = fixture.1.take()
    {
        assert_ne!(state.raid, Some(raid.raid_id));
        state.raid = Some(raid.raid_id);
        state.authority = Some(InventoryAuthority::new(u64::from(raid.raid_id), inventory));
        state.loot_ids.clear();
        state.peers.clear();
    }
    // Reuse the message counters without enabling payload logging for timing.
    #[cfg(test)]
    let timing = timing.map(|timing| {
        let original = telemetry.0;
        telemetry.0.get_or_insert_with(PayloadTotals::default);
        (timing, original, std::time::Instant::now())
    });
    let before = telemetry.0;
    state.sync_world(&raid);
    process(
        &mut server,
        &lobby,
        &raid,
        &participants,
        &sight,
        state,
        &mut telemetry,
    );
    #[cfg(test)]
    if let Some((mut timing, original, started)) = timing {
        let elapsed = started.elapsed();
        let previous = before.unwrap();
        let current = telemetry.0.unwrap();
        timing.0.push(ReceiveSample {
            active: raid.loot_active(),
            elapsed,
            requests: current.request_messages - previous.request_messages,
            updates: current.update_messages - previous.update_messages,
        });
        if original.is_none() {
            telemetry.0 = None;
        }
    }
    if telemetry.0 != before
        && let Some(totals) = telemetry.0
    {
        info!(
            "TK INVENTORY PAYLOAD request_bytes={} request_messages={} update_bytes={} update_messages={}",
            totals.request_bytes,
            totals.request_messages,
            totals.update_bytes,
            totals.update_messages
        );
    }
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
    pub(super) fn drop_center(
        &self,
        entity: Entity,
        half: Vec3,
        items: &[WorldItem],
    ) -> Option<Vec3> {
        let (input, root, movement, participant, pose, body, _) = self.actors.get(entity).ok()?;
        if participant.status != RaidStatus::Active
            || !body.alive()
            || !matches!(pose.mode, PhysicalMode::Standing | PhysicalMode::Stumbling)
        {
            return None;
        }
        let eye = super::simulation::neutral_eye(root, movement);
        let facing = Quat::from_rotation_y(input.camera.yaw);
        if !eye.is_finite() || !facing.is_finite() {
            return None;
        }
        let context = self.context.single().ok()?;
        let shape = Collider::cuboid(half.x, half.y, half.z);
        let filter = QueryFilter::new()
            .groups(CollisionGroups::new(Group::ALL, Group::GROUP_1))
            .exclude_collider(entity)
            .exclude_sensors();
        let options = ShapeCastOptions::with_max_time_of_impact(1.0);
        // Try straight ahead, then either side. Never teleport through a wall
        // or discard the inventory instance if no supported placement exists.
        for offset in [
            Vec3::new(0.0, 0.0, -0.95),
            Vec3::new(-0.8, 0.0, -0.55),
            Vec3::new(0.8, 0.0, -0.55),
        ] {
            let travel = facing * offset;
            if context
                .cast_shape(eye, Quat::IDENTITY, travel, &shape, options, filter)
                .is_some()
            {
                continue;
            }
            let start = eye + travel;
            let down = Vec3::NEG_Y * 2.5;
            let Some((_, hit)) =
                context.cast_shape(start, Quat::IDENTITY, down, &shape, options, filter)
            else {
                continue;
            };
            if !(0.0..=1.0).contains(&hit.time_of_impact)
                || !hit.details.is_some_and(|details| details.normal1.y > 0.9)
            {
                continue;
            }
            let center = start + down * hit.time_of_impact + Vec3::Y * 0.003;
            if items.iter().any(|item| {
                (center.xz() - item.center.xz())
                    .abs()
                    .cmplt(half.xz() + item.half_extents.xz())
                    .all()
            }) {
                continue;
            }
            return Some(start);
        }
        None
    }

    fn settle_drops(&self, items: &mut [WorldItem]) -> bool {
        let Ok(context) = self.context.single() else {
            return false;
        };
        let mut settled = false;
        let filter = QueryFilter::new()
            .groups(CollisionGroups::new(Group::ALL, Group::GROUP_1))
            .exclude_sensors();
        let dt = 1.0 / 128.0;
        for item in items {
            let Some(speed) = item.fall_speed else {
                continue;
            };
            let speed = speed + 9.81 * dt;
            let travel = Vec3::NEG_Y * (speed * dt);
            let Vec3 { x, y, z } = item.half_extents;
            let shape = Collider::cuboid(x, y, z);
            if let Some((_, hit)) = context.cast_shape(
                item.center,
                Quat::IDENTITY,
                travel,
                &shape,
                ShapeCastOptions::with_max_time_of_impact(1.0),
                filter,
            ) {
                let fraction = (hit.time_of_impact - 0.003 / travel.length()).clamp(0.0, 1.0);
                item.center += travel * fraction;
                item.fall_speed = None;
                settled = true;
            } else {
                item.center += travel;
                item.fall_speed = Some(speed);
            }
        }
        settled
    }

    fn pickup_input(&self, entity: Entity) -> Option<((u32, ItemInstanceId), bool)> {
        let (input, root, ..) = self.actors.get(entity).ok()?;
        input
            .pickup_focus
            .filter(|_| input.interact)
            .map(|request| (request, in_extraction_zone(root.translation)))
    }

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
            items
                .iter()
                .filter(|item| item.fall_speed.is_none())
                .map(|item| GroundTarget {
                    id: item.id,
                    center: item.center,
                    half_extents: item.half_extents,
                }),
        )
    }
}

impl Endpoint {
    fn empty_inventory(&self) -> Inventory {
        self.authority
            .as_ref()
            .map_or_else(Inventory::default, |authority| {
                authority.inventory().empty_successor()
            })
    }

    pub(super) fn carried(&self, raid: &RaidState, client: u64) -> u32 {
        if !raid.loot_active() || self.raid != Some(raid.raid_id) {
            return 0;
        }
        let Some(authority) = &self.authority else {
            return 0;
        };
        self.loot_ids
            .keys()
            .filter_map(|&id| authority.inventory().get(id))
            .filter(|item| item.location.compartment == CompartmentId(client))
            .map(|item| u32::from(item.quantity))
            .sum()
    }

    fn sync_world(&mut self, raid: &RaidState) {
        if self.raid != Some(raid.raid_id) || !raid.loot_active() {
            self.world_items.clear();
        }
        if raid.loot_active() && self.raid != Some(raid.raid_id) {
            let mut inventory = self.empty_inventory();
            let partial = std::env::var_os("TK_PLAYTEST_PARTIAL_MERGE").is_some();
            let bandages = if !partial
                && std::env::var("TK_PLAYTEST_GRID_SPLIT")
                    .is_ok_and(|v| matches!(v.as_str(), "1" | "2"))
            {
                3
            } else {
                2
            };
            seed_world(&mut inventory, &mut self.world_items, bandages);
            if partial {
                seed_partial_merge(&mut inventory, &mut self.world_items);
            }
            self.loot_ids = self
                .world_items
                .iter()
                .map(|item| (item.id, item.seed))
                .collect();
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
    telemetry: &mut PayloadTelemetry,
) {
    let settled = raid.loot_active() && sight.settle_drops(&mut state.world_items);
    if state.raid != Some(raid.raid_id) {
        state.loot_ids.clear();
        state.raid = Some(raid.raid_id);
        state.authority = Some(InventoryAuthority::new(
            u64::from(raid.raid_id),
            state.empty_inventory(),
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
                    ground: Vec::new(),
                    focus: None,
                    pickup_held: false,
                },
            );
            send(server, telemetry, client, key, UpdateBody::Begin);
            if server.is_connected(client) {
                send_snapshot(authority, server, telemetry, client, key, own, |_| true);
            }
        }
        let peer = state.peers.get_mut(&client).expect("connected peer");
        if server.is_connected(client) {
            let items: Vec<_> = state
                .world_items
                .iter()
                .map(|item| {
                    let seed = item.seed as u8;
                    let visual = &item_visuals()[S1_SEEDS[usize::from(seed)]];
                    GroundItem {
                        id: item.id,
                        seed,
                        root: (item.center - Vec3::from_array(visual.collider_center())).to_array(),
                    }
                })
                .collect();
            let membership_changed = peer.ground.len() != items.len()
                || peer
                    .ground
                    .iter()
                    .zip(&items)
                    .any(|(old, new)| old.id != new.id);
            // Send membership and final rest immediately, motion at 16 Hz.
            // Do not flood the reliable private transaction channel at 128 Hz.
            if (membership_changed || settled || state.window_tick.is_multiple_of(8))
                && peer.ground != items
            {
                peer.ground.clone_from(&items);
                send(
                    server,
                    telemetry,
                    client,
                    peer.key,
                    UpdateBody::Ground(items),
                );
            }
        }
        let focus = sight.focus(entity, &state.world_items);
        if peer.focus != focus && server.is_connected(client) {
            send(
                server,
                telemetry,
                client,
                peer.key,
                UpdateBody::GroundFocus(focus),
            );
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
                    raid.loot_active()
                        && participant.status == RaidStatus::Active
                        && grant.raid_id == raid.raid_id
                        && ((1_u64 << 63)..BANDAGE_SPARE_SLOT.0).contains(&grant.id.0)
                })
            });
        let open = grant.map(|grant| grant.id);
        if peer.open != open && server.is_connected(client) {
            if let Some(closed) = peer.open {
                send(
                    server,
                    telemetry,
                    client,
                    peer.key,
                    UpdateBody::Closed(closed),
                );
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
                send_snapshot(
                    authority,
                    server,
                    telemetry,
                    client,
                    peer.key,
                    grant.id,
                    |_| true,
                );
            }
        }
    }
    // Resolve at most one pickup per held request. A first commit removes the
    // world entry before any other peer can select or commit it this tick.
    for &client in &clients {
        let Some(peer) = state
            .peers
            .get_mut(&client)
            .filter(|_| server.is_connected(client))
        else {
            continue;
        };
        let request = sight.pickup_input(peer.entity);
        let already_held = std::mem::replace(&mut peer.pickup_held, request.is_some());
        let contest_evidence = request.is_some()
            && !already_held
            && std::env::var_os("TK_PLAYTEST_LOOT_CONTEST").is_some();
        if contest_evidence && let Some(((epoch, item), _)) = request {
            info!(
                "LOOT CONTEST received raid={epoch} item={} player={client}",
                item.0
            );
        }
        if let Some(((epoch, item), false)) = request
            && !already_held
            && raid.loot_active()
            && epoch == raid.raid_id
            && peer.focus == Some(item)
            && sight.focus(peer.entity, &state.world_items) == Some(item)
            && let Some(commit) = commit_ground_pickup(
                authority,
                &mut state.world_items,
                item,
                CompartmentId(client),
            )
        {
            if contest_evidence {
                info!(
                    "LOOT CONTEST committed raid={epoch} item={} player={client}",
                    item.0
                );
            }
            let deltas = commit.project(peer.key, |id| id == CompartmentId(client));
            send(
                server,
                telemetry,
                client,
                peer.key,
                UpdateBody::Delta(deltas),
            );
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
            if let Some(totals) = &mut telemetry.0 {
                totals.request_bytes += bytes.len();
                totals.request_messages += 1;
            }
            peer.remaining -= 1;
            let key = peer.key;
            let open = peer.open;
            let can_access = |id| id == CompartmentId(client) || open == Some(id);
            match InventoryRequest::decode(&bytes) {
                Ok(InventoryRequest::Drop {
                    session,
                    item,
                    revision,
                }) if session == key && raid.loot_active() => {
                    let owner = CompartmentId(client);
                    if authority.revision(owner) != Some(revision)
                        || !authority
                            .inventory()
                            .get(item)
                            .is_some_and(|entry| entry.location.compartment == owner)
                    {
                        continue;
                    }
                    let Some(&index) = state.loot_ids.get(&item) else {
                        continue;
                    };
                    let visual = &item_visuals()[S1_SEEDS[index]];
                    let half_extents = Vec3::from_array(visual.collider_half_extents());
                    let Some(center) =
                        sight.drop_center(peer.entity, half_extents, &state.world_items)
                    else {
                        continue;
                    };
                    let Some(source) = drop_slot(index, &state.world_items) else {
                        continue;
                    };
                    if let Ok(commit) = authority.drop_to_world(
                        item,
                        VersionedCompartment {
                            id: owner,
                            revision,
                        },
                        source,
                    ) {
                        state.world_items.push(WorldItem {
                            id: item,
                            source,
                            seed: index,
                            center,
                            half_extents,
                            fall_speed: Some(0.0),
                        });
                        state.world_items.sort_unstable_by_key(|entry| entry.id);
                        send(
                            server,
                            telemetry,
                            client,
                            key,
                            UpdateBody::Delta(commit.project(key, |id| id == owner)),
                        );
                    }
                }
                Ok(InventoryRequest::Resync {
                    session,
                    compartment,
                }) if session == peer.key => {
                    send_snapshot(
                        authority,
                        server,
                        telemetry,
                        client,
                        key,
                        compartment,
                        can_access,
                    );
                }
                Ok(InventoryRequest::Operate(request)) if request.session == peer.key => {
                    let active = raid.loot_active()
                        && participants
                            .get(peer.entity)
                            .is_ok_and(|(p, _)| p.status == RaidStatus::Active);
                    let reply = apply_operation(
                        authority,
                        &mut state.loot_ids,
                        &mut peer.operations,
                        request,
                        |id| active && can_access(id),
                    );
                    if let Some(commit) = reply.commit {
                        for (&recipient, view) in &state.peers {
                            if !server.is_connected(recipient) {
                                continue;
                            }
                            let deltas = commit.project(view.key, |id| {
                                id == CompartmentId(recipient) || view.open == Some(id)
                            });
                            if !deltas.is_empty() {
                                send(
                                    server,
                                    telemetry,
                                    recipient,
                                    view.key,
                                    UpdateBody::Delta(deltas),
                                );
                            }
                        }
                    }
                    if server.is_connected(client) {
                        send(
                            server,
                            telemetry,
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

fn drop_slot(seed: usize, world: &[WorldItem]) -> Option<CompartmentId> {
    let original = CompartmentId(WORLD_SLOT_START + seed as u64);
    let free = |slot| world.iter().all(|item| item.source != slot);
    if free(original) {
        Some(original)
    } else if S1_SEEDS[seed] == "med.bandage" && free(BANDAGE_SPARE_SLOT) {
        Some(BANDAGE_SPARE_SLOT)
    } else {
        None
    }
}

fn apply_operation(
    authority: &mut InventoryAuthority,
    lineage: &mut BTreeMap<ItemInstanceId, usize>,
    session: &mut OperationSession,
    request: InventoryOperation,
    can_access: impl Fn(CompartmentId) -> bool,
) -> OperationReply {
    let compatible = match request.action {
        InventoryAction::Merge { source, target } => lineage.get(&source) == lineage.get(&target),
        _ => true,
    };
    // Route rejections through the usual ledger so sequence/replay behavior
    // is unchanged and we don't disclose inaccessible item provenance.
    let reply = authority.apply(session, request, |id| compatible && can_access(id));
    if let Some(commit) = &reply.commit {
        let split_seed = match request.action {
            InventoryAction::Split { item, .. } => lineage.get(&item).copied(),
            _ => None,
        };
        for change in commit.changes() {
            if change.after.is_none() {
                lineage.remove(&change.id);
            } else if change.before.is_none()
                && let Some(seed) = split_seed
            {
                lineage.insert(change.id, seed);
            }
        }
    }
    reply
}

fn commit_ground_pickup(
    authority: &mut InventoryAuthority,
    world: &mut Vec<WorldItem>,
    item: ItemInstanceId,
    target: CompartmentId,
) -> Option<VersionedCommit> {
    let index = world.iter().position(|entry| entry.id == item)?;
    let commit = authority.pickup(item, world[index].source, target).ok()??;
    world.remove(index);
    Some(commit)
}

fn send_snapshot(
    authority: &InventoryAuthority,
    server: &mut RenetServer,
    telemetry: &mut PayloadTelemetry,
    client: u64,
    key: SessionKey,
    id: CompartmentId,
    may_read: impl Fn(CompartmentId) -> bool,
) {
    use common::catalog::item_inventory::SnapshotError;
    match authority.snapshot(id, may_read) {
        Ok(snapshot) => send(
            server,
            telemetry,
            client,
            key,
            UpdateBody::Snapshot(snapshot),
        ),
        Err(SnapshotError::Unavailable) => {
            send(server, telemetry, client, key, UpdateBody::Closed(id))
        }
        Err(SnapshotError::TooManyItems) => server.disconnect(client),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_renet2::prelude::RenetClient;
    use common::catalog::{
        item_grid::{Placement, Rotation},
        item_inventory::{InventoryAction, InventoryOperation, OperationResult},
    };
    use common::raid::RaidPhase;

    #[test]
    fn inventory_partial_merge_fixture_adds_a_distinct_same_seed_stack_only_when_requested() {
        let mut inventory = Inventory::default();
        let mut ground = Vec::new();
        seed_world(&mut inventory, &mut ground, 2);
        assert_eq!(ground.len(), S1_SEEDS.len());
        assert_eq!(ground.iter().filter(|item| item.seed == 0).count(), 1);
        let original = *inventory.get(ground[0].id).unwrap();
        assert_eq!(original.quantity, 2);

        seed_partial_merge(&mut inventory, &mut ground);
        assert_eq!(
            ground.len(),
            common::catalog::item_manifest::S1_GROUND_LIMIT
        );
        let extra = ground.last().unwrap();
        assert_eq!(extra.seed, ground[0].seed);
        assert_ne!(extra.id, ground[0].id);
        assert_ne!(extra.center, ground[0].center);
        let stack = inventory.get(extra.id).unwrap();
        assert_eq!(stack.location.compartment, BANDAGE_SPARE_SLOT);
        assert_eq!(stack.quantity, 2);
        assert_eq!(stack.contents, original.contents);
        assert_eq!(stack.condition, original.condition);
        assert_eq!(stack.definition.stack_max, 3);
        assert_eq!(inventory.get(ground[0].id), Some(&original));
    }

    #[test]
    fn inventory_payload_telemetry_counts_encoded_enqueues_only() {
        let mut server = RenetServer::new(common::data::renet_config());
        server.add_connection(7, false);
        let session = SessionKey {
            raid_epoch: 1,
            session_epoch: 1,
        };
        let mut telemetry = PayloadTelemetry(Some(PayloadTotals::default()));
        send(&mut server, &mut telemetry, 8, session, UpdateBody::Begin);
        send(&mut server, &mut telemetry, 7, session, UpdateBody::Begin);
        let mut peer = RenetClient::new(common::data::renet_config(), false);
        peer.set_connected();
        for packet in server.get_packets_to_send(7).unwrap() {
            peer.process_packet(packet);
        }
        let payload = peer.receive_message(CHANNEL).unwrap();
        assert_eq!(
            InventoryUpdate::decode(&payload).unwrap().body,
            UpdateBody::Begin
        );
        let totals = telemetry.0.unwrap();
        assert_eq!(totals.update_bytes, payload.len());
        assert_eq!(totals.update_messages, 1);
        assert_eq!(totals.request_messages, 0);
    }

    #[test]
    fn ground_pickup_removes_only_after_commit_and_only_once() {
        let mut world = Vec::new();
        let mut inventory = Inventory::default();
        seed_world(&mut inventory, &mut world, 2);
        let item = world[0].id;
        let source = world[0].source;
        let original = *inventory.get(item).unwrap();
        let [full, winner, other] = [7, 8, 9].map(CompartmentId);
        for id in [full, winner, other] {
            inventory
                .register_compartment(id, Footprint::new(1, 1).unwrap(), None)
                .unwrap();
        }
        inventory
            .insert_seed(
                "med.bandage",
                Condition::Intact,
                1,
                GridLocation {
                    compartment: full,
                    placement: Placement {
                        x: 0,
                        y: 0,
                        rotation: Rotation::Upright,
                    },
                },
            )
            .unwrap();
        let mut authority = InventoryAuthority::new(1, inventory);
        assert!(commit_ground_pickup(&mut authority, &mut world, item, full).is_none());
        assert_eq!(world[0].id, item);
        assert_eq!(authority.revision(source), Some(0));
        assert_eq!(authority.inventory().get(item), Some(&original));
        assert!(commit_ground_pickup(&mut authority, &mut world, item, winner).is_some());
        assert!(!world.iter().any(|entry| entry.id == item));
        assert!(commit_ground_pickup(&mut authority, &mut world, item, other).is_none());
        assert_eq!(
            authority
                .inventory()
                .get(item)
                .unwrap()
                .location
                .compartment,
            winner
        );
        assert_eq!(authority.revision(other), Some(0));
    }

    #[test]
    fn s1_ground_pickup_waits_for_contiguous_space_in_the_backpack() {
        let mut endpoint = Endpoint::default();
        let raid = RaidState {
            raid_id: 1,
            phase: RaidPhase::Active,
            ..default()
        };
        endpoint.sync_world(&raid);
        let [bandage, _, water, ssd, watch, daypack] =
            std::array::from_fn(|seed| endpoint.world_items[seed].id);
        let owner = CompartmentId(7);
        let key = SessionKey {
            raid_epoch: 1,
            session_epoch: 1,
        };
        let mut session = OperationSession::new(key);
        let authority = endpoint.authority.as_mut().unwrap();
        authority
            .register_compartment(owner, Footprint::new(4, 4).unwrap())
            .unwrap();
        let pickup = |endpoint: &mut Endpoint, item| {
            commit_ground_pickup(
                endpoint.authority.as_mut().unwrap(),
                &mut endpoint.world_items,
                item,
                owner,
            )
        };
        let mut sequence = 0;
        let mut move_to = |endpoint: &mut Endpoint, item, x, y| {
            let authority = endpoint.authority.as_mut().unwrap();
            sequence += 1;
            let request = InventoryOperation {
                session: key,
                id: sequence,
                expected: [VersionedCompartment {
                    id: owner,
                    revision: authority.revision(owner).unwrap(),
                }; 2],
                action: InventoryAction::Move {
                    item,
                    destination: GridLocation {
                        compartment: owner,
                        placement: Placement {
                            x,
                            y,
                            rotation: Rotation::Upright,
                        },
                    },
                },
            };
            assert_eq!(
                apply_operation(
                    authority,
                    &mut endpoint.loot_ids,
                    &mut session,
                    request,
                    |c| c == owner
                )
                .result,
                OperationResult::Committed,
            );
        };

        // The 3×3 daypack leaves an L-shaped border. Split that border
        // into isolated free cells with three ordinary 1×1 picks and moves.
        assert!(pickup(&mut endpoint, daypack).is_some());
        for (item, x, y) in [(bandage, 3, 1), (ssd, 1, 3), (watch, 3, 3)] {
            assert!(pickup(&mut endpoint, item).is_some());
            move_to(&mut endpoint, item, x, y);
        }
        assert_eq!(endpoint.carried(&raid, owner.0), 5);
        let water_slot = endpoint
            .world_items
            .iter()
            .find(|entry| entry.id == water)
            .unwrap()
            .source;
        let backpack_before = endpoint.fixture_snapshot(owner);
        let ground_before = endpoint.fixture_snapshot(water_slot);
        let original = *endpoint
            .authority
            .as_ref()
            .unwrap()
            .inventory()
            .get(water)
            .unwrap();

        assert!(pickup(&mut endpoint, water).is_none());
        assert_eq!(endpoint.fixture_snapshot(owner), backpack_before);
        assert_eq!(endpoint.fixture_snapshot(water_slot), ground_before);
        assert!(
            endpoint
                .world_items
                .iter()
                .any(|entry| entry.id == water && entry.source == water_slot)
        );
        assert_eq!(endpoint.carried(&raid, owner.0), 5);

        // Free a contiguous pair at (3,2) and (3,3), without dropping a stack.
        move_to(&mut endpoint, watch, 0, 3);
        let ready = endpoint.fixture_snapshot(owner);
        assert!(pickup(&mut endpoint, water).is_some());
        let after = endpoint.fixture_snapshot(owner);
        assert_eq!(after.version.revision, ready.version.revision + 1);
        assert!(!endpoint.world_items.iter().any(|entry| entry.id == water));
        assert!(endpoint.fixture_snapshot(water_slot).items.is_empty());
        let water_entry = endpoint
            .authority
            .as_ref()
            .unwrap()
            .inventory()
            .get(water)
            .unwrap();
        assert_eq!(water_entry.location.compartment, owner);
        assert_eq!(water_entry.definition, original.definition);
        assert_eq!(water_entry.quantity, original.quantity);
        assert_eq!(water_entry.condition, original.condition);
        assert_eq!(water_entry.contents, original.contents);
        assert_eq!(endpoint.carried(&raid, owner.0), 6);
    }

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
            assert_eq!(entry.quantity, if seed == "med.bandage" { 2 } else { 1 });
            assert_eq!(entry.location.compartment, item.source);
            let visual = &item_visuals()[seed];
            assert!(
                item.half_extents
                    .abs_diff_eq(Vec3::from_array(visual.collider_half_extents()), 1e-6)
            );
            assert!((item.center.y - item.half_extents.y - 0.1).abs() < 1e-6);
            let [crate_x, _, _] = common::map::LOOT_POSITIONS[index / 2];
            assert!((item.center.x - crate_x).abs() - item.half_extents.x >= 1.3);
        }
        endpoint.sync_world(&raid);
        assert_eq!(endpoint.world_items.len(), S1_SEEDS.len());
        let previous_ids: Vec<_> = endpoint.world_items.iter().map(|item| item.id).collect();
        raid.phase = RaidPhase::Debrief;
        endpoint.sync_world(&raid);
        assert!(endpoint.world_items.is_empty());
        raid.phase = RaidPhase::Lobby;
        endpoint.sync_world(&raid);
        assert!(endpoint.world_items.is_empty());
        raid.raid_id = 2;
        raid.phase = RaidPhase::Active;
        endpoint.sync_world(&raid);
        assert_eq!(endpoint.raid, Some(2));
        assert_eq!(endpoint.world_items.len(), S1_SEEDS.len());
        let inventory = endpoint.authority.as_ref().unwrap().inventory();
        assert!(previous_ids.iter().all(|id| inventory.get(*id).is_none()));
        for (previous, item) in previous_ids.into_iter().zip(&endpoint.world_items) {
            assert!(item.id.0 > previous.0);
            assert_eq!(
                inventory.get(item.id).unwrap().location.compartment,
                item.source
            );
        }
    }

    #[test]
    fn bandage_seed_lineage_survives_split_drop_pickup_and_merge_into_the_new_id() {
        let mut endpoint = Endpoint::default();
        let raid = RaidState {
            raid_id: 1,
            phase: RaidPhase::Active,
            ..default()
        };
        endpoint.sync_world(&raid);
        let seed = &endpoint.world_items[0];
        let id = seed.id;
        let seed_index = seed.seed;
        let original_slot = seed.source;
        let owner = CompartmentId(7);
        let authority = endpoint.authority.as_mut().unwrap();
        authority
            .register_compartment(owner, Footprint::new(4, 4).unwrap())
            .unwrap();
        let original = *authority.inventory().get(id).unwrap();
        commit_ground_pickup(authority, &mut endpoint.world_items, id, owner).unwrap();
        let key = SessionKey {
            raid_epoch: 1,
            session_epoch: 1,
        };
        let mut session = OperationSession::new(key);
        let request = |sequence, revision, action| InventoryOperation {
            session: key,
            id: sequence,
            expected: [VersionedCompartment {
                id: owner,
                revision,
            }; 2],
            action,
        };
        let split = request(
            1,
            1,
            InventoryAction::Split {
                item: id,
                quantity: 1,
                destination: GridLocation {
                    compartment: owner,
                    placement: Placement {
                        x: 1,
                        y: 0,
                        rotation: Rotation::Upright,
                    },
                },
            },
        );
        let reply = apply_operation(
            authority,
            &mut endpoint.loot_ids,
            &mut session,
            split,
            |c| c == owner,
        );
        let child = reply
            .commit
            .unwrap()
            .changes()
            .find(|change| change.before.is_none())
            .unwrap()
            .id;
        assert_eq!(endpoint.loot_ids.get(&child), Some(&seed_index));
        let replay = apply_operation(
            authority,
            &mut endpoint.loot_ids,
            &mut session,
            split,
            |c| c == owner,
        );
        assert_eq!(replay.result, OperationResult::Committed);
        assert!(replay.commit.is_none());
        assert_eq!(endpoint.carried(&raid, 7), 2);

        for (item, slot) in [(id, original_slot), (child, BANDAGE_SPARE_SLOT)] {
            assert_eq!(drop_slot(seed_index, &endpoint.world_items), Some(slot));
            let authority = endpoint.authority.as_mut().unwrap();
            let revision = authority.revision(owner).unwrap();
            authority
                .drop_to_world(
                    item,
                    VersionedCompartment {
                        id: owner,
                        revision,
                    },
                    slot,
                )
                .unwrap();
            endpoint.world_items.push(WorldItem {
                id: item,
                source: slot,
                seed: seed_index,
                half_extents: Vec3::ONE,
                center: Vec3::ZERO,
                fall_speed: None,
            });
        }
        assert_eq!(
            endpoint.world_items.len(),
            common::catalog::item_manifest::S1_GROUND_LIMIT
        );
        assert_eq!(endpoint.carried(&raid, 7), 0);
        for (index, item) in [id, child].into_iter().enumerate() {
            commit_ground_pickup(
                endpoint.authority.as_mut().unwrap(),
                &mut endpoint.world_items,
                item,
                owner,
            )
            .unwrap();
            assert_eq!(endpoint.carried(&raid, 7), index as u32 + 1);
        }
        let authority = endpoint.authority.as_mut().unwrap();
        let revision = authority.revision(owner).unwrap();
        let reply = apply_operation(
            authority,
            &mut endpoint.loot_ids,
            &mut session,
            request(
                2,
                revision,
                InventoryAction::Merge {
                    source: id,
                    target: child,
                },
            ),
            |c| c == owner,
        );
        assert_eq!(reply.result, OperationResult::Committed);
        assert!(!endpoint.loot_ids.contains_key(&id));
        assert!(authority.inventory().get(id).is_none());
        let reunited = authority.inventory().get(child).unwrap();
        assert_eq!(reunited.quantity, 2);
        assert_eq!(reunited.mass_g(), original.mass_g());
        assert_eq!(reunited.contents, original.contents);
        assert_eq!(reunited.condition, original.condition);
        assert_eq!(endpoint.carried(&raid, 7), 2);
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
                    &mut PayloadTelemetry(None),
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
