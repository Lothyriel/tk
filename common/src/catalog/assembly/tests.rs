use super::*;

fn interface(kind: InterfaceKind, standard: &str) -> Interface {
    Interface {
        kind,
        standard: standard.into(),
        size: 1,
    }
}

fn socket(key: &str, interface: Interface, accepts: Vec<ComponentKind>) -> Socket {
    let rail = interface.kind == InterfaceKind::Rail;
    Socket {
        key: key.into(),
        interface,
        accepts,
        pose: RigidPose::default(),
        positions: if rail { 16 } else { 1 },
        pitch_m: if rail { 0.01 } else { 0.0 },
        required: false,
    }
}

fn node(id: u16, definition: u32, parent: Option<u16>, socket: &str, position: u16) -> Attachment {
    Attachment {
        node: id,
        definition: DefinitionId(definition),
        parent,
        socket: socket.into(),
        position,
    }
}

fn component(id: u32, kind: ComponentKind, interface: Interface) -> ComponentDefinition {
    ComponentDefinition {
        id: DefinitionId(id),
        key: format!("fixture_{id}"),
        kind,
        ammo: DefinitionId(0),
        capacity: 0,
        mass_kg: 0.25,
        assembly: ComponentAssembly {
            interface,
            platform: Some("m4a1".into()),
            caliber: Some("556x45".into()),
            occupied_positions: 1,
            geometry: Geometry::default(),
            requires: vec![],
            excludes: vec![],
            modifiers: Modifiers::default(),
        },
    }
}

// Synthetic connector geometry, not a real-world kit or artistic acceptance.
// Expected values below come from this fixture's measured transforms/masses.
fn document() -> CatalogDocument {
    let mut doc: CatalogDocument = ron::from_str(CATALOG_SOURCE).unwrap();
    let root = &mut doc.weapons[0].assembly;
    root.mass_kg = 2.0;
    root.geometry.sockets.extend([
        socket(
            "top",
            interface(InterfaceKind::Rail, "picatinny"),
            vec![ComponentKind::Adapter, ComponentKind::IronSight],
        ),
        socket(
            "bottom",
            interface(InterfaceKind::Rail, "mlok"),
            vec![ComponentKind::Grip],
        ),
        socket(
            "barrel",
            interface(InterfaceKind::Platform, "barrel"),
            vec![ComponentKind::Barrel],
        ),
    ]);
    root.geometry.sockets[1].pose.translation = [0.0, 0.1, 0.0];
    root.geometry.sockets[2].pose.translation = [0.0, -0.1, 0.0];
    let mut magazine = doc.components[0].clone();
    magazine.id = DefinitionId(104);
    magazine.key = "fixture_magazine_20".into();
    magazine.capacity = 20;
    magazine.mass_kg = 0.1;
    let mut optic_mount = component(
        110,
        ComponentKind::Adapter,
        interface(InterfaceKind::Rail, "picatinny"),
    );
    optic_mount.assembly.occupied_positions = 2;
    optic_mount.assembly.geometry.sockets.push(socket(
        "optic",
        interface(InterfaceKind::Mount, "optic"),
        vec![ComponentKind::RedDot, ComponentKind::Scope],
    ));
    optic_mount.assembly.geometry.sockets[0].pose.translation = [0.0, 0.02, 0.0];
    let mut optic = component(
        111,
        ComponentKind::RedDot,
        interface(InterfaceKind::Mount, "optic"),
    );
    optic.assembly.geometry.anchors.push(Anchor {
        kind: AnchorKind::Sight,
        pose: RigidPose::default(),
    });
    let grip = component(
        112,
        ComponentKind::Grip,
        interface(InterfaceKind::Rail, "mlok"),
    );
    let mut barrel = component(
        113,
        ComponentKind::Barrel,
        interface(InterfaceKind::Platform, "barrel"),
    );
    barrel.assembly.geometry.sockets.push(socket(
        "thread",
        interface(InterfaceKind::Thread, "1/2-28"),
        vec![
            ComponentKind::Adapter,
            ComponentKind::Compensator,
            ComponentKind::MuzzleDevice,
        ],
    ));
    barrel.assembly.geometry.sockets[0].pose.translation = [0.0, 0.0, -0.4];
    barrel.assembly.geometry.anchors.push(Anchor {
        kind: AnchorKind::Bore,
        pose: RigidPose {
            translation: [0.0, 0.0, -0.4],
            ..Default::default()
        },
    });
    let mut muzzle_mount = component(
        114,
        ComponentKind::Adapter,
        interface(InterfaceKind::Thread, "1/2-28"),
    );
    muzzle_mount.assembly.geometry.sockets.push(socket(
        "qd",
        interface(InterfaceKind::Mount, "qd"),
        vec![ComponentKind::Suppressor],
    ));
    let mut suppressor = component(
        115,
        ComponentKind::Suppressor,
        interface(InterfaceKind::Mount, "qd"),
    );
    suppressor.assembly.requires.push(DefinitionId(113));
    suppressor.assembly.geometry.anchors.push(Anchor {
        kind: AnchorKind::DeviceExit,
        pose: RigidPose {
            translation: [0.0, 0.0, -0.2],
            ..Default::default()
        },
    });
    suppressor.assembly.modifiers = Modifiers {
        recoil: 0.8,
        velocity: 1.1,
        signature: 0.5,
    };
    doc.components.extend([
        magazine,
        optic_mount,
        optic,
        grip,
        barrel,
        muzzle_mount,
        suppressor,
    ]);
    doc
}

