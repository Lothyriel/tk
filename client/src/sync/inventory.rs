use std::collections::{BTreeMap, btree_map::Entry};

use bevy::prelude::*;
use bevy_renet2::prelude::RenetClient;
use common::{
    catalog::{
        CompartmentId,
        item_inventory::{
            InventoryAction, InventoryOperation, InventoryReplica, OperationResult, ReplicaUpdate,
            SessionKey, VersionedCompartment,
        },
    },
    replication::inventory::{CHANNEL, GroundItem, InventoryRequest, InventoryUpdate, UpdateBody},
};

#[derive(Default, Resource)]
pub(crate) struct State {
    session: Option<SessionKey>,
    replica: Option<InventoryReplica>,
    // False until enqueued. The reliable channel handles packet retransmission.
    resync: BTreeMap<CompartmentId, bool>,
    last_operation: u64,
    pending: Option<InventoryOperation>,
    ground: Vec<GroundItem>,
    focus: Option<common::catalog::ItemInstanceId>,
}

/// Intent from the inventory UI. Compartments are in source/destination order.
#[derive(Message)]
pub(crate) struct Operate {
    pub action: InventoryAction,
    pub scope: [CompartmentId; 2],
}

#[derive(Message)]
pub(crate) struct OperationFinished {
    pub id: u64,
    pub result: OperationResult,
}

impl State {
    pub(crate) fn ground(&self) -> (&[GroundItem], Option<SessionKey>) {
        (&self.ground, self.session)
    }

    pub(crate) fn focused_ground(&self, raid: u32) -> Option<&GroundItem> {
        if self.session?.raid_epoch != u64::from(raid) {
            return None;
        }
        let focus = self.focus?;
        self.ground.iter().find(|item| item.id == focus)
    }

    pub(crate) fn apply(&mut self, update: InventoryUpdate) -> Option<OperationFinished> {
        let key = update.session;
        if matches!(update.body, UpdateBody::Begin) {
            // Begin is ordered with snapshots and deltas. Ignore repeat or
            // older keys on the same connection.
            if self.session.is_none_or(|old| {
                (key.raid_epoch, key.session_epoch) > (old.raid_epoch, old.session_epoch)
            }) {
                *self = Self {
                    session: Some(key),
                    replica: Some(InventoryReplica::new(key)),
                    ..Self::default()
                };
            }
            return None;
        }
        if self.session != Some(key) {
            return None;
        }
        let replica = self.replica.as_mut()?;
        match update.body {
            UpdateBody::Snapshot(snapshot) => {
                let id = snapshot.version.id;
                if replica.apply_snapshot(key, snapshot) != ReplicaUpdate::Invalid {
                    self.resync.remove(&id);
                }
            }
            UpdateBody::Delta(deltas) => {
                if let ReplicaUpdate::Resync(ids) = replica.apply_deltas(key, deltas) {
                    for id in ids {
                        if let Entry::Vacant(entry) = self.resync.entry(id) {
                            entry.insert(false);
                        }
                    }
                }
            }
            UpdateBody::Closed(id) => {
                replica.close(key, id);
                self.resync.remove(&id);
            }
            UpdateBody::Ground(items) => {
                let count = common::catalog::item_manifest::S1_SEEDS.len();
                if items.len() <= count
                    && items.iter().enumerate().all(|(index, item)| {
                        item.id.0 != 0
                            && usize::from(item.seed) < count
                            && Vec3::from_array(item.root).is_finite()
                            && !items[..index].iter().any(|other| other.id == item.id)
                    })
                {
                    self.ground = items;
                    self.focus = self
                        .focus
                        .filter(|id| self.ground.iter().any(|item| item.id == *id));
                }
            }
            UpdateBody::GroundFocus(focus) => {
                self.focus = focus.filter(|id| self.ground.iter().any(|item| item.id == *id));
            }
            // An acknowledgment never substitutes for a confirmed delta.
            UpdateBody::Result { id, result } => {
                if let Some(request) = self.pending.filter(|request| request.id == id) {
                    self.pending = None;
                    if matches!(result, OperationResult::Conflict | OperationResult::Expired) {
                        for version in request.expected {
                            self.resync.entry(version.id).or_insert(false);
                        }
                    }
                    return Some(OperationFinished { id, result });
                }
            }
            UpdateBody::Begin => {}
        }
        None
    }

    fn operation(&self, intent: &Operate) -> Option<InventoryOperation> {
        if self.pending.is_some() || intent.scope.iter().any(|id| self.resync.contains_key(id)) {
            return None;
        }
        let replica = self.replica.as_ref()?;
        let version = |id| -> Option<VersionedCompartment> { Some(replica.get(id)?.version) };
        Some(InventoryOperation {
            session: self.session?,
            id: self.last_operation.checked_add(1)?,
            expected: [version(intent.scope[0])?, version(intent.scope[1])?],
            action: intent.action,
        })
    }
}

