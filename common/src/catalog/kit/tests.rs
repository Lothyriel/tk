use super::*;
use crate::combat::{Arsenal, Loadout};

fn mixed_kit() -> WeaponKit {
    let mut weapon = catalog().configuration(WeaponId::M4A1).unwrap();
    weapon
        .components
        .iter_mut()
        .find(|p| p.node == 1)
        .unwrap()
        .definition = DefinitionId(120);
    WeaponKit {
        weapon,
        reserves: vec![DefinitionId(101), DefinitionId(120)],
    }
}

#[test]
fn raid_kit_mixed_magazines_conserve_rounds_through_reload_and_restore() {
    let kit = mixed_kit();
    let decoded: WeaponKit = crate::data::decode(&crate::data::encode(&kit));
    assert_eq!(decoded, kit);
    let mut weapon = EquippedWeapon::from_kit(catalog(), WeaponInstanceId(901), &decoded).unwrap();
    assert_eq!(weapon.rounds(), 20);
    assert_eq!(weapon.reserve(), 50);
    assert_eq!(weapon.mechanics().total_rounds(), 70);
    let initial = weapon.mechanics().magazines();
    assert_eq!(
        initial
            .iter()
            .map(|m| (m.capacity, m.rounds))
            .collect::<Vec<_>>(),
        [(20, 19), (30, 30), (20, 20)]
    );
    let ids: Vec<_> = initial.iter().map(|m| m.id).collect();
    assert!(weapon.fire());
    assert!(weapon.mechanics_mut().start_reload(2.0));
    weapon.advance(0.6);
    assert!(weapon.mechanics().inserted().is_none());
    let bytes = crate::data::encode(weapon.instance());
    let mut restored = EquippedWeapon::restore(catalog(), crate::data::decode(&bytes)).unwrap();
    restored.advance(2.0);
    assert_eq!(restored.spec().magazine_size, 30);
    assert_eq!(restored.rounds(), 31);
    assert_eq!(restored.mechanics().total_rounds(), 69);
    assert_eq!(restored.mechanics().inserted().unwrap().id, ids[1]);
    assert_eq!(
        restored
            .mechanics()
            .magazines()
            .iter()
            .map(|m| m.id)
            .collect::<Vec<_>>(),
        ids
    );
    assert_eq!(kit.weapon.components[0].definition, DefinitionId(120));
}

#[test]
fn raid_kit_redeploy_preserves_selection_and_allocates_disjoint_owned_equipment() {
    let kits = [
        (EquipmentSlot::Primary, mixed_kit()),
        (
            EquipmentSlot::Sidearm,
            WeaponKit::from_configuration(
                catalog(),
                catalog().configuration(WeaponId::M1911).unwrap(),
            )
            .unwrap(),
        ),
    ];
    let mut first = Arsenal::from_kits(catalog(), Loadout::default(), &kits).unwrap();
    assert!(first.equipped_mut(EquipmentSlot::Primary).unwrap().fire());
    let second = Arsenal::from_kits(catalog(), Loadout::default(), &kits).unwrap();
    let mut weapons = std::collections::BTreeSet::new();
    let mut magazines = std::collections::BTreeSet::new();
    for arsenal in [&first, &second] {
        for (slot, kit) in &kits {
            let weapon = arsenal.equipped(*slot).unwrap();
            assert!(weapons.insert(weapon.instance().id));
            assert_eq!(weapon.instance().configuration, kit.weapon);
            assert_eq!(
                weapon.mechanics().equipped(),
                *slot == EquipmentSlot::Primary
            );
            for magazine in weapon.mechanics().magazines() {
                assert!(magazines.insert(magazine.id));
            }
        }
    }
    assert_eq!(first.current().rounds(), 19);
    assert_eq!(second.current().rounds(), 20);
    assert_eq!(second.current().reserve(), 50);
    assert_eq!(
        second.equipped(EquipmentSlot::Sidearm).unwrap().rounds(),
        17
    );
}

#[test]
fn raid_kit_rejects_invalid_reserves_and_never_grants_them_during_reconfigure() {
    let mut kit = mixed_kit();
    for reserves in [
        vec![DefinitionId(120); MAX_RETAINED_MAGAZINES],
        vec![DefinitionId(999_999)],
        vec![DefinitionId(122)], // stock, not a magazine
        vec![DefinitionId(102)], // pistol magazine, wrong ammo/interface
    ] {
        kit.reserves = reserves;
        assert!(kit.resolve(catalog()).is_err());
        assert!(EquippedWeapon::from_kit(catalog(), WeaponInstanceId(902), &kit).is_err());
    }
    kit.reserves.clear();
    let mut weapon = EquippedWeapon::from_kit(catalog(), WeaponInstanceId(903), &kit).unwrap();
    assert_eq!(weapon.mechanics().magazines().len(), 1);
    assert_eq!(weapon.reserve(), 0);
    assert_eq!(weapon.mechanics().total_rounds(), 20);
    let before = crate::data::encode(weapon.instance());
    let mut candidate = kit.weapon.clone();
    candidate
        .components
        .iter_mut()
        .find(|p| p.node == 1)
        .unwrap()
        .definition = DefinitionId(101);
    assert!(weapon
        .reconfigure(catalog(), 0, candidate)
        .unwrap_err()
        .message
        .contains("not owned"));
    assert_eq!(crate::data::encode(weapon.instance()), before);
}
