//! Weapon configuration is immutable per (epoch, handle). Snapshots reference
//! it; reliable messages carry the bounded tree only when it changes.
use std::collections::BTreeMap;

use rkyv::{Archive, Deserialize, Serialize};

use crate::{
    catalog::{Catalog, ResolvedWeapon, WeaponConfiguration, WeaponInstance, MAX_COMPONENTS},
    combat::Arsenal,
    data, EquipmentSlot, WeaponInstanceId,
};

pub const MAX_CONFIGURATION_BYTES: usize = 8_192;
pub const MAX_CACHED_CONFIGURATIONS: usize = 1_024;

/// Content compatibility marker, not an authentication/security hash. The exact
/// bundled document must agree, including definitions not present in a packet.
pub fn catalog_fingerprint(source: &str) -> u64 {
    source.bytes().fold(0xcbf29ce484222325, |hash, byte| {
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
        let mut weapon = instance.configuration.clone();
        weapon.components.sort_by_key(|node| node.node);
        if let Some(existing) = self.entries.iter().find(|entry| {
            entry.owner == owner
                && entry.slot == slot
                && entry.instance == instance.id
                && entry.revision == instance.revision
        }) {
            return if existing.weapon == weapon {
                Ok(existing.handle)
            } else {
                Err(ConfigurationError::Conflict)
            };
        }
        if self.entries.len() == MAX_CACHED_CONFIGURATIONS {
            return Err(ConfigurationError::Capacity);
        }
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
        {
            return Err(ConfigurationError::Bounds);
        }
        Ok(command)
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
        result.outcome = if command.request == 0 || !configuration_bounds(&command.weapon) {
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
                match arsenal.reconfigure(
                    catalog,
                    command.slot,
                    command.expected_revision,
                    command.weapon.clone(),
                ) {
                    Ok(revision) => {
                        result.revision = Some(revision);
                        CommandOutcome::Applied
                    }
                    Err(_) => CommandOutcome::InvalidConfiguration,
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
