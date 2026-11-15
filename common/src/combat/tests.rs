use super::*;

// ALPHA-04/05: equipment is constrained, and every weapon has its own magazine/cadence.
#[test]
fn loadout_only_allows_primary_and_sidearm() {
    let loadout = Loadout {
        primary: WeaponId::MP5,
        ..Default::default()
    };
    assert!(loadout.allows(WeaponId::MP5));
    assert!(loadout.allows(WeaponId::M1911));
    assert!(!loadout.allows(WeaponId::M4A1));
    assert_eq!(
        Loadout {
            primary: WeaponId::M1911,
            ..loadout
        }
        .validated()
        .unwrap()
        .primary,
        WeaponId::M4A1
    );
}

#[test]
fn three_weapons_have_specified_magazines_and_fire_modes() {
    assert_eq!(WeaponKind::Rifle.spec().magazine_size, 30);
    assert_eq!(WeaponKind::Smg.spec().magazine_size, 25);
    assert_eq!(WeaponKind::Pistol.spec().magazine_size, 17);
    assert!(WeaponKind::Rifle.spec().automatic);
    assert!(WeaponKind::Smg.spec().automatic);
    assert!(!WeaponKind::Pistol.spec().automatic);
    assert!(
        WeaponKind::Smg.spec().seconds_per_shot() < WeaponKind::Rifle.spec().seconds_per_shot()
    );
    assert!(
        AmmoKind::Ap.penetration(WeaponId::M4A1.spec())
            > AmmoKind::Ball.penetration(WeaponId::M4A1.spec())
    );
    assert!(
        AmmoKind::Ap.damage(WeaponId::M4A1.spec()) < AmmoKind::Ball.damage(WeaponId::M4A1.spec())
    );
}

// ALPHA-06: energy loss is based on actual distance through the material.
#[test]
fn wood_costs_twenty_units_per_meter_and_concrete_stops() {
    assert_eq!(SurfaceKind::Wood.penetrate(30.0, 0.5), Some(20.0));
    assert_eq!(SurfaceKind::Wood.penetrate(10.0, 0.5), None);
    assert_eq!(SurfaceKind::Wood.penetrate(30.0, 2.0), None);
    assert_eq!(SurfaceKind::Concrete.penetrate(1000.0, 0.01), None);
}

// ALPHA-07: exact blunt/penetrating fractions, wear and coverage.
#[test]
fn heavy_armor_stops_low_penetration_and_wears() {
    let mut body = BodyHealth::default();
    let mut armor = ArmorState::from_loadout(Loadout {
        armor: ArmorKind::Heavy,
        helmet: false,
        ..Default::default()
    });
    body.hit(BodyPart::Thorax, 50.0, 10.0, &mut armor);
    assert_eq!(body.parts[1], 75.0);
    assert_eq!(armor.vest, 80.0);
    assert!(body.alive());
}

#[test]
fn penetrating_hit_and_uncovered_limb_take_different_damage() {
    let mut body = BodyHealth::default();
    let mut armor = ArmorState::from_loadout(Loadout {
        armor: ArmorKind::Heavy,
        ..Default::default()
    });
    body.hit(BodyPart::Thorax, 50.0, 60.0, &mut armor);
    assert_eq!(body.parts[1], 45.0);
    assert_eq!(armor.vest, 80.0);
    body.hit(BodyPart::Arms, 20.0, 1.0, &mut armor);
    assert_eq!(body.parts[2], 40.0);
    assert_eq!(armor.vest, 80.0);
}

#[test]
fn helmet_protects_head_but_not_legs_and_broken_armor_does_not_protect() {
    let mut body = BodyHealth::default();
    let mut armor = ArmorState::from_loadout(Loadout {
        helmet: true,
        ..Default::default()
    });
    body.hit(BodyPart::Head, 20.0, 1.0, &mut armor);
    assert_eq!(body.parts[0], 31.0);
    assert_eq!(armor.helmet, 32.0);
    body.hit(BodyPart::Legs, 10.0, 1.0, &mut armor);
    assert_eq!(body.parts[3], 60.0);
    assert_eq!(armor.helmet, 32.0);
    armor.helmet = 0.0;
    body.hit(BodyPart::Head, 20.0, 1.0, &mut armor);
    assert_eq!(body.parts[0], 11.0);
}

