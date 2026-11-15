//! Weapon configuration is immutable per (epoch, handle). Snapshots reference
//! it; reliable messages carry the bounded tree only when it changes.
use std::collections::BTreeMap;
pub mod shots;

use rkyv::{Archive, Deserialize, Serialize};

use crate::{
    catalog::{Catalog, ResolvedWeapon, WeaponConfiguration, WeaponInstance, MAX_COMPONENTS},
    combat::Arsenal,
    data, EquipmentSlot, WeaponInstanceId,
};

pub const MAX_CONFIGURATION_BYTES: usize = 8_192;
pub const MAX_CACHED_CONFIGURATIONS: usize = 1_024;
pub const CONFIGURATION_CHANNEL: u8 = 3;

#[derive(Debug, Clone, Archive, Serialize, Deserialize)]
pub enum ConfigurationMessage {
    Epoch { epoch: u64, catalog: u64 },
    Configuration(Configuration),
    Result(CommandResult),
}

impl ConfigurationMessage {
    pub fn decode(bytes: &[u8]) -> Result<Self, ConfigurationError> {
        if bytes.len() > MAX_CONFIGURATION_BYTES {
            return Err(ConfigurationError::PayloadSize);
        }
        let message: Self = data::try_decode(bytes).map_err(|_| ConfigurationError::Archive)?;
        if let Self::Configuration(packet) = &message {
            packet.bounds()?;
        }
        Ok(message)
    }
}

/// Content compatibility marker, not an authentication/security hash. The exact
/// bundled document and generated rig contract must agree, including definitions
/// not present in a packet. Otherwise equal configuration IDs could resolve to
/// different grips or ballistic origins on either side.
pub fn catalog_fingerprint(source: &str) -> u64 {
    content_fingerprint(source, crate::catalog::LOGICAL_RIGS_SOURCE)
}

fn content_fingerprint(source: &str, rigs: &str) -> u64 {
    source
        .bytes()
        .chain([0])
        .chain(rigs.bytes())
        .fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        })
}

#[derive(Debug, Clone, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct Configuration {
    pub epoch: u64,
    pub handle: u32,
    pub catalog: u64,
    pub owner: u64,
    pub slot: EquipmentSlot,
    pub instance: WeaponInstanceId,
    pub revision: u64,
    pub weapon: WeaponConfiguration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigurationError {
    PayloadSize,
    Archive,
    Bounds,
    Epoch,
    Catalog,
    Conflict,
    Capacity,
    Invalid(crate::catalog::CatalogError),
}

fn configuration_bounds(weapon: &WeaponConfiguration) -> bool {
    !weapon.components.is_empty()
        && weapon.components.len() <= MAX_COMPONENTS
        && weapon
            .components
            .iter()
            .all(|node| node.node != 0 && !node.socket.is_empty() && node.socket.len() <= 64)
}

impl Configuration {
    /// Size is checked before aligned allocation/checked archive decoding.
    /// Semantic limits are checked before invoking the assembly resolver.
    pub fn decode(bytes: &[u8]) -> Result<Self, ConfigurationError> {
        if bytes.len() > MAX_CONFIGURATION_BYTES {
            return Err(ConfigurationError::PayloadSize);
        }
        let packet: Self = data::try_decode(bytes).map_err(|_| ConfigurationError::Archive)?;
        packet.bounds()?;
        Ok(packet)
    }