pub(super) fn receive(
    client: Option<ResMut<RenetClient>>,
    mut state: ResMut<State>,
    mut intents: MessageReader<Operate>,
    mut finished: MessageWriter<OperationFinished>,
) {
    let Some(mut client) = client.filter(|client| client.is_connected()) else {
        intents.clear();
        if state.session.is_some() {
            *state = State::default();
        }
        return;
    };
    for _ in 0..32 {
        let Some(bytes) = client.receive_message(CHANNEL) else {
            break;
        };
        if let Ok(update) = InventoryUpdate::decode(&bytes)
            && let Some(result) = state.apply(update)
        {
            debug!(operation_id = result.id, outcome = ?result.result, "inventory operation finished");
            finished.write(result);
        }
    }
    if let Some(session) = state.session {
        for (&compartment, sent) in state.resync.iter_mut().filter(|(_, sent)| !**sent).take(8) {
            let request = InventoryRequest::Resync {
                session,
                compartment,
            };
            let Ok(bytes) = request.encode() else {
                break;
            };
            if !client.can_send_message(CHANNEL, bytes.len()) {
                break;
            }
            client.send_message(CHANNEL, bytes);
            *sent = true;
        }
    }
    for intent in intents.read() {
        let Some(operation) = state.operation(intent) else {
            continue;
        };
        let Ok(bytes) = InventoryRequest::Operate(operation).encode() else {
            continue;
        };
        if client.can_send_message(CHANNEL, bytes.len()) {
            client.send_message(CHANNEL, bytes);
            state.last_operation = operation.id;
            state.pending = Some(operation);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_renet2::prelude::RenetServer;
    use common::catalog::{
        ItemInstanceId,
        item_grid::{Placement, Rotation},
        item_inventory::{
            CompartmentDelta, GridLocation, InventorySnapshot, ItemDelta, VersionedCompartment,
        },
    };

    #[test]
    fn inventory_channel_requests_resync_and_accepts_confirmed_snapshot() {
        let config = common::data::renet_config();
        let mut server = RenetServer::new(config.clone());
        let mut client = RenetClient::new(config, false);
        server.add_connection(7, false);
        client.set_connected();
        let mut app = App::new();
        app.insert_resource(client)
            .init_resource::<State>()
            .add_message::<Operate>()
            .add_message::<OperationFinished>()
            .add_systems(Update, receive);
        let key = SessionKey {
            raid_epoch: 1,
            session_epoch: 2,
        };
        let id = CompartmentId(5);
        let version = |revision| VersionedCompartment { id, revision };
        let snapshot = |revision| {
            UpdateBody::Snapshot(InventorySnapshot {
                version: version(revision),
                size: [4, 4],
                items: vec![],
            })
        };
        let gap = || {
            UpdateBody::Delta(vec![CompartmentDelta {
                version: version(2),
                items: vec![ItemDelta {
                    id: ItemInstanceId(1),
                    item: None,
                }],
            }])
        };
        let deliver = |server: &mut RenetServer, app: &mut App, bodies: Vec<UpdateBody>| {
            for body in bodies {
                server.send_message(
                    7,
                    CHANNEL,
                    InventoryUpdate { session: key, body }.encode().unwrap(),
                );
            }
            for packet in server.get_packets_to_send(7).unwrap() {
                app.world_mut()
                    .resource_mut::<RenetClient>()
                    .process_packet(packet);
            }
            app.update();
        };
        deliver(
            &mut server,
            &mut app,
            vec![UpdateBody::Begin, snapshot(0), gap(), gap()],
        );
        let confirmed = |app: &App| {
            app.world()
                .resource::<State>()
                .replica
                .as_ref()
                .unwrap()
                .get(id)
                .unwrap()
                .version
        };
        assert_eq!(confirmed(&app), version(0));
        for packet in app
            .world_mut()
            .resource_mut::<RenetClient>()
            .get_packets_to_send()
        {
            server.process_packet_from(packet, 7).unwrap();
        }
        assert_eq!(
            InventoryRequest::decode(&server.receive_message(7, CHANNEL).unwrap()),
            Ok(InventoryRequest::Resync {
                session: key,
                compartment: id
            })
        );
        assert!(server.receive_message(7, CHANNEL).is_none());
        deliver(&mut server, &mut app, vec![snapshot(2), snapshot(1)]);
        assert_eq!(confirmed(&app), version(2));
        assert!(app.world().resource::<State>().resync.is_empty());
        let action = InventoryAction::Move {
            item: ItemInstanceId(1),
            destination: GridLocation {
                compartment: id,
                placement: Placement {
                    x: 1,
                    y: 0,
                    rotation: Rotation::Upright,
                },
            },
        };
        for _ in 0..2 {
            app.world_mut().write_message(Operate {
                action,
                scope: [id; 2],
            });
        }
        app.update();
        for packet in app
            .world_mut()
            .resource_mut::<RenetClient>()
            .get_packets_to_send()
        {
            server.process_packet_from(packet, 7).unwrap();
        }
        assert_eq!(
            InventoryRequest::decode(&server.receive_message(7, CHANNEL).unwrap()),
            Ok(InventoryRequest::Operate(InventoryOperation {
                session: key,
                id: 1,
                expected: [version(2); 2],
                action,
            }))
        );
        assert!(server.receive_message(7, CHANNEL).is_none());
        deliver(
            &mut server,
            &mut app,
            vec![UpdateBody::Result {
                id: 9,
                result: OperationResult::Committed,
            }],
        );
        assert!(app.world().resource::<State>().pending.is_some());
        deliver(
            &mut server,
            &mut app,
            vec![UpdateBody::Result {
                id: 1,
                result: OperationResult::Conflict,
            }],
        );
        let mut cursor = bevy::ecs::message::MessageCursor::<OperationFinished>::default();
        let mut results = cursor.read(app.world().resource::<Messages<OperationFinished>>());
        let result = results.next().unwrap();
        assert_eq!((result.id, result.result), (1, OperationResult::Conflict));
        assert!(results.next().is_none());
        assert_eq!(confirmed(&app), version(2));
        assert!(app.world().resource::<State>().pending.is_none());
        assert_eq!(app.world().resource::<State>().resync.get(&id), Some(&true));
        app.world_mut().remove_resource::<RenetClient>();
        app.update();
        assert!(app.world().resource::<State>().replica.is_none());
    }
}
