//! Recipient-scoped state for the private inventory stream.
use super::*;
use rkyv::{Archive, Deserialize, Serialize};

pub(super) const MAX_SNAPSHOT_ITEMS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct ProjectedItem {
    pub definition: String,
    pub condition: Condition,
    pub quantity: u16,
    pub placement: Placement,
    pub contents: ItemContents,
}

impl From<ItemInstance> for ProjectedItem {
    fn from(item: ItemInstance) -> Self {
        Self {
            definition: item.definition.id.into(),
            condition: item.condition,
            quantity: item.quantity,
            placement: item.location.placement,
            contents: item.contents,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct ItemDelta {
    pub id: ItemInstanceId,
    /// None removes the ID from this compartment, not necessarily from the raid.
    pub item: Option<ProjectedItem>,
}

#[derive(Debug, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct CompartmentDelta {
    pub version: VersionedCompartment,
    pub items: Vec<ItemDelta>,
}

#[derive(Debug, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct InventorySnapshot {
    pub version: VersionedCompartment,
    pub size: [u8; 2],
    pub items: Vec<(ItemInstanceId, ProjectedItem)>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SnapshotError {
    Unavailable,
    TooManyItems,
}

#[derive(Debug, PartialEq, Eq)]
pub struct VersionedCommit {
    raid_epoch: u64,
    scope: [CompartmentId; 2],
    next: [u64; 2],
    receipt: InventoryCommit,
}

impl VersionedCommit {
    pub fn changes(&self) -> impl Iterator<Item = InventoryChange> {
        std::iter::once(self.receipt.primary).chain(self.receipt.secondary)
    }

    pub(super) fn new(
        raid_epoch: u64,
        scope: [CompartmentId; 2],
        next: [u64; 2],
        receipt: InventoryCommit,
    ) -> Self {
        Self {
            raid_epoch,
            scope,
            next,
            receipt,
        }
    }

    /// Project a whole commit for one recipient using current server permissions.
    /// Consumers must deliver the returned compartments as one update.
    pub fn project(
        &self,
        recipient: SessionKey,
        may_read: impl Fn(CompartmentId) -> bool,
    ) -> Vec<CompartmentDelta> {
        if recipient.raid_epoch != self.raid_epoch {
            return Vec::new();
        }
        self.scope
            .into_iter()
            .enumerate()
            .filter(|&(index, id)| (index == 0 || id != self.scope[0]) && may_read(id))
            .map(|(index, id)| {
                let items = std::iter::once(self.receipt.primary)
                    .chain(self.receipt.secondary)
                    .filter_map(|change| {
                        let belongs = |item: &ItemInstance| item.location.compartment == id;
                        let after = change.after.filter(belongs);
                        (after.is_some() || change.before.is_some_and(|item| belongs(&item))).then(
                            || ItemDelta {
                                id: change.id,
                                item: after.map(ProjectedItem::from),
                            },
                        )
                    })
                    .collect();
                CompartmentDelta {
                    version: VersionedCompartment {
                        id,
                        revision: self.next[index],
                    },
                    items,
                }
            })
            .collect()
    }
}

impl InventoryAuthority {
    pub fn snapshot(
        &self,
        id: CompartmentId,
        may_read: impl Fn(CompartmentId) -> bool,
    ) -> Result<InventorySnapshot, SnapshotError> {
        if !may_read(id) {
            return Err(SnapshotError::Unavailable);
        }
        let inventory = self.inventory();
        let grid = inventory
            .compartments
            .get(&id)
            .ok_or(SnapshotError::Unavailable)?;
        let mut visible = inventory
            .items
            .iter()
            .filter(|(_, item)| item.location.compartment == id);
        let items: Vec<_> = visible
            .by_ref()
            .take(MAX_SNAPSHOT_ITEMS)
            .map(|(&id, &item)| (id, ProjectedItem::from(item)))
            .collect();
        if visible.next().is_some() {
            return Err(SnapshotError::TooManyItems);
        }
        Ok(InventorySnapshot {
            version: VersionedCompartment {
                id,
                revision: self.revision(id).ok_or(SnapshotError::Unavailable)?,
            },
            size: grid.size.dimensions(),
            items,
        })
    }
}