    fn bounds(&self) -> Result<(), ConfigurationError> {
        if self.epoch == 0
            || self.handle == 0
            || self.instance.0 == 0
            || !configuration_bounds(&self.weapon)
        {
            return Err(ConfigurationError::Bounds);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct CachedConfiguration {
    pub packet: Configuration,
    pub resolved: ResolvedWeapon,
}

/// Shared receive boundary used by the native client and headless transports.
/// Keep only the newest snapshot while reliable configuration catches up.
#[derive(Debug, Default)]
pub struct ConfigurationReceiver {
    pub cache: ConfigurationCache,
    pub last_result: Option<CommandResult>,
    pending: Option<crate::WorldSnapshot>,
    last_sequence: u64,
}

impl ConfigurationReceiver {
    pub fn message(
        &mut self,
        bytes: &[u8],
        catalog: &Catalog,
        fingerprint: u64,
    ) -> Result<(), ConfigurationError> {
        match ConfigurationMessage::decode(bytes)? {
            ConfigurationMessage::Epoch { epoch, catalog } => {
                if catalog != fingerprint {
                    return Err(ConfigurationError::Catalog);
                }
                if self.cache.begin_epoch(epoch)? {
                    self.last_result = None;
                    if self
                        .pending
                        .as_ref()
                        .is_some_and(|snapshot| snapshot.configuration_epoch < epoch)
                    {
                        self.pending = None;
                    }
                }
            }
            ConfigurationMessage::Configuration(packet) => {
                self.cache.apply(catalog, fingerprint, packet)?;
            }
            ConfigurationMessage::Result(result) => {
                if result.epoch != self.cache.epoch() {
                    return Err(ConfigurationError::Epoch);
                }
                if self
                    .last_result
                    .as_ref()
                    .is_none_or(|last| result.request >= last.request)
                {
                    self.last_result = Some(result);
                }
            }
        }
        Ok(())
    }

    pub fn snapshot(&mut self, snapshot: crate::WorldSnapshot) -> Result<(), ConfigurationError> {
        if snapshot.configuration_epoch == 0 || snapshot.configuration_epoch < self.cache.epoch() {
            return Err(ConfigurationError::Epoch);
        }
        if !snapshot.catalog_valid()
            || snapshot
                .players
                .iter()
                .any(|actor| actor.configuration == 0 || actor.weapon_instance.0 == 0)
        {
            return Err(ConfigurationError::Bounds);
        }
        if snapshot.sequence > self.last_sequence
            && self
                .pending
                .as_ref()
                .is_none_or(|previous| snapshot.sequence > previous.sequence)
        {
            self.pending = Some(snapshot);
        }
        Ok(())
    }

    pub fn take_ready(&mut self) -> Result<Option<crate::WorldSnapshot>, ConfigurationError> {
        let Some(snapshot) = &self.pending else {
            return Ok(None);
        };
        if snapshot.configuration_epoch != self.cache.epoch() {
            return Ok(None);
        }
        for actor in &snapshot.players {
            if self
                .cache
                .get(snapshot.configuration_epoch, actor.configuration)
                .is_none()
            {
                return Ok(None);
            }
            if !self.cache.matches(snapshot.configuration_epoch, actor) {
                self.pending = None;
                return Err(ConfigurationError::Conflict);
            }
        }
        let snapshot = self.pending.take().expect("pending snapshot");
        self.last_sequence = snapshot.sequence;
        Ok(Some(snapshot))
    }
}

/// Unknown references never allocate placeholders. Retain old revisions until
/// epoch reset so in-flight snapshots cannot accidentally use the newest tree.
#[derive(Debug, Default)]
pub struct ConfigurationCache {
    epoch: u64,
    entries: BTreeMap<u32, CachedConfiguration>,
}

impl ConfigurationCache {
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn begin_epoch(&mut self, epoch: u64) -> Result<bool, ConfigurationError> {
        if epoch == 0 || epoch < self.epoch {
            return Err(ConfigurationError::Epoch);
        }
        if epoch == self.epoch {
            return Ok(false);
        }
        self.entries.clear();
        self.epoch = epoch;
        Ok(true)
    }

    pub fn get(&self, epoch: u64, handle: u32) -> Option<&CachedConfiguration> {
        (epoch == self.epoch)
            .then(|| self.entries.get(&handle))
            .flatten()
    }

    /// A handle is not sufficient on its own: the snapshot must refer to the
    /// same actor, physical instance, slot and definition as the cached tree.
    pub fn matches(&self, epoch: u64, actor: &crate::ClientData) -> bool {
        self.get(epoch, actor.configuration).is_some_and(|entry| {
            let packet = &entry.packet;
            packet.owner == actor.id
                && packet.instance == actor.weapon_instance
                && packet.slot == actor.weapon_slot
                && packet.weapon.definition == actor.weapon_id
                && entry.resolved.class == actor.weapon
        })
    }

    pub fn apply(
        &mut self,
        catalog: &Catalog,
        fingerprint: u64,
        mut packet: Configuration,
    ) -> Result<bool, ConfigurationError> {
        packet.bounds()?;
        if packet.epoch != self.epoch {
            return Err(ConfigurationError::Epoch);
        }
        if packet.catalog != fingerprint {
            return Err(ConfigurationError::Catalog);
        }
        packet.weapon.components.sort_by_key(|node| node.node);
        if let Some(existing) = self.entries.get(&packet.handle) {
            return if existing.packet == packet {
                Ok(false)
            } else {
                Err(ConfigurationError::Conflict)
            };
        }
        for existing in self.entries.values().map(|entry| &entry.packet) {
            if existing.instance == packet.instance
                && (existing.weapon.definition != packet.weapon.definition
                    || existing.revision == packet.revision && existing.weapon != packet.weapon)
            {
                return Err(ConfigurationError::Conflict);
            }
        }
        if self.entries.len() == MAX_CACHED_CONFIGURATIONS {
            return Err(ConfigurationError::Capacity);
        }
        let resolved = catalog
            .resolve(&packet.weapon)
            .map_err(ConfigurationError::Invalid)?;
        // The complete tree and all derived properties become visible together.
        self.entries
            .insert(packet.handle, CachedConfiguration { packet, resolved });
        Ok(true)
    }
}

/// Server-side index. Capacity/exhaustion are explicit: the caller must publish
/// a new epoch and bootstrap clients before issuing any reference in that epoch.
#[derive(Debug)]
pub struct ConfigurationPublisher {
    epoch: u64,
    catalog: u64,
    entries: Vec<Configuration>,
}

impl ConfigurationPublisher {
    pub fn new(epoch: u64, catalog: u64) -> Result<Self, ConfigurationError> {
        if epoch == 0 {
            return Err(ConfigurationError::Epoch);
        }
        Ok(Self {
            epoch,
            catalog,
            entries: Vec::new(),
        })
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn entries(&self) -> &[Configuration] {
        &self.entries
    }

    pub fn reset(&mut self) -> Result<u64, ConfigurationError> {
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or(ConfigurationError::Capacity)?;
        self.entries.clear();
        Ok(self.epoch)
    }

    pub fn register(
        &mut self,
        owner: u64,
        slot: EquipmentSlot,
        instance: &WeaponInstance,
    ) -> Result<u32, ConfigurationError> {
        if let Some(existing) = self.entries.iter().find(|entry| {
            entry.owner == owner
                && entry.slot == slot
                && entry.instance == instance.id
                && entry.revision == instance.revision
        }) {
            // The hot snapshot path only compares the bounded tree; allocation
            // and canonicalization happen once on publication, not every tick.
            let weapon = &instance.configuration;
            return if existing.weapon.definition == weapon.definition
                && existing.weapon.components.len() == weapon.components.len()
                && existing
                    .weapon
                    .components
                    .iter()
                    .all(|node| weapon.components.contains(node))
            {
                Ok(existing.handle)
            } else {
                Err(ConfigurationError::Conflict)
            };
        }
        if self.entries.len() == MAX_CACHED_CONFIGURATIONS {
            return Err(ConfigurationError::Capacity);
        }
        let mut weapon = instance.configuration.clone();
        weapon.components.sort_by_key(|node| node.node);
        let packet = Configuration {
            epoch: self.epoch,
            handle: self.entries.len() as u32 + 1,
            catalog: self.catalog,
            owner,
            slot,
            instance: instance.id,
            revision: instance.revision,
            weapon,
        };
        packet.bounds()?;
        if data::encode(&packet).len() > MAX_CONFIGURATION_BYTES {
            return Err(ConfigurationError::PayloadSize);
        }
        let handle = packet.handle;
        self.entries.push(packet);
        Ok(handle)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct ConfigurationCommand {
    pub epoch: u64,
    /// Monotonic within one connection/epoch; never wrap or reuse an ID.
    pub request: u64,
    pub slot: EquipmentSlot,
    pub instance: WeaponInstanceId,
    pub expected_revision: u64,
    pub weapon: WeaponConfiguration,
    /// None conserves the physical pool. Some selects a new free lobby kit;
    /// the authenticated caller must apply exactly the same ready/phase lock.
    pub reserves: Option<Vec<crate::catalog::DefinitionId>>,
}

impl ConfigurationCommand {
    pub fn decode(bytes: &[u8]) -> Result<Self, ConfigurationError> {
        if bytes.len() > MAX_CONFIGURATION_BYTES {
            return Err(ConfigurationError::PayloadSize);
        }
        let command: Self = data::try_decode(bytes).map_err(|_| ConfigurationError::Archive)?;
        if command.epoch == 0
            || command.request == 0
            || command.instance.0 == 0
            || !configuration_bounds(&command.weapon)
            || !command.reserves_bounded()
        {
            return Err(ConfigurationError::Bounds);
        }
        Ok(command)
    }

    fn reserves_bounded(&self) -> bool {
        self.reserves
            .as_ref()
            .is_none_or(|reserves| reserves.len() < crate::catalog::MAX_RETAINED_MAGAZINES)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub enum CommandOutcome {
    Applied,
    EpochMismatch,
    StaleRequest,
    RequestConflict,
    NotOwned,
    Locked,
    StaleRevision,
    InvalidConfiguration,
}

#[derive(Debug, Clone, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct CommandResult {
    pub epoch: u64,
    pub request: u64,
    pub outcome: CommandOutcome,
    pub revision: Option<u64>,
    pub reason: Option<String>,
}

/// One ledger per authenticated connection. Reliable ordered delivery permits a
/// constant-size last-result cache; older requests are rejected, never replayed.
#[derive(Debug, Default)]
pub struct CommandLedger {
    epoch: u64,
    last: Option<(ConfigurationCommand, CommandResult)>,
}

impl CommandLedger {
    pub fn execute(
        &mut self,
        epoch: u64,
        mut command: ConfigurationCommand,
        allowed: bool,
        arsenal: &mut Arsenal,
        catalog: &Catalog,
    ) -> CommandResult {
        let mut result = CommandResult {
            epoch,
            request: command.request,
            outcome: CommandOutcome::EpochMismatch,
            revision: None,
            reason: None,
        };
        if command.epoch != epoch || epoch == 0 {
            return result;
        }
        if self.epoch != epoch {
            self.epoch = epoch;
            self.last = None;
        }
        command.weapon.components.sort_by_key(|node| node.node);
        if let Some((previous, response)) = &self.last {
            if command.request == previous.request {
                if command == *previous {
                    return response.clone();
                }
                result.outcome = CommandOutcome::RequestConflict;
                return result;
            }
            if command.request < previous.request {
                result.outcome = CommandOutcome::StaleRequest;
                return result;
            }
        }
        result.outcome = if command.request == 0
            || !configuration_bounds(&command.weapon)
            || !command.reserves_bounded()
        {
            CommandOutcome::InvalidConfiguration
        } else if let Some(weapon) = arsenal
            .equipped(command.slot)
            .filter(|weapon| weapon.instance().id == command.instance)
        {
            result.revision = Some(weapon.instance().revision);
            if !allowed {
                CommandOutcome::Locked
            } else if command.expected_revision != weapon.instance().revision {
                CommandOutcome::StaleRevision
            } else {
                let applied = if let Some(reserves) = &command.reserves {
                    arsenal.select_kit(
                        catalog,
                        command.slot,
                        command.expected_revision,
                        crate::catalog::WeaponKit {
                            weapon: command.weapon.clone(),
                            reserves: reserves.clone(),
                        },
                    )
                } else {
                    arsenal.reconfigure(
                        catalog,
                        command.slot,
                        command.expected_revision,
                        command.weapon.clone(),
                    )
                };
                match applied {
                    Ok(revision) => {
                        result.revision = Some(revision);
                        CommandOutcome::Applied
                    }
                    Err(error) => {
                        result.reason = Some(error.to_string().chars().take(512).collect());
                        CommandOutcome::InvalidConfiguration
                    }
                }
            }
        } else {
            CommandOutcome::NotOwned
        };
        self.last = Some((command, result.clone()));
        result
    }
}

#[cfg(test)]
mod tests;
