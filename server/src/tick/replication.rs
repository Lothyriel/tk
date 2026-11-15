//! Reliable configuration publication and authenticated configuration commands.
use std::collections::BTreeMap;

use super::*;
use common::replication::*;

#[derive(Default)]
struct Peer {
    epoch: u64,
    sent: usize,
    ledger: CommandLedger,
}

#[derive(Resource)]
pub(super) struct Replication {
    publisher: ConfigurationPublisher,
    peers: BTreeMap<u64, Peer>,
    raid: Option<(u32, RaidPhase)>,
}

impl Default for Replication {
    fn default() -> Self {
        Self {
            publisher: ConfigurationPublisher::new(
                1,
                catalog_fingerprint(common::catalog::CATALOG_SOURCE),
            )
            .expect("initial nonzero epoch"),
            peers: BTreeMap::new(),
            raid: None,
        }
    }
}

impl Replication {
    pub fn epoch(&self) -> u64 {
        self.publisher.epoch()
    }

    pub fn forget(&mut self, client: u64) {
        self.peers.remove(&client);
    }

    pub fn synchronize_raid(&mut self, raid: &RaidState) -> Result<(), ConfigurationError> {
        if self.raid.is_some_and(|(id, phase)| {
            id != raid.raid_id || phase != RaidPhase::Lobby && raid.phase == RaidPhase::Lobby
        }) {
            self.publisher.reset()?;
        }
        self.raid = Some((raid.raid_id, raid.phase));
        Ok(())
    }

    pub fn register(&mut self, client: u64, arsenal: &Arsenal) -> Result<(), ConfigurationError> {
        for slot in [EquipmentSlot::Primary, EquipmentSlot::Sidearm] {
            if let Some(weapon) = arsenal.equipped(slot) {
                self.publisher.register(client, slot, weapon.instance())?;
            }
        }
        Ok(())
    }

    pub fn handle(&self, owner: u64, arsenal: &Arsenal) -> u32 {
        self.publisher
            .entries()
            .iter()
            .rev()
            .find(|entry| {
                entry.owner == owner
                    && entry.slot == arsenal.active
                    && entry.instance == arsenal.current().instance().id
                    && entry.revision == arsenal.current().instance().revision
            })
            .expect("all actors registered before snapshot")
            .handle
    }

    pub fn reset(&mut self) -> Result<(), ConfigurationError> {
        self.publisher.reset().map(|_| ())
    }

    pub fn publish(&mut self, server: &mut RenetServer) {
        let clients = server.clients_id();
        self.peers.retain(|client, _| clients.contains(client));
        for client in clients {
            let peer = self.peers.entry(client).or_default();
            if peer.epoch != self.publisher.epoch() {
                let bytes = data::encode(&ConfigurationMessage::Epoch {
                    epoch: self.publisher.epoch(),
                    catalog: catalog_fingerprint(common::catalog::CATALOG_SOURCE),
                });
                if !server.can_send_message(client, CONFIGURATION_CHANNEL, bytes.len()) {
                    continue;
                }
                server.send_message(client, CONFIGURATION_CHANNEL, bytes);
                peer.epoch = self.publisher.epoch();
                peer.sent = 0;
                peer.ledger = CommandLedger::default();
            }
            // Progress is a per-peer cursor. Queue pressure delays publication,
            // never silently advances past unsent configuration.
            for packet in &self.publisher.entries()[peer.sent..] {
                let bytes = data::encode(&ConfigurationMessage::Configuration(packet.clone()));
                if !server.can_send_message(client, CONFIGURATION_CHANNEL, bytes.len()) {
                    break;
                }
                server.send_message(client, CONFIGURATION_CHANNEL, bytes);
                peer.sent += 1;
            }
        }
    }
}

pub(super) fn commands(
    mut server: ResMut<RenetServer>,
    mut replication: ResMut<Replication>,
    lobby: Res<Lobby>,
    raid: Res<RaidState>,
    mut players: Query<(&Participant, &mut Arsenal)>,
) {
    if let Err(error) = replication.synchronize_raid(&raid) {
        error!("Configuration epoch exhausted: {error:?}");
        server.disconnect_all();
        return;
    }
    let epoch = replication.epoch();
    for client in server.clients_id() {
        for _ in 0..16 {
            // Reserve enough queue space for the reply before consuming input.
            if !server.can_send_message(client, CONFIGURATION_CHANNEL, 128) {
                break;
            }
            let Some(bytes) = server.receive_message(client, CONFIGURATION_CHANNEL) else {
                break;
            };
            let command = match ConfigurationCommand::decode(&bytes) {
                Ok(command) => command,
                Err(error) => {
                    warn!("Rejected configuration command from {client}: {error:?}");
                    continue;
                }
            };
            let Some(&entity) = lobby.players.get(&client) else {
                continue;
            };
            let Ok((participant, mut arsenal)) = players.get_mut(entity) else {
                continue;
            };
            let Some(peer) = replication.peers.get_mut(&client) else {
                continue;
            };
            // A client cannot transact before receiving the epoch bootstrap.
            if peer.epoch != epoch {
                continue;
            }
            let allowed = raid.phase == RaidPhase::Lobby && !participant.ready;
            let result = peer.ledger.execute(
                epoch,
                command,
                allowed,
                &mut arsenal,
                common::catalog::catalog(),
            );
            server.send_message(
                client,
                CONFIGURATION_CHANNEL,
                data::encode(&ConfigurationMessage::Result(result)),
            );
        }
    }
}
