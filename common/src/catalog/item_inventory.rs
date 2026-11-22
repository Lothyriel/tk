//! S1 inventory rules, without spawning items or granting gameplay actions.
use super::{
    item_grid::{Footprint, Placement, PlacementError},
    item_manifest::{planned_items, PlannedItem, S1_SEEDS},
    CompartmentId, ItemInstanceId,
};
use std::collections::BTreeMap;

mod archive;
pub use archive::ArchiveError;
mod authority;
pub use authority::{
    InventoryAuthority, InventoryOperation, OperationReply, OperationResult, OperationSession,
    SessionKey, VersionedCompartment,
};
mod projection;
pub use projection::{
    CompartmentDelta, InventorySnapshot, ItemDelta, ProjectedItem, SnapshotError, VersionedCommit,
};
mod replica;
pub use replica::{InventoryReplica, ReplicaUpdate};
mod transaction;
pub use transaction::{InventoryAction, InventoryChange, InventoryCommit, TransactionError};

#[derive(Clone, Copy, Debug, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum Condition {
    Intact,
    Damaged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct GridLocation {
    pub compartment: CompartmentId,
    pub placement: Placement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemInstance {
    pub definition: &'static PlannedItem<'static>,
    pub condition: Condition,
    pub quantity: u16,
    pub location: GridLocation,
    pub contents: ItemContents,
}

/// Per-unit state, independent of stack quantity. S1 does not expose use or
/// magazine loading actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum ItemContents {
    None,
    Dressing { sealed: bool },
    Water { millilitres: u16 },
    EmptyMagazine,
}

impl ItemContents {
    const WATER_CAPACITY_ML: u16 = 500;

    fn initial(definition: &PlannedItem<'_>) -> Self {
        match definition.profile {
            "med_bleed" => Self::Dressing { sealed: true },
            "food" => Self::Water {
                millilitres: Self::WATER_CAPACITY_ML,
            },
            "magazine" => Self::EmptyMagazine,
            _ => Self::None,
        }
    }
}

impl ItemInstance {
    fn seed(
        definition: &str,
        condition: Condition,
        quantity: u16,
        location: GridLocation,
    ) -> Result<Self, InventoryError> {
        let definition = planned_items()
            .get(definition)
            .filter(|item| S1_SEEDS.contains(&item.id))
            .ok_or(InventoryError::UnknownDefinition)?;
        if quantity == 0 || quantity > definition.stack_max {
            return Err(InventoryError::InvalidQuantity);
        }
        Ok(Self {
            definition,
            condition,
            quantity,
            location,
            contents: ItemContents::initial(definition),
        })
    }

    fn with_contents(mut self, contents: ItemContents) -> Result<Self, InventoryError> {
        let valid = match (self.contents, contents) {
            (ItemContents::Water { .. }, ItemContents::Water { millilitres }) => {
                millilitres <= ItemContents::WATER_CAPACITY_ML
            }
            (ItemContents::Dressing { .. }, ItemContents::Dressing { .. }) => true,
            (initial, supplied) => initial == supplied,
        };
        if !valid {
            return Err(InventoryError::InvalidContents);
        }
        self.contents = contents;
        Ok(self)
    }

