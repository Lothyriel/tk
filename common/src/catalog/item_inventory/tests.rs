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