fn assembled() -> WeaponConfiguration {
    WeaponConfiguration {
        definition: WeaponId::M4A1,
        components: vec![
            node(1, 104, None, "magazine", 0),
            node(2, 110, None, "top", 3),
            node(3, 111, Some(2), "optic", 0),
            node(4, 112, None, "bottom", 2),
            node(5, 113, None, "barrel", 0),
            node(6, 114, Some(5), "thread", 0),
            node(7, 115, Some(6), "qd", 0),
        ],
    }
}

fn catalog_fixture() -> Catalog {
    Catalog::validate("fixture.ron", document()).unwrap()
}

#[test]
fn assembly_positive_matrix_and_independent_mass_anchor_oracles() {
    let catalog = catalog_fixture();
    // Both magazines; full adapter chains, compatible rail and bore/device
    // separation must coexist in the same accepted configuration.
    for (magazine, capacity, mass) in [(101, 30, 3.61), (104, 20, 3.6)] {
        let mut configuration = assembled();
        configuration.components[0].definition = DefinitionId(magazine);
        let resolved = catalog.resolve(&configuration).unwrap();
        assert_eq!(resolved.magazine_size, capacity);
        assert!((resolved.assembly.mass_kg - mass).abs() < 1e-6);
        assert!((resolved.muzzle_speed - 786.5).abs() < 0.001);
        assert_eq!(resolved.barrel_offset, [0.0, 0.0, -0.4]);
        let anchors = &resolved.assembly.anchors;
        assert!((anchors[&AnchorKind::DeviceExit].translation.z + 0.6).abs() < 1e-6);
        assert!(
            (anchors[&AnchorKind::Sight].translation - Vec3::new(0.0, 0.12, 0.03)).length() < 1e-6
        );
        // 6 parts x 0.25 kg; y moment = .025+.030-.025=.030;
        // z moment = .0075+.0075+.005-.1-.1=-.180 kg*m.
        assert!((resolved.assembly.center_of_mass[1] - 0.03 / mass).abs() < 1e-6);
        assert!((resolved.assembly.center_of_mass[2] + 0.18 / mass).abs() < 1e-6);
        assert!(
            (resolved.assembly.balance_kg_m - (0.03_f32.powi(2) + 0.18_f32.powi(2)).sqrt()).abs()
                < 1e-6
        );
        assert_eq!(resolved.assembly.recoil_multiplier, 0.8);
        assert_eq!(resolved.assembly.signature_multiplier, 0.5);
    }
    let baseline = catalog.weapon(WeaponId::M4A1).unwrap();
    assert_eq!(baseline.muzzle_speed, 715.0);
    assert_eq!(baseline.barrel_offset, [0.28, -0.15, -0.92]);
}