    pub fn mass_g(self) -> u64 {
        let missing_g = match self.contents {
            ItemContents::Water { millilitres } => {
                u32::from(ItemContents::WATER_CAPACITY_ML.saturating_sub(millilitres))
            }
            _ => 0,
        };
        u64::from(self.quantity) * u64::from(self.definition.mass_g.saturating_sub(missing_g))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum InventoryError {
    UnknownDefinition,
    UnknownItem,
    UnknownCompartment,
    DuplicateCompartment,
    FilteredOut,
    InvalidQuantity,
    InvalidContents,
    SameItem,
    IncompatibleStack,
    StackFull,
    IdExhausted,
    Placement(PlacementError),
}

struct Compartment {
    size: Footprint,
    profile: Option<String>,
}

/// One authoritative collection. A compartment's occupancy is derived from
/// instance locations; IDs are issued monotonically within this collection.
#[derive(Default)]
pub struct Inventory {
    compartments: BTreeMap<CompartmentId, Compartment>,
    items: BTreeMap<ItemInstanceId, ItemInstance>,
    last_id: u64,
}

impl Inventory {
    pub fn register_compartment(
        &mut self,
        id: CompartmentId,
        size: Footprint,
        profile: Option<String>,
    ) -> Result<(), InventoryError> {
        use std::collections::btree_map::Entry;
        match self.compartments.entry(id) {
            Entry::Vacant(entry) => {
                entry.insert(Compartment { size, profile });
                Ok(())
            }
            Entry::Occupied(_) => Err(InventoryError::DuplicateCompartment),
        }
    }

    pub fn get(&self, id: ItemInstanceId) -> Option<&ItemInstance> {
        self.items.get(&id)
    }

    pub fn insert_seed(
        &mut self,
        definition: &str,
        condition: Condition,
        quantity: u16,
        location: GridLocation,
    ) -> Result<ItemInstanceId, InventoryError> {
        self.insert(ItemInstance::seed(
            definition, condition, quantity, location,
        )?)
    }

    pub fn insert_seed_with_contents(
        &mut self,
        definition: &str,
        condition: Condition,
        quantity: u16,
        location: GridLocation,
        contents: ItemContents,
    ) -> Result<ItemInstanceId, InventoryError> {
        self.insert(
            ItemInstance::seed(definition, condition, quantity, location)?
                .with_contents(contents)?,
        )
    }

    pub fn move_item(
        &mut self,
        id: ItemInstanceId,
        location: GridLocation,
    ) -> Result<(), InventoryError> {
        let mut item = self.item(id)?;
        item.location = location;
        self.validate_placement(item, Some(id))?;
        self.items.insert(id, item);
        Ok(())
    }

    pub fn split(
        &mut self,
        id: ItemInstanceId,
        quantity: u16,
        location: GridLocation,
    ) -> Result<ItemInstanceId, InventoryError> {
        let source = self.item(id)?;
        if quantity == 0 || quantity >= source.quantity {
            return Err(InventoryError::InvalidQuantity);
        }
        let new_id = self.insert(ItemInstance {
            quantity,
            location,
            ..source
        })?;
        self.items.get_mut(&id).unwrap().quantity -= quantity;
        Ok(new_id)
    }

    /// Move as much of the source stack as the destination can accept. A fully
    /// absorbed source is retired; an incompatible or full target changes nothing.
    pub fn merge(
        &mut self,
        source: ItemInstanceId,
        target: ItemInstanceId,
    ) -> Result<u16, InventoryError> {
        if source == target {
            return Err(InventoryError::SameItem);
        }
        let mut from = self.item(source)?;
        let mut into = self.item(target)?;
        if from.definition.id != into.definition.id
            || from.condition != into.condition
            || from.contents != into.contents
        {
            return Err(InventoryError::IncompatibleStack);
        }
        let moved = from.quantity.min(into.definition.stack_max - into.quantity);
        if moved == 0 {
            return Err(InventoryError::StackFull);
        }
        into.quantity += moved;
        from.quantity -= moved;
        self.items.insert(target, into);
        if from.quantity == 0 {
            self.items.remove(&source);
        } else {
            self.items.insert(source, from);
        }
        Ok(moved)
    }

    fn item(&self, id: ItemInstanceId) -> Result<ItemInstance, InventoryError> {
        self.get(id).copied().ok_or(InventoryError::UnknownItem)
    }

    fn insert(&mut self, item: ItemInstance) -> Result<ItemInstanceId, InventoryError> {
        self.validate_placement(item, None)?;
        let id = self
            .last_id
            .checked_add(1)
            .ok_or(InventoryError::IdExhausted)?;
        self.items.insert(ItemInstanceId(id), item);
        self.last_id = id;
        Ok(ItemInstanceId(id))
    }

    fn validate_placement(
        &self,
        item: ItemInstance,
        exclude: Option<ItemInstanceId>,
    ) -> Result<(), InventoryError> {
        let location = item.location;
        let grid = self
            .compartments
            .get(&location.compartment)
            .ok_or(InventoryError::UnknownCompartment)?;
        if grid
            .profile
            .as_deref()
            .is_some_and(|profile| profile != item.definition.profile)
        {
            return Err(InventoryError::FilteredOut);
        }
        let others = self
            .items
            .iter()
            .filter(|(id, other)| {
                Some(**id) != exclude && other.location.compartment == location.compartment
            })
            .map(|(_, other)| other.location.placement.area(other.definition.footprint));
        location
            .placement
            .area(item.definition.footprint)
            .validate(grid.size, others)
            .map_err(InventoryError::Placement)
    }
}

#[cfg(test)]
mod tests;
