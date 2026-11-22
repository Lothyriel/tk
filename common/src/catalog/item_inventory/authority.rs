//! Server-side revision and replay state. It is not part of an inventory archive.
use super::*;
use std::collections::VecDeque;

const REPLAY_WINDOW: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct SessionKey {
    pub raid_epoch: u64,
    pub session_epoch: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct VersionedCompartment {
    pub id: CompartmentId,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct InventoryOperation {
    pub session: SessionKey,
    pub id: u64,
    /// Expected revisions in source/destination order. For an operation within
    /// one compartment the two revisions must agree.
    pub expected: [VersionedCompartment; 2],
    pub action: InventoryAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum OperationResult {
    Committed,
    Rejected(TransactionError),
    Conflict,
    WrongSession,
    OutOfOrder,
    Expired,
    RevisionExhausted,
}

#[derive(Debug, PartialEq, Eq)]
pub struct OperationReply {
    pub result: OperationResult,
    /// Only a fresh successful operation produces a receipt. In particular,
    /// replaying an acknowledgement cannot publish another state change.
    pub commit: Option<VersionedCommit>,
}

/// One stream per authenticated connection. The server supplies a fresh key
/// on reconnect or raid transition; network input cannot select this state.
pub struct OperationSession {
    key: SessionKey,
    last: u64,
    results: VecDeque<OperationResult>,
}

impl OperationSession {
    pub fn new(key: SessionKey) -> Self {
        Self {
            key,
            last: 0,
            results: VecDeque::with_capacity(REPLAY_WINDOW),
        }
    }
}

pub struct InventoryAuthority {
    raid_epoch: u64,
    inventory: Inventory,
    revisions: BTreeMap<CompartmentId, u64>,
}

impl InventoryAuthority {
    pub fn new(raid_epoch: u64, inventory: Inventory) -> Self {
        Self {
            raid_epoch,
            inventory,
            revisions: BTreeMap::new(),
        }
    }

    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }

    pub fn register_compartment(
        &mut self,
        id: CompartmentId,
        size: Footprint,
    ) -> Result<(), InventoryError> {
        self.inventory.register_compartment(id, size, None)
    }

    /// Do not discard owned items on disconnect. Their lifecycle is managed
    /// by the raid; an unused grid can be reclaimed immediately.
    pub fn remove_empty_compartment(&mut self, id: CompartmentId) {
        if !self
            .inventory
            .items
            .values()
            .any(|item| item.location.compartment == id)
        {
            self.inventory.compartments.remove(&id);
            self.revisions.remove(&id);
        }
    }

    pub fn revision(&self, id: CompartmentId) -> Option<u64> {
        self.inventory
            .compartments
            .contains_key(&id)
            .then(|| self.revisions.get(&id).copied().unwrap_or(0))
    }

    /// Commit a server-validated world pickup. The caller owns reach, visibility
    /// and actor checks. `None` means no room; the world item stays in place.
    pub fn pickup(
        &mut self,
        item: ItemInstanceId,
        source: CompartmentId,
        target: CompartmentId,
    ) -> Result<Option<VersionedCommit>, OperationResult> {
        let unavailable = OperationResult::Rejected(TransactionError::Unavailable);
        if source == target
            || !self
                .inventory
                .get(item)
                .is_some_and(|entry| entry.location.compartment == source)
        {
            return Err(unavailable);
        }
        let Some(destination) = self
            .inventory
            .first_fit(item, target)
            .map_err(|error| OperationResult::Rejected(TransactionError::Rule(error)))?
        else {
            return Ok(None);
        };
        let expected = [source, target].map(|id| VersionedCompartment {
            id,
            revision: self.revision(id).expect("validated pickup compartment"),
        });
        self.commit(
            InventoryAction::Move { item, destination },
            expected,
            |id| id == source || id == target,
        )
        .map(Some)
    }

    pub fn apply(
        &mut self,
        session: &mut OperationSession,
        request: InventoryOperation,
        may_write: impl Fn(CompartmentId) -> bool,
    ) -> OperationReply {
        let without_commit = |result| OperationReply {
            result,
            commit: None,
        };
        if request.session != session.key || request.session.raid_epoch != self.raid_epoch {
            return without_commit(OperationResult::WrongSession);
        }
        if request.id == 0 {
            return without_commit(OperationResult::Expired);
        }
        if request.id <= session.last {
            let distance = session.last - request.id;
            let result = if distance < session.results.len() as u64 {
                session.results[session.results.len() - 1 - distance as usize]
            } else {
                OperationResult::Expired
            };
            return without_commit(result);
        }
        if session.last.checked_add(1) != Some(request.id) {
            return without_commit(OperationResult::OutOfOrder);
        }
        let reply = match self.commit(request.action, request.expected, may_write) {
            Ok(commit) => OperationReply {
                result: OperationResult::Committed,
                commit: Some(commit),
            },
            Err(result) => without_commit(result),
        };
        session.last = request.id;
        if session.results.len() == REPLAY_WINDOW {
            session.results.pop_front();
        }
        session.results.push_back(reply.result);
        reply
    }

    fn commit(
        &mut self,
        action: InventoryAction,
        expected: [VersionedCompartment; 2],
        may_write: impl Fn(CompartmentId) -> bool,
    ) -> Result<VersionedCommit, OperationResult> {
        let rejected = OperationResult::Rejected;
        let scope = self
            .inventory
            .transaction_scope(action, &may_write)
            .map_err(rejected)?;
        let mut next = [0; 2];
        for (index, id) in scope.into_iter().enumerate() {
            let revision = self.revision(id).expect("validated compartment");
            if expected[index] != (VersionedCompartment { id, revision }) {
                return Err(OperationResult::Conflict);
            }
            next[index] = revision
                .checked_add(1)
                .ok_or(OperationResult::RevisionExhausted)?;
        }
        let commit = self
            .inventory
            .transact(action, |id| scope.contains(&id))
            .map_err(rejected)?;
        for (id, revision) in scope.into_iter().zip(next) {
            self.revisions.insert(id, revision);
        }
        Ok(VersionedCommit::new(self.raid_epoch, scope, next, commit))
    }
}
