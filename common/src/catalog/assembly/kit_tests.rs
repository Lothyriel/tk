//! Acceptance for the real TAR-26 kit, separate from synthetic connector tests.
use super::*;

fn node(
    node: u16,
    definition: u32,
    parent: Option<u16>,
    socket: &str,
    position: u16,
) -> Attachment {
    Attachment {
        node,
        definition: DefinitionId(definition),
        parent,
        socket: socket.into(),
        position,
    }
}

fn replace(config: &mut WeaponConfiguration, id: u16, definition: u32) {
    config
        .components
        .iter_mut()
        .find(|n| n.node == id)
        .unwrap()
        .definition = DefinitionId(definition);
}

fn suppress(config: &mut WeaponConfiguration) {
    replace(config, 5, 128);
    config
        .components
        .push(node(8, 129, Some(5), "suppressor", 0));
}

fn optic(config: &mut WeaponConfiguration, position: u16) {
    config.components.push(node(9, 132, None, "top", position));
    config.components.push(node(10, 133, Some(9), "optic", 0));
}

#[test]
fn m4a1_kit_base_reconstructs_authored_parts_and_preserves_mass_and_contacts() {
    let catalog = Catalog::parse(CATALOG_FILE, CATALOG_SOURCE).unwrap();
    let config = catalog.configuration(WeaponId::M4A1).unwrap();
    let spec = catalog.resolve(&config).unwrap();
    assert_eq!(spec.magazine_size, 30);
    assert!((spec.assembly.mass_kg - 2.99).abs() < 1e-6);
    let original = authored_rig("m4a1_refined").unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for (asset, pose) in std::iter::once(("m4a1_receiver", Transform::IDENTITY)).chain(
        spec.assembly
            .parts
            .iter()
            .filter(|p| p.kind != ComponentKind::Magazine)
            .map(|p| {
                (
                    catalog.components[&p.definition]
                        .assembly
                        .geometry
                        .authored_rig
                        .as_deref()
                        .unwrap(),
                    p.pose,
                )
            }),
    ) {
        for (name, local) in &authored_rig(asset).unwrap().pivots {
            assert!(seen.insert(name), "duplicate rendered part {name}");
            let actual = pose * local.transform();
            let expected = original.pivots[name].transform();
            assert!(
                actual.translation.distance(expected.translation) < 1e-6,
                "{name}"
            );
            assert!(
                actual.rotation.angle_between(expected.rotation) < 1e-5,
                "{name}"
            );
        }
    }
    assert_eq!(seen.len(), original.pivots.len());
    for (kind, expected) in [
        (AnchorKind::Bore, [0.0, 0.0, -0.36830]),
        (AnchorKind::DeviceExit, [0.0, 0.0, -0.39565]),
        (AnchorKind::StockContact, [0.0, -0.035, 0.44255]),
        (AnchorKind::SupportGrip, [0.0, -0.027, -0.110]),
        (AnchorKind::RearIron, [0.0, 0.066, 0.142]),
    ] {
        assert!(
            spec.assembly.anchors[&kind]
                .translation
                .distance(Vec3::from(expected))
                < 1e-6
        );
    }
}