// ALPHA-08: localized health, vital death, leg penalty and limb overflow.
#[test]
fn body_has_four_parts_and_vitals_are_fatal() {
    let body = BodyHealth::default();
    assert_eq!(body.parts, [35.0, 85.0, 60.0, 70.0]);
    assert_eq!(body.total(), 250.0);
    let mut head = body.clone();
    head.hit(BodyPart::Head, 35.0, 100.0, &mut ArmorState::default());
    assert!(!head.alive());
    assert_eq!(head.parts[1], 85.0);
    let mut thorax = body;
    thorax.hit(BodyPart::Thorax, 85.0, 100.0, &mut ArmorState::default());
    assert!(!thorax.alive());
}

#[test]
fn injured_legs_slow_movement_and_limb_overflow_reaches_thorax() {
    let mut body = BodyHealth::default();
    body.hit(BodyPart::Legs, 36.0, 100.0, &mut ArmorState::default());
    assert_eq!(body.speed_factor(), 0.55);
    body.hit(BodyPart::Arms, 80.0, 100.0, &mut ArmorState::default());
    assert_eq!(body.parts[2], 0.0);
    assert_eq!(body.parts[1], 75.0);
}

#[test]
fn hit_location_distinguishes_head_thorax_arms_and_legs() {
    assert_eq!(
        BodyPart::at_local_point(Vec3::new(0.0, 0.65, 0.0)),
        BodyPart::Head
    );
    assert_eq!(BodyPart::at_local_point(Vec3::ZERO), BodyPart::Thorax);
    assert_eq!(
        BodyPart::at_local_point(Vec3::new(0.3, 0.1, 0.0)),
        BodyPart::Arms
    );
    assert_eq!(
        BodyPart::at_local_point(Vec3::new(0.0, -0.6, 0.0)),
        BodyPart::Legs
    );
}

// ALPHA-05: cadence, semi-auto trigger, manual reload and finite reserves.
#[test]
fn arsenal_enforces_cadence_and_semi_automatic_trigger() {
    let mut arsenal = Arsenal::new(Loadout::default());
    let mut input = crate::ClientInput {
        fire: true,
        fire_pressed_sequence: 1,
        ..Default::default()
    };
    assert_eq!(arsenal.step(&input, 0.0, 0.0), Some(WeaponId::M4A1));
    assert_eq!(arsenal.current().rounds(), 29);
    assert_eq!(arsenal.current().reserve(), 90);
    assert_eq!(arsenal.step(&input, 0.01, 0.01), None);
    assert_eq!(arsenal.step(&input, 0.1, 0.11), Some(WeaponId::M4A1));
    input.weapon = EquipmentSlot::Sidearm;
    input.fire_pressed_sequence = 2;
    assert_eq!(arsenal.step(&input, 0.5, 0.61), Some(WeaponId::M1911));
    assert_eq!(arsenal.step(&input, 0.5, 1.11), None);
    assert_eq!(arsenal.current().rounds(), 16);
    input.fire_pressed_sequence = 3;
    assert_eq!(arsenal.step(&input, 0.5, 1.61), Some(WeaponId::M1911));
    assert_eq!(arsenal.current().rounds(), 15);
}

#[test]
fn reload_is_manual_timed_and_cannot_create_ammo() {
    let mut arsenal = Arsenal::new(Loadout::default());
    arsenal
        .equipped_mut(EquipmentSlot::Primary)
        .unwrap()
        .set_ammunition(0, 2)
        .unwrap();
    let mut input = crate::ClientInput {
        fire: true,
        ..Default::default()
    };
    assert_eq!(arsenal.step(&input, 3.0, 3.0), None);
    assert_eq!(arsenal.current().rounds(), 0);
    input.fire = false;
    input.reload_sequence = 1;
    arsenal.step(&input, 0.0, 3.0);
    assert_eq!(arsenal.reload_remaining, 2.4);
    arsenal.step(&input, 1.0, 4.0);
    assert_eq!(arsenal.current().rounds(), 0);
    arsenal.step(&input, 2.0, 6.0);
    assert_eq!(arsenal.current().rounds(), 2);
    assert_eq!(arsenal.current().reserve(), 0);
    input.reload_sequence = 2;
    arsenal.step(&input, 0.0, 6.0);
    assert_eq!(arsenal.reload_remaining, 0.0);
}

