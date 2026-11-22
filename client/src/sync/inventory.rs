use std::collections::{BTreeMap, btree_map::Entry};

use bevy::prelude::*;
use bevy_renet2::prelude::RenetClient;
use common::{
    catalog::{
        CompartmentId,
        item_inventory::{InventoryReplica, ReplicaUpdate, SessionKey},
    },
    replication::inventory::{CHANNEL, InventoryRequest, InventoryUpdate, UpdateBody},
};

#[derive(Default, Resource)]
pub(super) struct State {
    session: Option<SessionKey>,
    replica: Option<InventoryReplica>,
    // False until enqueued. The reliable channel handles packet retransmission.
    resync: BTreeMap<CompartmentId, bool>,
}

impl State {
    fn apply(&mut self, update: InventoryUpdate) {
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
                    resync: BTreeMap::new(),
                };
            }
            return;
        }
        if self.session != Some(key) {
            return;
        }
        let Some(replica) = self.replica.as_mut() else {
            return;
        };
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
            // An operation acknowledgment is never a confirmed item update.
            UpdateBody::Result { .. } | UpdateBody::Begin => {}
        }
    }
}

pub(super) fn receive(client: Option<ResMut<RenetClient>>, mut state: ResMut<State>) {
    let Some(mut client) = client.filter(|client| client.is_connected()) else {
        if state.session.is_some() {
            *state = State::default();
        }
        return;
    };
    for _ in 0..32 {
        let Some(bytes) = client.receive_message(CHANNEL) else {
            break;
        };
        if let Ok(update) = InventoryUpdate::decode(&bytes) {
            state.apply(update);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_renet2::prelude::RenetServer;
    use common::catalog::{
        ItemInstanceId,
        item_inventory::{CompartmentDelta, InventorySnapshot, ItemDelta, VersionedCompartment},
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
        app.world_mut().remove_resource::<RenetClient>();
        app.update();
        assert!(app.world().resource::<State>().replica.is_none());
    }
}
