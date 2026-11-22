//! Scoped inventory mutations. Only the authority supplies compartment access.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum InventoryAction {
    Move {
        item: ItemInstanceId,
        destination: GridLocation,
    },
    Split {
        item: ItemInstanceId,
        quantity: u16,
        destination: GridLocation,
    },
    Merge {
        source: ItemInstanceId,
        target: ItemInstanceId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransactionError {
    /// Absent and inaccessible instances are deliberately indistinguishable.
    Unavailable,
    Rule(InventoryError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InventoryChange {
    pub id: ItemInstanceId,
    pub before: Option<ItemInstance>,
    pub after: Option<ItemInstance>,
}

/// Internal receipt, not a network payload. A later replication layer must
/// project both sides of each change onto the recipient's authorized view.
#[derive(Debug, PartialEq, Eq)]
pub struct InventoryCommit {
    pub primary: InventoryChange,
    pub secondary: Option<InventoryChange>,
}

impl Inventory {
    /// Compartments are the authorization boundary, including across owners.
    /// The authority must check the current permissions for every operation.
    pub fn transact(
        &mut self,
        action: InventoryAction,
        may_write: impl Fn(CompartmentId) -> bool,
    ) -> Result<InventoryCommit, TransactionError> {
        let accessible = |id| {
            self.get(id)
                .copied()
                .filter(|item| may_write(item.location.compartment))
                .ok_or(TransactionError::Unavailable)
        };
        let destination_accessible = |destination: GridLocation| {
            (may_write(destination.compartment)
                && self.compartments.contains_key(&destination.compartment))
            .then_some(())
            .ok_or(TransactionError::Unavailable)
        };
        let rule = TransactionError::Rule;
        let (id, before, secondary) = match action {
            InventoryAction::Move { item, destination } => {
                let before = accessible(item)?;
                destination_accessible(destination)?;
                self.move_item(item, destination).map_err(rule)?;
                (item, before, None)
            }
            InventoryAction::Split {
                item,
                quantity,
                destination,
            } => {
                let before = accessible(item)?;
                destination_accessible(destination)?;
                let created = self.split(item, quantity, destination).map_err(rule)?;
                (item, before, Some(self.change(created, None)))
            }
            InventoryAction::Merge { source, target } => {
                let before = accessible(source)?;
                let target_before = accessible(target)?;
                self.merge(source, target).map_err(rule)?;
                (
                    source,
                    before,
                    Some(self.change(target, Some(target_before))),
                )
            }
        };
        Ok(InventoryCommit {
            primary: self.change(id, Some(before)),
            secondary,
        })
    }

    fn change(&self, id: ItemInstanceId, before: Option<ItemInstance>) -> InventoryChange {
        InventoryChange {
            id,
            before,
            after: self.get(id).copied(),
        }
    }
}
