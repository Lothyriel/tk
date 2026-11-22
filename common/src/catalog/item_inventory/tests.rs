use super::*;
use crate::catalog::item_grid::Rotation;

#[test]
fn item_inventory_does_not_merge_open_and_sealed_dressings() {
    let mut inventory = fixture();
    let sealed = inventory
        .insert_seed("med.bandage", Condition::Intact, 1, at(1, 0, 0))
        .unwrap();
    let opened = inventory
        .insert_seed_with_contents(
            "med.bandage",
            Condition::Intact,
            1,
            at(1, 1, 0),
            ItemContents::Dressing { sealed: false },
        )
        .unwrap();
    assert_eq!(
        inventory.merge(opened, sealed),
        Err(InventoryError::IncompatibleStack)
    );
    assert_eq!(inventory.get(opened).unwrap().quantity, 1);
    assert_eq!(inventory.get(sealed).unwrap().quantity, 1);
}

fn at(compartment: u64, x: u8, y: u8) -> GridLocation {
    GridLocation {
        compartment: CompartmentId(compartment),
        placement: Placement {
            x,
            y,
            rotation: Rotation::Upright,
        },
    }
}

fn fixture() -> Inventory {
    let mut inventory = Inventory::default();
    let size = Footprint::new(4, 4).unwrap();
    inventory
        .register_compartment(CompartmentId(1), size, None)
        .unwrap();
    inventory
        .register_compartment(CompartmentId(2), size, Some("med_bleed".into()))
        .unwrap();
    inventory
}

#[test]
fn item_inventory_replica_recovers_an_atomic_transfer_after_a_missing_delta() {
    let mut inventory = fixture();
    let from = at(1, 0, 0);
    let intermediate = at(1, 1, 0);
    let to = at(2, 1, 0);
    let item = inventory
        .insert_seed("med.bandage", Condition::Damaged, 2, from)
        .unwrap();
    let key = SessionKey {
        raid_epoch: 1,
        session_epoch: 2,
    };
    let mut session = OperationSession::new(key);
    let mut authority = InventoryAuthority::new(key.raid_epoch, inventory);
    let mut replica = InventoryReplica::new(key);
    let version = |id, revision| VersionedCompartment { id, revision };
    let a0 = version(from.compartment, 0);
    let b0 = version(to.compartment, 0);
    let a1 = version(from.compartment, 1);
    let snapshot = |authority: &InventoryAuthority, id| authority.snapshot(id, |_| true).unwrap();
    for id in [from.compartment, to.compartment] {
        assert_eq!(
            replica.apply_snapshot(key, snapshot(&authority, id)),
            ReplicaUpdate::Applied
        );
    }
    let first = InventoryOperation {
        session: key,
        id: 1,
        expected: [a0; 2],
        action: InventoryAction::Move {
            item,
            destination: intermediate,
        },
    };
    let missed = authority
        .apply(&mut session, first, |_| true)
        .commit
        .unwrap();
    let second = InventoryOperation {
        id: 2,
        expected: [a1, b0],
        action: InventoryAction::Move {
            item,
            destination: to,
        },
        ..first
    };
    let transfer = authority
        .apply(&mut session, second, |_| true)
        .commit
        .unwrap();
    let transfer_bytes = crate::data::encode(&transfer.project(key, |_| true));
    let delivered = || crate::data::try_decode::<Vec<CompartmentDelta>>(&transfer_bytes).unwrap();
    assert_eq!(
        replica.apply_deltas(key, delivered()),
        ReplicaUpdate::Resync(vec![from.compartment, to.compartment])
    );
    assert_eq!(replica.get(from.compartment).unwrap().version, a0);
    assert!(replica.get(to.compartment).unwrap().items.is_empty());
    let stale_snapshot = InventorySnapshot {
        version: a0,
        size: [4, 4],
        items: vec![],
    };
    assert_eq!(
        replica.apply_deltas(key, missed.project(key, |_| true)),
        ReplicaUpdate::Applied
    );
    assert_eq!(
        replica.apply_deltas(key, delivered()),
        ReplicaUpdate::Applied
    );
    assert_eq!(
        replica.apply_deltas(key, delivered()),
        ReplicaUpdate::Ignored
    );
    assert_eq!(
        replica.apply_snapshot(key, stale_snapshot),
        ReplicaUpdate::Ignored
    );
    for id in [from.compartment, to.compartment] {
        assert_eq!(replica.get(id), Some(&snapshot(&authority, id)));
    }
    let new_key = SessionKey {
        session_epoch: 3,
        ..key
    };
    let mut reconnected = InventoryReplica::new(new_key);
    assert_eq!(
        reconnected.apply_deltas(key, delivered()),
        ReplicaUpdate::Ignored
    );
    assert_eq!(
        reconnected.apply_snapshot(new_key, snapshot(&authority, to.compartment)),
        ReplicaUpdate::Applied
    );
    reconnected.close(key, to.compartment);
    assert!(reconnected.get(to.compartment).is_some());
    reconnected.close(new_key, to.compartment);
    assert!(reconnected.get(to.compartment).is_none());
}

