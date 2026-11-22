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
    pub(super) fn transaction_scope(
        &self,
        action: InventoryAction,
        may_write: impl Fn(CompartmentId) -> bool,
    ) -> Result<[CompartmentId; 2], TransactionError> {
        let accessible = |id| {
            self.get(id)
                .map(|item| item.location.compartment)
                .filter(|&compartment| may_write(compartment))
                .ok_or(TransactionError::Unavailable)
        };
        match action {
            InventoryAction::Move { item, destination }
            | InventoryAction::Split {
                item, destination, ..
            } => {
                let source = accessible(item)?;
                let target = destination.compartment;
                if !may_write(target) || !self.compartments.contains_key(&target) {
                    return Err(TransactionError::Unavailable);
                }
                Ok([source, target])
            }
            InventoryAction::Merge { source, target } => {
                Ok([accessible(source)?, accessible(target)?])
            }
        }
    }

    /// Compartments are the authorization boundary, including across owners.
    /// The authority must check the current permissions for every operation.
    pub fn transact(
        &mut self,
        action: InventoryAction,
        may_write: impl Fn(CompartmentId) -> bool,
    ) -> Result<InventoryCommit, TransactionError> {
        self.transaction_scope(action, may_write)?;
        let rule = TransactionError::Rule;
        let (id, before, secondary) = match action {
            InventoryAction::Move { item, destination } => {
                let before = self.item(item).map_err(rule)?;
                self.move_item(item, destination).map_err(rule)?;
                (item, before, None)
            }
            InventoryAction::Split {
                item,
                quantity,
                destination,
            } => {
                let before = self.item(item).map_err(rule)?;
                let created = self.split(item, quantity, destination).map_err(rule)?;
                (item, before, Some(self.change(created, None)))
            }
            InventoryAction::Merge { source, target } => {
                let before = self.item(source).map_err(rule)?;
                let target_before = self.item(target).map_err(rule)?;
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
