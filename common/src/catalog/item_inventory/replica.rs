//! Confirmed client state; requests and acknowledgements never mutate it.
use super::{projection::MAX_SNAPSHOT_ITEMS, *};

#[derive(Debug, PartialEq, Eq)]
pub enum ReplicaUpdate {
    Applied,
    Ignored,
    Resync(Vec<CompartmentId>),
    Invalid,
}

pub struct InventoryReplica {
    session: SessionKey,
    compartments: BTreeMap<CompartmentId, InventorySnapshot>,
}

impl InventoryReplica {
    pub fn new(session: SessionKey) -> Self {
        Self {
            session,
            compartments: BTreeMap::new(),
        }
    }

    pub fn get(&self, id: CompartmentId) -> Option<&InventorySnapshot> {
        self.compartments.get(&id)
    }

    /// A close on the ordered private stream removes the entire confirmed view.
    pub fn close(&mut self, session: SessionKey, id: CompartmentId) {
        if session == self.session {
            self.compartments.remove(&id);
        }
    }

    pub fn apply_snapshot(
        &mut self,
        session: SessionKey,
        mut snapshot: InventorySnapshot,
    ) -> ReplicaUpdate {
        let version = snapshot.version;
        if session != self.session
            || self
                .get(version.id)
                .is_some_and(|old| old.version.revision >= version.revision)
        {
            return ReplicaUpdate::Ignored;
        }
        if snapshot.items.len() > MAX_SNAPSHOT_ITEMS || snapshot.size.contains(&0) {
            return ReplicaUpdate::Invalid;
        }
        snapshot.items.sort_unstable_by_key(|(id, _)| *id);
        if snapshot.items.iter().any(|(id, _)| id.0 == 0)
            || snapshot
                .items
                .windows(2)
                .any(|items| items[0].0 == items[1].0)
        {
            return ReplicaUpdate::Invalid;
        }
        self.compartments.insert(version.id, snapshot);
        ReplicaUpdate::Applied
    }

    /// Stage all compartments before exposing any part of a transfer.
    pub fn apply_deltas(
        &mut self,
        session: SessionKey,
        deltas: Vec<CompartmentDelta>,
    ) -> ReplicaUpdate {
        if session != self.session || deltas.is_empty() {
            return ReplicaUpdate::Ignored;
        }
        if deltas.len() > 2
            || (deltas.len() == 2 && deltas[0].version.id == deltas[1].version.id)
            || deltas.iter().any(|delta| {
                delta.items.is_empty()
                    || delta.items.len() > 2
                    || delta.items.iter().any(|change| change.id.0 == 0)
                    || (delta.items.len() == 2 && delta.items[0].id == delta.items[1].id)
            })
        {
            return ReplicaUpdate::Invalid;
        }
        let versions = deltas.iter().map(|delta| delta.version);
        if versions.clone().all(|version| {
            self.get(version.id)
                .is_some_and(|old| old.version.revision >= version.revision)
        }) {
            return ReplicaUpdate::Ignored;
        }
        if !versions.clone().all(|version| {
            self.get(version.id)
                .is_some_and(|old| old.version.revision.checked_add(1) == Some(version.revision))
        }) {
            return ReplicaUpdate::Resync(versions.map(|version| version.id).collect());
        }
        let mut staged = Vec::with_capacity(deltas.len());
        for delta in deltas {
            let id = delta.version.id;
            let old = &self.compartments[&id];
            let mut items: BTreeMap<_, _> = old.items.iter().cloned().collect();
            for change in delta.items {
                if let Some(item) = change.item {
                    items.insert(change.id, item);
                } else if items.remove(&change.id).is_none() {
                    return ReplicaUpdate::Invalid;
                }
            }
            if items.len() > MAX_SNAPSHOT_ITEMS {
                return ReplicaUpdate::Invalid;
            }
            staged.push((
                id,
                InventorySnapshot {
                    version: delta.version,
                    size: old.size,
                    items: items.into_iter().collect(),
                },
            ));
        }
        self.compartments.extend(staged);
        ReplicaUpdate::Applied
    }
}