#[test]
fn item_inventory_projection_is_private_and_keeps_the_commit_revision() {
    let mut inventory = fixture();
    let from = at(1, 0, 0);
    let to = at(2, 1, 0);
    let item = inventory
        .insert_seed("med.bandage", Condition::Damaged, 2, from)
        .unwrap();
    let key = SessionKey {
        raid_epoch: 1,
        session_epoch: 2,
    };
    let mut session = OperationSession::new(key);
    let mut authority = InventoryAuthority::new(key.raid_epoch, inventory);
    let request = InventoryOperation {
        session: key,
        id: 1,
        expected: [
            VersionedCompartment {
                id: from.compartment,
                revision: 0,
            },
            VersionedCompartment {
                id: to.compartment,
                revision: 0,
            },
        ],
        action: InventoryAction::Move {
            item,
            destination: to,
        },
    };
    let commit = authority
        .apply(&mut session, request, |_| true)
        .commit
        .unwrap();
    let source = commit.project(key, |id| id == from.compartment);
    assert_eq!(
        source,
        vec![CompartmentDelta {
            version: VersionedCompartment {
                id: from.compartment,
                revision: 1
            },
            items: vec![ItemDelta {
                id: item,
                item: None
            }],
        }]
    );
    let target = commit.project(key, |id| id == to.compartment);
    assert_eq!(target.len(), 1);
    let snapshot = authority
        .snapshot(to.compartment, |id| id == to.compartment)
        .unwrap();
    assert_eq!(target[0].version, snapshot.version);
    assert_eq!(target[0].items[0].item.as_ref(), Some(&snapshot.items[0].1));
    assert_eq!(snapshot.items[0].1.placement, to.placement);
    assert_eq!(snapshot.items[0].1.quantity, 2);
    assert_eq!(commit.project(key, |_| true).len(), 2);
    assert!(commit.project(key, |_| false).is_empty());
    assert!(commit
        .project(
            SessionKey {
                raid_epoch: 0,
                ..key
            },
            |_| true
        )
        .is_empty());
    assert_eq!(
        authority.snapshot(to.compartment, |_| false),
        Err(SnapshotError::Unavailable)
    );

    let next = authority
        .apply(
            &mut session,
            InventoryOperation {
                id: 2,
                expected: [snapshot.version; 2],
                action: InventoryAction::Move {
                    item,
                    destination: at(2, 0, 0),
                },
                ..request
            },
            |_| true,
        )
        .commit
        .unwrap()
        .project(key, |_| true);
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].version.revision, 2);
    assert_eq!(commit.project(key, |id| id == to.compartment), target);
    let encoded = crate::data::encode(&target);
    assert_eq!(
        crate::data::try_decode::<Vec<CompartmentDelta>>(&encoded).unwrap(),
        target
    );
}

#[test]
fn item_inventory_projection_snapshot_does_not_truncate() {
    let mut inventory = Inventory::default();
    let id = CompartmentId(1);
    inventory
        .register_compartment(id, Footprint::new(17, 16).unwrap(), None)
        .unwrap();
    for index in 0..257 {
        inventory
            .insert_seed(
                "med.bandage",
                Condition::Intact,
                1,
                at(1, (index % 17) as u8, (index / 17) as u8),
            )
            .unwrap();
    }
    let authority = InventoryAuthority::new(1, inventory);
    assert_eq!(
        authority.snapshot(id, |_| false),
        Err(SnapshotError::Unavailable)
    );
    assert_eq!(
        authority.snapshot(id, |_| true),
        Err(SnapshotError::TooManyItems)
    );
}