#[test]
fn switching_cancels_reload_and_cannot_equip_unselected_primary() {
    let mut arsenal = Arsenal::new(Loadout {
        primary: WeaponId::MP5,
        ..Default::default()
    });
    assert_eq!(arsenal.current().rounds(), 25);
    assert_eq!(arsenal.current().reserve(), 75);
    let mut input = crate::ClientInput {
        weapon: EquipmentSlot::Primary,
        reload_sequence: 1,
        ..Default::default()
    };
    arsenal
        .equipped_mut(EquipmentSlot::Primary)
        .unwrap()
        .set_ammunition(2, 75)
        .unwrap();
    arsenal.step(&input, 0.0, 0.0);
    assert!(arsenal.reload_remaining > 0.0);
    input.weapon = EquipmentSlot::Sidearm;
    arsenal.step(&input, 0.1, 0.1);
    assert_eq!(arsenal.reload_remaining, 0.0);
    assert_eq!(
        arsenal.equipped(EquipmentSlot::Primary).unwrap().rounds(),
        2
    );
    // The input selects a slot: changing its lobby proposal cannot replace the
    // authoritative equipped primary during the raid.
    input.loadout.primary = WeaponId::M4A1;
    input.weapon = EquipmentSlot::Primary;
    arsenal.step(&input, 0.1, 0.2);
    assert_eq!(arsenal.current().spec().definition, WeaponId::MP5);
}

#[test]
fn catalog_variant_and_two_equipped_instances_use_slots_and_resolved_capacity() {
    use crate::catalog::{CatalogDocument, CATALOG_SOURCE};
    let mut document: CatalogDocument = ron::from_str(CATALOG_SOURCE).unwrap();
    let mut variant = document.weapons[0].clone();
    variant.id = WeaponId(4);
    variant.key = "m4a1_test_variant".into();
    document.components[0].capacity = 12;
    document.weapons.push(variant);
    let catalog = Catalog::validate("test.ron", document).unwrap();
    let mut arsenal = Arsenal::from_slots(
        &catalog,
        Loadout::default(),
        &[
            (EquipmentSlot::Primary, WeaponId(4)),
            (EquipmentSlot::Sidearm, WeaponId(4)),
        ],
    )
    .unwrap();
    let primary_id = arsenal.current().instance().id;
    let other = arsenal.equipped(EquipmentSlot::Sidearm).unwrap();
    assert_ne!(primary_id, other.instance().id);
    assert_eq!(other.rounds(), 12);
    assert_eq!(arsenal.current().spec().class, WeaponKind::Rifle);
    let mut input = ClientInput {
        fire: true,
        ..Default::default()
    };
    assert_eq!(arsenal.step(&input, 0.0, 0.0), Some(WeaponId(4)));
    assert_eq!(arsenal.current().rounds(), 11);
    input.fire = false;
    input.reload_sequence = 1;
    arsenal.step(&input, 0.0, 0.1);
    assert!(arsenal.reload_remaining > 0.0);
    input.weapon = EquipmentSlot::Sidearm;
    input.fire = true;
    assert_eq!(arsenal.step(&input, 0.2, 0.3), Some(WeaponId(4)));
    assert_eq!(arsenal.reload_remaining, 0.0);
    assert_eq!(arsenal.current().rounds(), 11);
    assert_eq!(arsenal.current().reserve(), 36);
    let primary = arsenal.equipped(EquipmentSlot::Primary).unwrap();
    assert_eq!(primary.rounds(), 11);
    assert_eq!(primary.reserve(), 36);
    assert_eq!(primary.instance().id, primary_id);
    assert!(arsenal
        .equipped_mut(EquipmentSlot::Primary)
        .unwrap()
        .set_ammunition(13, 36)
        .is_err());
}
