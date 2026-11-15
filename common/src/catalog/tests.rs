use super::*;

fn document() -> CatalogDocument {
    ron::from_str(CATALOG_SOURCE).unwrap()
}

#[test]
fn catalog_migrates_baseline_properties_without_changing_balance() {
    let catalog = Catalog::parse(CATALOG_FILE, CATALOG_SOURCE).unwrap();
    for (id, key, capacity, rpm, reload, damage, penetration, velocity, automatic) in [
        (
            WeaponId::M4A1,
            "m4a1",
            30,
            600.0,
            2.4,
            34.0,
            36.0,
            715.0,
            true,
        ),
        (
            WeaponId::M1911,
            "m1911",
            17,
            400.0,
            1.5,
            26.0,
            10.0,
            375.0,
            false,
        ),
        (
            WeaponId::MP5,
            "mp5",
            25,
            850.0,
            1.9,
            23.0,
            14.0,
            420.0,
            true,
        ),
    ] {
        let spec = catalog.weapon(id).unwrap();
        assert_eq!(spec.catalog_key, key);
        assert_eq!(
            (
                spec.magazine_size,
                spec.rounds_per_minute,
                spec.reload_seconds
            ),
            (capacity, rpm, reload)
        );
        assert_eq!(
            (
                spec.damage,
                spec.penetration,
                spec.muzzle_speed,
                spec.automatic
            ),
            (damage, penetration, velocity, automatic)
        );
    }
    assert_eq!(catalog.weapon(WeaponId::M4A1).unwrap().name, "M4A1");
    assert!(catalog.weapon(WeaponId(9999)).is_none());
    assert!(catalog.configuration(WeaponId(9999)).is_err());
}

#[test]
fn catalog_rejects_duplicate_identity_across_categories_and_unknown_references() {
    type InvalidDocument = (&'static str, fn(&mut CatalogDocument));
    let cases: &[InvalidDocument] = &[
        ("weapons[1].id", |d| d.weapons[1].id = d.weapons[0].id),
        ("components[0].id", |d| {
            d.components[0].id = DefinitionId(d.weapons[0].id.0)
        }),
        ("weapons[1].key", |d| {
            d.weapons[1].key = d.weapons[0].key.clone()
        }),
        ("weapons[0].profile", |d| {
            d.weapons[0].profile = DefinitionId(9999)
        }),
        ("weapons[0].ammo", |d| {
            d.weapons[0].ammo = DefinitionId(9999)
        }),
        ("weapons[0].components[0]", |d| {
            d.weapons[0].components[0].definition = DefinitionId(9999)
        }),
        ("weapons[0].components[0]", |d| {
            d.weapons[0].components[0].definition = d.weapons[1].components[0].definition
        }),
        ("components[0].ammo", |d| {
            d.components[0].ammo = DefinitionId(9999)
        }),
        ("version", |d| d.version += 1),
    ];
    for (field, change) in cases {
        let mut document = document();
        change(&mut document);
        let err = Catalog::validate("invalid.ron", document).unwrap_err();
        assert_eq!(&err.field, field, "{err}");
        assert_eq!(err.file, "invalid.ron");
    }
}

#[test]
fn catalog_rejects_invalid_numeric_values_with_file_and_field() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 0.0] {
        let mut doc = document();
        doc.profiles[0].rounds_per_minute = value;
        let err = Catalog::validate("bad-profile.ron", doc).unwrap_err();
        assert_eq!(err.field, "profiles[0].rounds_per_minute");
        assert!(err.to_string().contains("bad-profile.ron"));
        let mut doc = document();
        doc.ammo[0].damage = value;
        assert_eq!(
            Catalog::validate("bad-ammo.ron", doc).unwrap_err().field,
            "ammo[0].damage"
        );
        let mut doc = document();
        doc.components[0].mass_kg = value;
        assert_eq!(
            Catalog::validate("bad-component.ron", doc)
                .unwrap_err()
                .field,
            "components[0].mass_kg"
        );
    }
    let source = CATALOG_SOURCE.replace("capacity: 30", "capacity: -30");
    let err = Catalog::parse("negative.ron", &source).unwrap_err();
    assert_eq!(err.file, "negative.ron");
    assert!(err.field.contains("components[0].capacity"), "{err}");
    let source = CATALOG_SOURCE.replace("damage: 34.0", "damage: NaN");
    assert_eq!(
        Catalog::parse("nan.ron", &source).unwrap_err().field,
        "ammo[0].damage"
    );
}

#[test]
fn catalog_instances_of_new_variant_have_independent_ammunition_and_stable_roundtrip_ids() {
    let mut doc = document();
    let mut variant = doc.weapons[0].clone();
    variant.id = WeaponId(4);
    variant.key = "m4a1_variant".into();
    variant.name = "An independent configuration".into();
    doc.weapons.insert(0, variant);
    let catalog = Catalog::validate("variant.ron", doc).unwrap();
    let configuration = catalog.configuration(WeaponId(4)).unwrap();
    let mut first =
        EquippedWeapon::new(&catalog, WeaponInstanceId(7001), configuration.clone()).unwrap();
    let second = EquippedWeapon::new(&catalog, WeaponInstanceId(7002), configuration).unwrap();
    assert!(first.fire());
    assert_eq!((first.rounds(), second.rounds()), (29, 30));
    first.reload();
    assert_eq!((first.rounds(), first.reserve()), (30, 89));
    assert_eq!((second.rounds(), second.reserve()), (30, 90));
    assert_eq!(first.spec().class, WeaponClass::Rifle);
    assert_eq!(catalog.weapon(WeaponId::M4A1).unwrap().catalog_key, "m4a1");
    let restored: WeaponInstance = crate::data::decode(&crate::data::encode(first.instance()));
    assert_eq!(restored.id, first.instance().id);
    assert_eq!(restored.configuration, first.instance().configuration);
    assert_eq!((restored.magazine, restored.reserve), (30, 89));
    for slot in [EquipmentSlot::Primary, EquipmentSlot::Sidearm] {
        assert_eq!(
            crate::data::decode::<EquipmentSlot>(&crate::data::encode(&slot)),
            slot
        );
    }
}