#[test]
fn item_inventory_operation_replays_result_without_repeating_commit() {
    let mut inventory = fixture();
    let own = at(1, 0, 0);
    let other = at(2, 0, 0);
    let item = inventory
        .insert_seed("med.bandage", Condition::Intact, 3, own)
        .unwrap();
    let key = SessionKey {
        raid_epoch: 4,
        session_epoch: 8,
    };
    let mut session = OperationSession::new(key);
    let mut rival = OperationSession::new(SessionKey {
        session_epoch: 9,
        ..key
    });
    let mut authority = InventoryAuthority::new(key.raid_epoch, inventory);
    let request = InventoryOperation {
        session: key,
        id: 1,
        expected: [
            VersionedCompartment {
                id: own.compartment,
                revision: 0,
            },
            VersionedCompartment {
                id: other.compartment,
                revision: 0,
            },
        ],
        action: InventoryAction::Split {
            item,
            quantity: 1,
            destination: other,
        },
    };
    assert_eq!(
        authority
            .apply(
                &mut session,
                InventoryOperation { id: 2, ..request },
                |_| true
            )
            .result,
        OperationResult::OutOfOrder
    );
    let committed = authority.apply(&mut session, request, |_| true);
    assert_eq!(committed.result, OperationResult::Committed);
    assert!(committed.commit.is_some());
    let replay = authority.apply(&mut session, request, |_| false);
    assert_eq!(
        replay,
        OperationReply {
            result: OperationResult::Committed,
            commit: None
        }
    );
    assert_eq!(
        authority.apply(&mut rival, request, |_| true).result,
        OperationResult::WrongSession
    );
    let stale = InventoryOperation {
        session: SessionKey {
            session_epoch: 9,
            ..key
        },
        ..request
    };
    assert_eq!(
        authority.apply(&mut rival, stale, |_| true).result,
        OperationResult::Conflict
    );
    assert_eq!(
        authority.apply(&mut rival, stale, |_| true).result,
        OperationResult::Conflict
    );
    let result = authority.apply(
        &mut session,
        InventoryOperation {
            id: 2,
            expected: [
                VersionedCompartment {
                    id: other.compartment,
                    revision: 1,
                },
                VersionedCompartment {
                    id: other.compartment,
                    revision: 1,
                },
            ],
            ..request
        },
        |_| true,
    );
    assert_eq!(result.result, OperationResult::Conflict);
    assert_eq!(authority.revision(own.compartment), Some(1));
    assert_eq!(authority.revision(other.compartment), Some(1));
    assert_eq!(authority.inventory().items.len(), 2);
    assert_eq!(
        authority
            .inventory()
            .items
            .values()
            .map(|item| item.mass_g())
            .sum::<u64>(),
        105
    );
    for id in 3..=66 {
        assert_eq!(
            authority
                .apply(&mut session, InventoryOperation { id, ..request }, |_| {
                    false
                })
                .result,
            OperationResult::Rejected(TransactionError::Unavailable)
        );
    }
    assert_eq!(
        authority.apply(&mut session, request, |_| true).result,
        OperationResult::Expired
    );
    let mut next_raid = InventoryAuthority::new(5, fixture());
    assert_eq!(
        next_raid.apply(&mut session, request, |_| true).result,
        OperationResult::WrongSession
    );
}