#[test]
fn m4a1_kit_extreme_matrix_accepts_only_physically_compatible_combinations() {
    let catalog = Catalog::parse(CATALOG_FILE, CATALOG_SOURCE).unwrap();
    let mut accepted = 0;
    let mut rejected = 0;
    for barrel in [125, 126] {
        for handguard in [123, 124] {
            for magazine in [101, 120] {
                for stock in [121, 122] {
                    for stock_position in [0, 5] {
                        for device in [127, 128, 129] {
                            for grip in [0, 134, 135] {
                                let mut config = catalog.configuration(WeaponId::M4A1).unwrap();
                                for (n, d) in
                                    [(1, magazine), (2, stock), (3, handguard), (4, barrel)]
                                {
                                    replace(&mut config, n, d);
                                }
                                config
                                    .components
                                    .iter_mut()
                                    .find(|n| n.node == 2)
                                    .unwrap()
                                    .position = stock_position;
                                if device == 129 {
                                    suppress(&mut config);
                                } else {
                                    replace(&mut config, 5, device);
                                }
                                if grip != 0 {
                                    config.components.push(node(
                                        11,
                                        grip,
                                        Some(3),
                                        "bottom",
                                        if grip == 134 { 4 } else { 1 },
                                    ));
                                }
                                let expected_ok = !(handguard == 124
                                    && (grip != 0 || (barrel == 126 && device == 129)));
                                let result = catalog.resolve(&config);
                                assert_eq!(result.is_ok(), expected_ok,
                                    "barrel={barrel} handguard={handguard} magazine={magazine} stock={stock}/{stock_position} device={device} grip={grip}: {result:?}");
                                let Ok(spec) = result else {
                                    rejected += 1;
                                    continue;
                                };
                                accepted += 1;
                                assert_eq!(
                                    spec.magazine_size,
                                    if magazine == 101 { 30 } else { 20 }
                                );
                                let bore = if barrel == 125 { -0.36830 } else { -0.26162 };
                                let extension = match device {
                                    127 => 0.045,
                                    128 => 0.06604,
                                    _ => 0.16256,
                                };
                                assert!(
                                    (spec.assembly.anchors[&AnchorKind::Bore].translation.z - bore)
                                        .abs()
                                        < 1e-6
                                );
                                assert!(
                                    (spec.assembly.anchors[&AnchorKind::DeviceExit].translation.z
                                        - (bore + 0.01765 - extension))
                                        .abs()
                                        < 1e-6
                                );
                                let rear = if stock == 121 { 0.178 } else { 0.1778 };
                                assert!(
                                    (spec.assembly.anchors[&AnchorKind::StockContact]
                                        .translation
                                        .z
                                        - (0.182 + f32::from(stock_position) * 0.01651 + rear))
                                        .abs()
                                        < 1e-6
                                );
                                let expected_mass = 1.322689
                                    + 0.14
                                    + 0.07
                                    + if magazine == 101 { 0.11 } else { 0.113398 }
                                    + if stock == 121 { 0.25 } else { 0.252311 }
                                    + if handguard == 123 { 0.312311 } else { 0.189942 }
                                    + if barrel == 125 { 0.72 } else { 0.522245 }
                                    + match device {
                                        127 => 0.065,
                                        128 => 0.1105,
                                        _ => 0.5925,
                                    }
                                    + match grip {
                                        134 => 0.073709,
                                        135 => 0.071724,
                                        _ => 0.0,
                                    };
                                assert!((spec.assembly.mass_kg - expected_mass).abs() < 1e-6);
                                assert!(spec.assembly.center_of_mass.iter().all(|v| v.is_finite()));
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!((accepted, rejected), (184, 104));
}

#[test]
fn m4a1_kit_optic_mount_and_descendant_contacts_follow_installed_parts() {
    let catalog = Catalog::parse(CATALOG_FILE, CATALOG_SOURCE).unwrap();
    let mut config = catalog.configuration(WeaponId::M4A1).unwrap();
    optic(&mut config, 3);
    suppress(&mut config);
    config.components.push(node(11, 134, Some(3), "bottom", 4));
    let spec = catalog.resolve(&config).unwrap();
    assert!((spec.assembly.mass_kg - (2.99 - 0.065 + 0.5925 + 0.36 + 0.073709)).abs() < 1e-6);
    let eye = spec.assembly.anchors[&AnchorKind::Sight].translation;
    assert!(eye.distance(Vec3::new(0.0, 0.072, 0.272)) < 1e-6);
    assert!(spec.assembly.ads_from_root.transform_point(eye).length() < 1e-6);
    let support = spec.assembly.anchors[&AnchorKind::SupportGrip];
    assert!(support.translation.distance(Vec3::new(0.0, -0.077, -0.091)) < 1e-6);
    assert!(
        support
            .rotation
            .angle_between(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
            < 1e-5
    );
    let fixed = spec.assembly.pivots["charging_handle"];
    config.components.retain(|n| ![8, 11].contains(&n.node));
    let removed = catalog.resolve(&config).unwrap();
    assert_eq!(removed.assembly.pivots["charging_handle"], fixed);
    assert!(
        (removed.assembly.anchors[&AnchorKind::DeviceExit]
            .translation
            .z
            + 0.41669)
            .abs()
            < 1e-6
    );
    assert!(
        (removed.assembly.anchors[&AnchorKind::SupportGrip]
            .translation
            .y
            + 0.027)
            .abs()
            < 1e-6
    );
    config.components.reverse();
    assert_eq!(catalog.resolve(&config).unwrap().assembly, removed.assembly);
}

#[test]
fn m4a1_kit_rejects_real_rail_interface_and_clearance_conflicts() {
    let catalog = Catalog::parse(CATALOG_FILE, CATALOG_SOURCE).unwrap();
    let base = catalog.configuration(WeaponId::M4A1).unwrap();
    let mut cases = Vec::new();
    let mut config = base.clone();
    config.components.extend([
        node(11, 134, Some(3), "bottom", 4),
        node(12, 135, Some(3), "bottom", 1),
    ]);
    cases.push((config, "occupied socket/rail overlap"));
    let mut config = base.clone();
    config.components.push(node(11, 135, Some(3), "bottom", 3));
    cases.push((config, "exceeds socket/rail length"));
    let mut config = base.clone();
    replace(&mut config, 3, 124);
    config.components.push(node(11, 134, Some(3), "bottom", 0));
    cases.push((config, "incompatible interface"));
    let mut config = base.clone();
    config.components.push(node(10, 133, None, "top", 0));
    cases.push((config, "incompatible interface"));
    let mut config = base.clone();
    config
        .components
        .push(node(8, 129, Some(5), "suppressor", 0));
    cases.push((config, "missing socket"));
    let mut config = base.clone();
    replace(&mut config, 5, 129);
    cases.push((config, "incompatible interface"));
    let mut config = base.clone();
    optic(&mut config, 5); // Distinct rail intervals, but housing hits the fixed rear iron.
    cases.push((config, "solid conflict"));
    let mut config = base.clone();
    config.components.retain(|n| n.node != 7);
    optic(&mut config, 10); // Foot fits the rail; optic body obstructs handle/fingers.
    cases.push((config, "clearance conflict"));
    let mut config = base.clone();
    replace(&mut config, 4, 126);
    replace(&mut config, 3, 124);
    suppress(&mut config);
    cases.push((config, "solid conflict"));
    for (config, reason) in cases {
        let error = catalog.resolve(&config).unwrap_err();
        assert!(error.message.contains(reason), "{reason}: {error}");
    }
}

#[test]
fn m4a1_kit_grip_rail_endpoints_clear_both_magazine_sweeps() {
    let catalog = Catalog::parse(CATALOG_FILE, CATALOG_SOURCE).unwrap();
    for magazine in [101, 120] {
        for (grip, last) in [(134, 9), (135, 2)] {
            let mut first_contact = None;
            for position in 0..=last {
                let mut config = catalog.configuration(WeaponId::M4A1).unwrap();
                replace(&mut config, 1, magazine);
                config
                    .components
                    .push(node(11, grip, Some(3), "bottom", position));
                let spec = catalog.resolve(&config).unwrap();
                let contact = spec.assembly.anchors[&AnchorKind::SupportGrip].translation;
                let first = *first_contact.get_or_insert(contact);
                assert!(contact.distance(first + Vec3::Z * f32::from(position) * 0.01) < 1e-6);
            }
        }
    }
}

#[test]
fn m4a1_kit_physical_magazines_use_real_capacity_without_rewriting_rounds() {
    let catalog = Catalog::parse(CATALOG_FILE, CATALOG_SOURCE).unwrap();
    for (definition, capacity) in [(101, 30), (120, 20)] {
        let mut config = catalog.configuration(WeaponId::M4A1).unwrap();
        replace(&mut config, 1, definition);
        let mut weapon =
            EquippedWeapon::new(&catalog, WeaponInstanceId(42), config.clone()).unwrap();
        assert_eq!(weapon.rounds(), capacity);
        assert!(weapon
            .mechanics()
            .magazines()
            .iter()
            .all(|m| m.definition == DefinitionId(definition)
                && m.capacity == capacity
                && m.rounds <= capacity));
        let before = crate::data::encode(weapon.mechanics());
        // A named definition is not an owned physical magazine. Reconfiguration
        // may not manufacture the other capacity or silently discard ammunition.
        replace(&mut config, 1, if definition == 101 { 120 } else { 101 });
        assert!(weapon.reconfigure(&catalog, 0, config).is_err());
        assert_eq!(crate::data::encode(weapon.mechanics()), before);
        assert_eq!(weapon.instance().revision, 0);
    }
}