#[test]
fn assembly_rejects_independent_negative_configuration_matrix() {
    type Case = (&'static str, fn(&mut WeaponConfiguration), &'static str);
    let cases: &[Case] = &[
        (
            "missing mount",
            |c| c.components[2].parent = None,
            "missing socket",
        ),
        (
            "wrong socket interface",
            |c| {
                c.components[2].parent = None;
                c.components[2].socket = "top".into();
            },
            "interface",
        ),
        (
            "rail overlap",
            |c| c.components.push(node(8, 110, None, "top", 4)),
            "overlap",
        ),
        (
            "rail beyond end",
            |c| c.components[1].position = 15,
            "length",
        ),
        (
            "integer overflow rail",
            |c| c.components[1].position = u16::MAX,
            "length",
        ),
        (
            "missing parent",
            |c| {
                c.components.remove(1);
            },
            "missing parent",
        ),
        ("cycle", |c| c.components[1].parent = Some(3), "cycle"),
        ("self cycle", |c| c.components[1].parent = Some(2), "cycle"),
        ("duplicate node", |c| c.components[1].node = 1, "duplicate"),
        ("reserved root", |c| c.components[0].node = 0, "reserved"),
        (
            "unknown ID",
            |c| c.components[0].definition = DefinitionId(99999),
            "unknown",
        ),
        (
            "missing magazine",
            |c| {
                c.components.remove(0);
            },
            "required",
        ),
        (
            "rigid slot offset",
            |c| c.components[0].position = 1,
            "length",
        ),
        (
            "wrong magwell",
            |c| c.components[0].definition = DefinitionId(102),
            "interface",
        ),
    ];
    let catalog = catalog_fixture();
    for (label, change, reason) in cases {
        let mut config = assembled();
        change(&mut config);
        let err = catalog.resolve(&config).unwrap_err();
        assert!(err.message.contains(reason), "{label}: {err}");
        assert_eq!(err.file, "fixture.ron");
    }
}