#[test]
fn item_inventory_transaction_checks_both_compartments_before_mutating() {
    let mut inventory = fixture();
    let own = at(1, 0, 0);
    let other = at(2, 0, 0);
    let source = inventory
        .insert_seed("med.bandage", Condition::Intact, 2, own)
        .unwrap();
    let target = inventory
        .insert_seed("med.bandage", Condition::Intact, 1, other)
        .unwrap();
    let can_write = |id| id == own.compartment;
    let initial = inventory.export_archive().unwrap();
    for action in [
        InventoryAction::Move {
            item: source,
            destination: other,
        },
        InventoryAction::Move {
            item: target,
            destination: own,
        },
        InventoryAction::Split {
            item: source,
            quantity: 1,
            destination: other,
        },
        InventoryAction::Merge { source, target },
        InventoryAction::Merge {
            source,
            target: ItemInstanceId(999),
        },
    ] {
        assert_eq!(
            inventory.transact(action, can_write),
            Err(TransactionError::Unavailable)
        );
        assert_eq!(inventory.export_archive().unwrap(), initial);
    }
    assert_eq!(
        inventory.transact(
            InventoryAction::Move {
                item: source,
                destination: other
            },
            |_| true
        ),
        Err(TransactionError::Rule(InventoryError::Placement(
            PlacementError::Occupied
        )))
    );
    assert_eq!(inventory.export_archive().unwrap(), initial);
}

#[test]
fn item_inventory_transaction_receipt_tracks_transfer_and_retirement() {
    let mut inventory = fixture();
    let own = at(1, 0, 0);
    let other = at(2, 0, 0);
    let source = inventory
        .insert_seed("med.bandage", Condition::Intact, 3, own)
        .unwrap();
    let split = inventory
        .transact(
            InventoryAction::Split {
                item: source,
                quantity: 2,
                destination: other,
            },
            |_| true,
        )
        .unwrap();
    let created = split.secondary.unwrap();
    assert_eq!(created.before, None);
    assert_eq!(created.after.unwrap().location, other);
    assert_eq!(split.primary.before.unwrap().quantity, 3);
    assert_eq!(split.primary.after.unwrap().quantity, 1);
    let moved = inventory
        .transact(
            InventoryAction::Move {
                item: created.id,
                destination: at(1, 1, 0),
            },
            |_| true,
        )
        .unwrap();
    assert_eq!(moved.primary.before.unwrap().location, other);
    assert_eq!(moved.primary.after.unwrap().location, at(1, 1, 0));
    assert_eq!(moved.secondary, None);
    let merged = inventory
        .transact(
            InventoryAction::Merge {
                source: created.id,
                target: source,
            },
            |_| true,
        )
        .unwrap();
    assert_eq!(merged.primary.before.unwrap().quantity, 2);
    assert_eq!(merged.primary.after, None);
    assert_eq!(merged.secondary.unwrap().after.unwrap().mass_g(), 105);
    assert!(inventory.get(created.id).is_none());
}

#[test]
fn item_inventory_moves_and_rotates_atomically() {
    let mut inventory = fixture();
    let water = inventory
        .insert_seed("food.water500", Condition::Intact, 1, at(1, 3, 0))
        .unwrap();
    let other = inventory
        .insert_seed("food.water500", Condition::Damaged, 1, at(1, 1, 0))
        .unwrap();
    assert_ne!(
        inventory.get(water).unwrap().condition,
        inventory.get(other).unwrap().condition
    );
    assert_eq!(
        inventory.move_item(water, at(2, 0, 0)),
        Err(InventoryError::FilteredOut)
    );
    let mut rotated = at(1, 3, 0);
    rotated.placement.rotation = Rotation::QuarterTurn;
    let original = *inventory.get(water).unwrap();
    assert_eq!(
        inventory.move_item(water, rotated),
        Err(InventoryError::Placement(PlacementError::OutsideGrid))
    );
    rotated.placement.x = 1;
    assert_eq!(
        inventory.move_item(water, rotated),
        Err(InventoryError::Placement(PlacementError::Occupied))
    );
    assert_eq!(inventory.get(water), Some(&original));
    rotated.placement.x = 2;
    inventory.move_item(water, rotated).unwrap();
    assert_eq!(inventory.get(water).unwrap().location, rotated);
    assert_eq!(inventory.get(other).unwrap().location, at(1, 1, 0));
}

#[test]
fn item_inventory_stacks_conserve_mass_and_retire_merged_ids() {
    let mut inventory = fixture();
    let a = inventory
        .insert_seed("med.bandage", Condition::Intact, 3, at(1, 0, 0))
        .unwrap();
    let damaged = inventory
        .insert_seed("med.bandage", Condition::Damaged, 1, at(1, 1, 0))
        .unwrap();
    let mass = |inv: &Inventory| inv.items.values().map(|item| item.mass_g()).sum::<u64>();
    assert_eq!(mass(&inventory), 140);
    assert_eq!(
        inventory.split(a, 1, at(1, 1, 0)),
        Err(InventoryError::Placement(PlacementError::Occupied))
    );
    assert_eq!(
        inventory.split(a, 3, at(1, 2, 0)),
        Err(InventoryError::InvalidQuantity)
    );
    let b = inventory.split(a, 2, at(2, 0, 0)).unwrap();
    assert_eq!(inventory.get(a).unwrap().quantity, 1);
    assert_eq!(
        inventory.merge(damaged, b),
        Err(InventoryError::IncompatibleStack)
    );
    let c = inventory
        .insert_seed("med.bandage", Condition::Intact, 2, at(1, 2, 0))
        .unwrap();
    assert_eq!(inventory.merge(c, b), Ok(1));
    assert_eq!(inventory.get(c).unwrap().quantity, 1);
    assert_eq!(inventory.merge(a, c), Ok(1));
    assert!(inventory.get(a).is_none());
    assert_eq!(inventory.merge(c, b), Err(InventoryError::StackFull));
    assert_eq!(mass(&inventory), 210);
    let d = inventory.split(b, 1, at(1, 0, 0)).unwrap();
    assert!(d > a && d > c);
    assert_eq!(mass(&inventory), 210);
}

#[test]
fn item_inventory_only_accepts_seed_definitions_and_bounded_quantities() {
    let mut inventory = fixture();
    for seed in S1_SEEDS {
        let mut inventory = fixture();
        let id = inventory
            .insert_seed(seed, Condition::Intact, 1, at(1, 0, 0))
            .unwrap();
        assert_eq!(inventory.get(id).unwrap().definition.id, seed);
    }
    assert_eq!(
        inventory.insert_seed("food.water1000", Condition::Intact, 1, at(1, 0, 0)),
        Err(InventoryError::UnknownDefinition)
    );
    for quantity in [0, 4] {
        assert_eq!(
            inventory.insert_seed("med.bandage", Condition::Intact, quantity, at(1, 0, 0)),
            Err(InventoryError::InvalidQuantity)
        );
    }
    assert!(inventory.items.is_empty());
}

#[test]
fn item_inventory_generated_split_move_merge_preserves_cells_and_mass() {
    for x in 0..4 {
        for y in 0..4 {
            for total in 2..=3 {
                for part in 1..total {
                    let mut inventory = fixture();
                    let source = at(1, x, y);
                    let other_grid = at(2, x, y);
                    let id = inventory
                        .insert_seed("med.bandage", Condition::Intact, total, source)
                        .unwrap();
                    let split = inventory.split(id, part, other_grid).unwrap();
                    assert_eq!(
                        inventory.move_item(split, source),
                        Err(InventoryError::Placement(PlacementError::Occupied))
                    );
                    let free = at(1, (x + 1) % 4, y);
                    inventory.move_item(split, free).unwrap();
                    let cells: BTreeMap<_, _> = inventory
                        .items
                        .values()
                        .map(|item| {
                            let location = item.location;
                            (
                                (
                                    location.compartment,
                                    location.placement.x,
                                    location.placement.y,
                                ),
                                item.quantity,
                            )
                        })
                        .collect();
                    assert_eq!(
                        cells,
                        BTreeMap::from([
                            ((CompartmentId(1), x, y), total - part),
                            ((CompartmentId(1), (x + 1) % 4, y), part),
                        ])
                    );
                    assert_eq!(
                        inventory.items.values().map(|i| i.mass_g()).sum::<u64>(),
                        35 * u64::from(total)
                    );
                    assert_eq!(inventory.merge(split, id), Ok(part));
                    assert_eq!(inventory.get(id).unwrap().quantity, total);
                    assert_eq!(inventory.get(id).unwrap().mass_g(), 35 * u64::from(total));
                    assert!(inventory.get(split).is_none());
                }
            }
        }
    }
}