#[test]
fn assembly_rejects_platform_caliber_thread_size_dependencies_and_exclusions() {
    type Case = (fn(&mut CatalogDocument), &'static str);
    let cases: &[Case] = &[
        (
            |d| d.components[3].assembly.caliber = Some("9x19".into()),
            "caliber",
        ),
        (
            |d| d.components[3].assembly.platform = Some("other".into()),
            "platform",
        ),
        (|d| d.components[3].ammo = DefinitionId(202), "ammo"),
        (
            |d| d.components[8].assembly.interface.standard = "5/8-24".into(),
            "interface",
        ),
        (|d| d.components[8].assembly.interface.size = 2, "interface"),
        (
            |d| d.components[5].assembly.requires.push(DefinitionId(101)),
            "requires",
        ),
        (
            |d| d.components[5].assembly.excludes.push(DefinitionId(112)),
            "excludes",
        ),
    ];
    for (change, reason) in cases {
        let mut doc = document();
        change(&mut doc);
        let catalog = Catalog::validate("fixture.ron", doc).unwrap();
        let err = catalog.resolve(&assembled()).unwrap_err();
        assert!(err.message.contains(reason), "{err}");
    }
}

#[test]
fn assembly_is_deterministic_for_all_entry_permutations() {
    let catalog = catalog_fixture();
    let mut config = assembled();
    let expected = catalog.resolve(&config).unwrap();
    // All 7! permutations, using an independent in-place permutation walk.
    fn permutations(
        catalog: &Catalog,
        config: &mut WeaponConfiguration,
        start: usize,
        expected: &ResolvedAssembly,
        count: &mut usize,
    ) {
        if start == config.components.len() {
            assert_eq!(&catalog.resolve(config).unwrap().assembly, expected);
            *count += 1;
        } else {
            for i in start..config.components.len() {
                config.components.swap(start, i);
                permutations(catalog, config, start + 1, expected, count);
                config.components.swap(start, i);
            }
        }
    }
    let mut count = 0;
    permutations(&catalog, &mut config, 0, &expected.assembly, &mut count);
    assert_eq!(count, 5040);
}

#[test]
fn assembly_rigid_rotations_and_conservative_swept_clearance() {
    let mut doc = document();
    doc.weapons[0].assembly.geometry.sockets[1].pose.rotation =
        Quat::from_rotation_y(std::f32::consts::FRAC_PI_2).to_array();
    // Charging/iron corridor in root coordinates, including transformed rail.
    doc.weapons[0].assembly.geometry.clearance.push(Bounds {
        min: [0.01, 0.1, -0.02],
        max: [0.05, 0.15, 0.02],
    });
    doc.components[5].assembly.geometry.solids.push(Bounds {
        min: [-0.01; 3],
        max: [0.01; 3],
    });
    let catalog = Catalog::validate("fixture.ron", doc).unwrap();
    assert!(catalog
        .resolve(&assembled())
        .unwrap_err()
        .message
        .contains("clearance"));
    let mut config = assembled();
    config.components[1].position = 10;
    let resolved = catalog.resolve(&config).unwrap();
    assert!(
        (resolved.assembly.anchors[&AnchorKind::Sight].translation - Vec3::new(0.1, 0.12, 0.0))
            .length()
            < 1e-6
    );
    assert_eq!(resolved.assembly.obstruction.len(), 1);
}

#[test]
fn assembly_revision_atomicity_cache_and_ammunition_conservation() {
    let catalog = catalog_fixture();
    let mut equipped = EquippedWeapon::new(&catalog, WeaponInstanceId(42), assembled()).unwrap();
    assert!(equipped.fire());
    let original = crate::data::encode(equipped.instance());
    let original_assembly = equipped.spec().assembly.clone();
    let mut config = assembled();
    config.components.remove(1); // optic remains, cannot remove its mount alone
    assert!(equipped
        .reconfigure(&catalog, 0, config)
        .unwrap_err()
        .message
        .contains("parent"));
    assert_eq!(crate::data::encode(equipped.instance()), original);
    assert_eq!(equipped.spec().assembly, original_assembly);
    let mut config = assembled();
    config.components.reverse();
    let cache_ptr = equipped.spec().assembly.parts.as_ptr();
    assert_eq!(equipped.reconfigure(&catalog, 0, config).unwrap(), 0);
    assert_eq!(equipped.spec().assembly.parts.as_ptr(), cache_ptr);
    let mut config = assembled();
    config.components[0].definition = DefinitionId(101);
    assert!(equipped
        .reconfigure(&catalog, 0, config.clone())
        .unwrap_err()
        .message
        .contains("not owned"));
    assert_eq!(crate::data::encode(equipped.instance()), original);
    // Supply a real owned 30-round magazine rather than changing the capacity
    // of the existing 20-round physical object.
    let mut instance = equipped.instance().clone();
    let mut magazines = instance.mechanics.magazines().to_vec();
    magazines[1].definition = DefinitionId(101);
    magazines[1].capacity = 30;
    magazines[1].rounds = 30;
    instance.mechanics = MechanicalState::with_magazines(
        &catalog,
        equipped.spec(),
        magazines,
        instance.mechanics.inserted().map(|m| m.id),
        instance.mechanics.chamber(),
    )
    .unwrap();
    equipped = EquippedWeapon::restore(&catalog, instance).unwrap();
    let total = equipped.mechanics().total_rounds();
    assert_eq!(
        equipped.reconfigure(&catalog, 0, config.clone()).unwrap(),
        1
    );
    assert_eq!((equipped.rounds(), equipped.reserve()), (31, 58));
    assert_eq!(equipped.mechanics().total_rounds(), total);
    assert_eq!(equipped.spec().magazine_size, 30);
    assert!(equipped.reconfigure(&catalog, 0, assembled()).is_err());
    assert_eq!(equipped.reconfigure(&catalog, 1, assembled()).unwrap(), 2);
    assert_eq!((equipped.rounds(), equipped.reserve()), (21, 68));
    assert_eq!(equipped.mechanics().total_rounds(), total);
    assert_eq!(equipped.spec().magazine_size, 20);
    assert_eq!(equipped.instance().revision, 2);
    let restored: WeaponInstance = crate::data::decode(&crate::data::encode(equipped.instance()));
    assert_eq!(restored.configuration, assembled());
    assert_eq!(restored.revision, 2);
}

#[test]
fn assembly_content_validation_rejects_nonfinite_geometry_and_dependency_cycles() {
    type Mutation = fn(&mut CatalogDocument);
    let mutations: &[Mutation] = &[
        |d| d.components[5].assembly.geometry.center_of_mass[0] = f32::NAN,
        |d| d.components[5].assembly.modifiers.velocity = f32::INFINITY,
        |d| d.components[5].assembly.modifiers.recoil = 0.0,
        |d| d.weapons[0].assembly.geometry.sockets[1].pose.rotation = [0.0; 4],
        |d| d.weapons[0].assembly.geometry.sockets[1].pitch_m = -0.1,
        |d| d.weapons[0].assembly.geometry.sockets[1].positions = 257,
        |d| {
            d.components[5].assembly.geometry.solids.push(Bounds {
                min: [1.0; 3],
                max: [0.0; 3],
            })
        },
        |d| d.components[5].assembly.requires.push(DefinitionId(99999)),
        |d| {
            d.components[5].assembly.requires.push(DefinitionId(112));
            d.components[6].assembly.requires.push(DefinitionId(111));
        },
    ];
    for change in mutations {
        let mut doc = document();
        change(&mut doc);
        let err = Catalog::validate("bad.ron", doc).unwrap_err();
        assert_eq!(err.file, "bad.ron");
        assert!(!err.field.is_empty());
    }
}

#[test]
fn assembly_accepts_exact_tree_limits_and_rejects_next_value() {
    let mut doc = document();
    doc.weapons[0].assembly.geometry.sockets[1].positions = 32;
    doc.components[4].assembly.occupied_positions = 1;
    let catalog = Catalog::validate("fixture.ron", doc).unwrap();
    let mut config = catalog.configuration(WeaponId::M4A1).unwrap();
    for index in 0..31 {
        config
            .components
            .push(node(index + 2, 110, None, "top", index));
    }
    assert_eq!(catalog.resolve(&config).unwrap().assembly.parts.len(), 32);
    config.components.push(node(33, 110, None, "top", 31));
    assert!(catalog
        .resolve(&config)
        .unwrap_err()
        .message
        .contains("1..=32"));

    let mut doc = document();
    let link = interface(InterfaceKind::Platform, "link");
    doc.weapons[0].assembly.geometry.sockets.push(socket(
        "link",
        link.clone(),
        vec![ComponentKind::Adapter],
    ));
    let mut adapter = component(120, ComponentKind::Adapter, link.clone());
    adapter
        .assembly
        .geometry
        .sockets
        .push(socket("link", link, vec![ComponentKind::Adapter]));
    doc.components.push(adapter);
    let catalog = Catalog::validate("fixture.ron", doc).unwrap();
    let mut config = catalog.configuration(WeaponId::M4A1).unwrap();
    for index in 0..8 {
        config.components.push(node(
            index + 2,
            120,
            if index == 0 { None } else { Some(index + 1) },
            "link",
            0,
        ));
    }
    assert!(catalog.resolve(&config).is_ok());
    config.components.push(node(10, 120, Some(9), "link", 0));
    assert!(catalog
        .resolve(&config)
        .unwrap_err()
        .message
        .contains("depth 8"));
}

#[test]
fn assembly_rejects_solid_overlap_ambiguous_anchors_and_composed_extremes() {
    let mut doc = document();
    let bounds = Bounds {
        min: [-0.1; 3],
        max: [0.1; 3],
    };
    doc.components[4].assembly.geometry.solids.push(bounds);
    doc.components[5].assembly.geometry.solids.push(bounds);
    let catalog = Catalog::validate("fixture.ron", doc).unwrap();
    assert!(catalog
        .resolve(&assembled())
        .unwrap_err()
        .message
        .contains("solid conflict"));
    let mut doc = document();
    doc.components[6].assembly.geometry.anchors.push(Anchor {
        kind: AnchorKind::Sight,
        pose: RigidPose::default(),
    });
    let catalog = Catalog::validate("fixture.ron", doc).unwrap();
    assert!(catalog
        .resolve(&assembled())
        .unwrap_err()
        .message
        .contains("ambiguous Sight"));
    for (value, succeeds) in [(2.0, true), (4.0, false), (0.1, false)] {
        let mut doc = document();
        doc.components[4].assembly.modifiers.recoil = value;
        doc.components[5].assembly.modifiers.recoil = value;
        let catalog = Catalog::validate("fixture.ron", doc).unwrap();
        assert_eq!(catalog.resolve(&assembled()).is_ok(), succeeds);
    }
    let mut doc = document();
    doc.weapons[0].assembly.mass_kg = 30.0;
    doc.components[0].mass_kg = 20.0;
    assert_eq!(
        Catalog::validate("fixture.ron", doc.clone())
            .unwrap()
            .weapon(WeaponId::M4A1)
            .unwrap()
            .assembly
            .mass_kg,
        50.0
    );
    doc.components[0].mass_kg = 20.01;
    assert!(Catalog::validate("fixture.ron", doc)
        .unwrap_err()
        .message
        .contains("limits"));
}

#[test]
fn assembly_removal_of_explicit_dependency_is_atomic_even_without_parent_link() {
    let mut doc = document();
    doc.components[5].assembly.requires.push(DefinitionId(112));
    let catalog = Catalog::validate("fixture.ron", doc).unwrap();
    let mut equipped = EquippedWeapon::new(&catalog, WeaponInstanceId(50), assembled()).unwrap();
    let before = crate::data::encode(equipped.instance());
    let mut config = assembled();
    config
        .components
        .retain(|n| n.definition != DefinitionId(112));
    assert!(equipped
        .reconfigure(&catalog, 0, config)
        .unwrap_err()
        .message
        .contains("requires"));
    assert_eq!(crate::data::encode(equipped.instance()), before);
}

#[test]
fn assembly_root_bridges_keep_bore_and_device_exit_distinct_through_ads() {
    let mut doc = document();
    doc.weapons[0].assembly.hip_from_root.translation = [0.3, -0.2, -0.2];
    doc.weapons[0].assembly.ads_from_root.translation = [0.0, -0.1, 0.0];
    let catalog = Catalog::validate("fixture.ron", doc).unwrap();
    let resolved = catalog.resolve(&assembled()).unwrap();
    assert!((Vec3::from(resolved.barrel_offset) - Vec3::new(0.3, -0.2, -0.6)).length() < 1e-6);
    for (ads, expected) in [
        (0.0, Vec3::new(0.3, -0.2, -0.6)),
        (0.5, Vec3::new(0.15, -0.15, -0.5)),
        (1.0, Vec3::new(0.0, -0.1, -0.4)),
    ] {
        assert!(
            (crate::weapon_animation::resolved_barrel_offset(&resolved, ads) - expected).length()
                < 1e-6
        );
    }
    assert!(
        (resolved.assembly.anchors[&AnchorKind::DeviceExit]
            .translation
            .z
            + 0.6)
            .abs()
            < 1e-6
    );
}
