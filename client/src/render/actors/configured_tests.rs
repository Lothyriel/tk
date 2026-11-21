use super::super::{feedback, voxel};
use super::*;
use common::{
    catalog::catalog,
    replication::{Configuration, catalog_fingerprint},
};

fn fixture(weapon: WeaponKind) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        bevy::camera::visibility::VisibilityPlugin,
    ))
    .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs_f64(1.0 / 60.0),
    ))
    .init_resource::<Assets<Mesh>>()
    .init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
    .init_resource::<Assets<StandardMaterial>>()
    .init_resource::<feedback::Feedback>()
    .insert_resource(PlayerId(99))
    .add_plugins((Plugin, voxel::Plugin))
    .add_systems(Update, feedback::advance);
    let definition = weapon.definition();
    let mut session = ClientSession::default();
    session.configurations.cache.begin_epoch(1).unwrap();
    let fingerprint = catalog_fingerprint(common::catalog::CATALOG_SOURCE);
    session
        .configurations
        .cache
        .apply(
            catalog(),
            fingerprint,
            Configuration {
                epoch: 1,
                handle: 1,
                catalog: fingerprint,
                owner: 7,
                slot: if weapon == WeaponKind::Pistol {
                    EquipmentSlot::Sidearm
                } else {
                    EquipmentSlot::Primary
                },
                instance: WeaponInstanceId(42),
                revision: 1,
                weapon: catalog().configuration(definition).unwrap(),
                magazines: (1..=4)
                    .map(|id| common::replication::MagazineDefinition {
                        id: common::catalog::MagazineInstanceId(id),
                        definition: catalog().configuration(definition).unwrap().components[0]
                            .definition,
                    })
                    .collect(),
            },
        )
        .unwrap();
    let mut player = ClientData {
        id: 7,
        alive: true,
        weapon,
        weapon_id: definition,
        configuration: 1,
        weapon_instance: WeaponInstanceId(42),
        ..default()
    };
    player.pose.weapon = weapon;
    player.mechanics.magazine_inserted = true;
    player.mechanics.equipped = true;
    session.players.push(player);
    app.insert_resource(session);
    let body = app.world_mut().spawn(body()).id();
    app.world_mut()
        .spawn((
            Client { id: 7 },
            Transform::IDENTITY,
            Visibility::Inherited,
            RemoteTarget {
                position: Vec3::new(2.0, 0.0, -3.0),
                rotation: Quat::from_rotation_y(0.8),
                snap: true,
            },
        ))
        .add_child(body);
    app.update();
    (app, body)
}

fn descendants(world: &World, root: Entity) -> Vec<Entity> {
    let mut result = vec![];
    if let Some(children) = world.get::<Children>(root) {
        for child in children.iter() {
            result.push(child);
            result.extend(descendants(world, child));
        }
    }
    result
}

fn weapon_root(app: &App, body: Entity) -> Entity {
    let matches: Vec<_> = descendants(app.world(), body)
        .into_iter()
        .filter(|&e| app.world().get::<WorldWeapon>(e).is_some())
        .collect();
    assert_eq!(matches.len(), 1);
    matches[0]
}

#[test]
fn configured_remote_stock_contacts_actual_chest_or_plate_with_rigid_arms() {
    for weapon in [WeaponKind::Rifle, WeaponKind::Smg] {
        for armor in [ArmorKind::None, ArmorKind::Light, ArmorKind::Heavy] {
            let (mut app, body) = fixture(weapon);
            let root = weapon_root(&app, body);
            {
                let mut session = app.world_mut().resource_mut::<ClientSession>();
                let player = &mut session.players[0];
                player.armor = ArmorState::from_loadout(Loadout { armor, ..default() });
                player.pose.ads = 1.0;
                player.pose.crouch = 0.7;
                player.pose.aim_pitch = -0.6;
                player.pose.aim_roll = 0.25;
            }
            for _ in 0..180 {
                app.update();
                assert_arms(&app, body, root);
            }
            let children = descendants(app.world(), body);
            let surface = children
                .iter()
                .copied()
                .find(|&e| {
                    if armor == ArmorKind::None {
                        app.world()
                            .get::<BoneVisual>(e)
                            .is_some_and(|b| b.segment == Segment::Thorax && b.inner == 0)
                    } else {
                        app.world()
                            .get::<EquipmentVisual>(e)
                            .is_some_and(|g| !g.helmet)
                    }
                })
                .unwrap();
            let world = app.world();
            let stock = weapon.spec().assembly.anchors[&common::catalog::AnchorKind::StockContact];
            let accepted = world.get::<ConfiguredPose>(root).unwrap();
            let contact = world
                .get::<GlobalTransform>(root)
                .unwrap()
                .mul_transform(accepted.solution.root)
                .transform_point(stock.translation);
            let surface_pose = world.get::<GlobalTransform>(surface).unwrap();
            let local = surface_pose.to_matrix().inverse().transform_point3(contact);
            let mesh = world
                .resource::<Assets<Mesh>>()
                .get(&world.get::<Mesh3d>(surface).unwrap().0)
                .unwrap();
            let bevy::mesh::VertexAttributeValues::Float32x3(vertices) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap()
            else {
                panic!("vertices");
            };
            let front = vertices.iter().map(|v| v[2]).fold(f32::INFINITY, f32::min);
            assert!(
                (local.z - front).abs() < 0.001,
                "{weapon:?}/{armor:?}: {local:?}, front={front}"
            );
            let scale = surface_pose.scale();
            assert!((local.x * scale.x - 0.14).abs() < 0.001);
            assert!((local.y * scale.y - 0.10).abs() < 0.001);
            // A logical stock beyond the anatomical budget must not silently
            // fall back to an unbraced pose or replace the accepted hierarchy.
            let before = *world.get::<Transform>(surface).unwrap();
            let accepted_root = accepted.solution.root;
            let entities = descendants(world, root);
            let mut document: common::catalog::CatalogDocument =
                ron::from_str(common::catalog::CATALOG_SOURCE).unwrap();
            let geometry = if weapon == WeaponKind::Rifle {
                &mut document
                    .components
                    .iter_mut()
                    .find(|c| c.id.0 == 121)
                    .unwrap()
                    .assembly
                    .geometry
            } else {
                &mut document
                    .weapons
                    .iter_mut()
                    .find(|w| w.class == weapon)
                    .unwrap()
                    .assembly
                    .geometry
            };
            geometry
                .anchors
                .iter_mut()
                .find(|a| a.kind == common::catalog::AnchorKind::StockContact)
                .unwrap()
                .pose
                .translation = [0.0, 0.0, 4.0];
            let custom = common::catalog::Catalog::validate("unreachable-stock", document).unwrap();
            {
                let mut session = app.world_mut().resource_mut::<ClientSession>();
                let mut packet = session
                    .configurations
                    .cache
                    .get(1, 1)
                    .unwrap()
                    .packet
                    .clone();
                packet.handle = 2;
                packet.revision = 2;
                session
                    .configurations
                    .cache
                    .apply(&custom, packet.catalog, packet)
                    .unwrap();
                session.players[0].configuration = 2;
            }
            app.update();
            assert!(
                app.world()
                    .get::<configured::ConfigurationVisualError>(root)
                    .is_some()
            );
            assert_eq!(
                app.world()
                    .get::<ConfiguredPose>(root)
                    .unwrap()
                    .solution
                    .root,
                accepted_root
            );
            assert_eq!(*app.world().get::<Transform>(surface).unwrap(), before);
            assert_eq!(descendants(app.world(), root), entities);
        }
    }
}

fn assert_arms(app: &App, body: Entity, weapon: Entity) {
    assert!(
        app.world()
            .get::<configured::ConfigurationVisualError>(weapon)
            .is_none(),
        "{:?}, mechanics={:?}",
        app.world()
            .get::<configured::ConfigurationVisualError>(weapon)
            .map(|e| &e.0),
        app.world().resource::<ClientSession>().players[0].mechanics,
    );
    let accepted = app.world().get::<ConfiguredPose>(weapon).unwrap();
    let hidden_meshes = descendants(app.world(), weapon)
        .into_iter()
        .filter(|&e| {
            app.world().get::<Mesh3d>(e).is_some()
                && app.world().get::<Visibility>(e) == Some(&Visibility::Hidden)
        })
        .count();
    assert_eq!(
        hidden_meshes, 6,
        "body meshes replace all configured arm/palm meshes"
    );
    for (arm, shoulder, segments) in [
        (
            accepted.solution.support,
            accepted.shoulders[0],
            [
                Segment::LeftUpperArm,
                Segment::LeftForearm,
                Segment::LeftHand,
            ],
        ),
        (
            accepted.solution.trigger,
            accepted.shoulders[1],
            [
                Segment::RightUpperArm,
                Segment::RightForearm,
                Segment::RightHand,
            ],
        ),
    ] {
        let find = |segment| {
            descendants(app.world(), body)
                .into_iter()
                .find(|&e| {
                    app.world()
                        .get::<BoneVisual>(e)
                        .is_some_and(|b| b.segment == segment && b.inner == 0)
                })
                .unwrap()
        };
        let [upper, lower, hand] =
            segments.map(|segment| *app.world().get::<Transform>(find(segment)).unwrap());
        assert!(upper.transform_point(Vec3::Y * 0.15).distance(shoulder) < 2e-6);
        assert!(
            upper
                .transform_point(Vec3::NEG_Y * 0.15)
                .distance(lower.transform_point(Vec3::Y * 0.14))
                < 2e-6
        );
        assert!(
            lower
                .transform_point(Vec3::NEG_Y * 0.14)
                .distance(hand.translation)
                < 2e-6
        );
        assert!(hand.translation.distance(arm.wrist.translation) < 2e-6);
        assert!(
            hand.rotation
                .normalize()
                .angle_between(arm.wrist.rotation.normalize())
                < 0.001
        );
        assert_eq!(upper.scale, Vec3::ONE);
        assert_eq!(lower.scale, Vec3::ONE);
    }
    let action = app.world().resource::<feedback::Feedback>().responses[&7]
        .action
        .unwrap();
    let player = &app.world().resource::<ClientSession>().players[0];
    let rig = common::weapon_animation::WeaponRig::for_weapon(player.weapon);
    let world_root = app
        .world()
        .get::<GlobalTransform>(weapon)
        .unwrap()
        .mul_transform(accepted.solution.root);
    let socket = |name: &str| {
        let socket_root = if name == "grip.support" {
            descendants(app.world(), weapon)
                .into_iter()
                .find(|&e| {
                    app.world()
                        .get::<configured::ConfiguredPiece>(e)
                        .is_some_and(|p| matches!(p.definition, 134 | 135))
                })
                .unwrap_or(weapon)
        } else {
            weapon
        };
        let entity = descendants(app.world(), socket_root)
            .into_iter()
            .find(|&e| {
                app.world()
                    .get::<Name>(e)
                    .is_some_and(|n| n.as_str() == name)
            })
            .unwrap();
        *app.world().get::<GlobalTransform>(entity).unwrap()
    };
    let magazine = socket("magazine.grasp");
    let control = socket("control.grasp");
    let request = app.world().get::<ConfiguredWeapon>(weapon).unwrap();
    let session = app.world().resource::<ClientSession>();
    let spec = session
        .configurations
        .cache
        .get(request.epoch, request.handle)
        .unwrap()
        .visual(request.magazine)
        .unwrap();
    let recoil = app.world().resource::<feedback::Feedback>().responses[&7].recoil;
    let logical = handling::configured_contacts(spec, action, recoil, &player.mechanics).unwrap();
    let logical_support = world_root.mul_transform(logical.support);
    let clearance = world_root.rotation()
        * if player.weapon == WeaponKind::Rifle {
            common::weapon_animation::m4a1::support_clearance(&action)
        } else {
            Vec3::ZERO
        };
    let actual_support = if player.weapon == WeaponKind::Rifle {
        socket("grip.support")
    } else {
        world_root.mul_transform(Transform::from_translation(rig.support))
    };
    let expected_rotation = actual_support
        .rotation()
        .slerp(magazine.rotation(), action.magazine_grip)
        .slerp(control.rotation(), action.control_grip);
    assert!(
        logical_support.translation().distance(
            actual_support
                .translation()
                .lerp(magazine.translation(), action.magazine_grip)
                .lerp(control.translation(), action.control_grip)
                + clearance
        ) < 2e-6
    );
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        assert!(
            (logical_support.rotation() * axis - expected_rotation * axis).length() < 2e-6,
            "logical authority contact must match baked mechanism socket"
        );
    }
    let expected = actual_support
        .translation()
        .lerp(magazine.translation(), action.magazine_grip)
        .lerp(control.translation(), action.control_grip)
        + clearance;
    let hand = descendants(app.world(), body)
        .into_iter()
        .find(|&e| {
            app.world()
                .get::<BoneVisual>(e)
                .is_some_and(|b| b.segment == Segment::LeftHand && b.inner == 0)
        })
        .unwrap();
    assert!(
        app.world()
            .get::<GlobalTransform>(hand)
            .unwrap()
            .translation()
            .distance(expected)
            < 2e-6
    );
    let handoff = app.world().resource::<PoseCache>().last_live[&(7, Segment::LeftHand)];
    assert!(
        handoff.translation.distance(
            app.world()
                .get::<GlobalTransform>(hand)
                .unwrap()
                .translation()
        ) < 2e-6
    );
    let part = descendants(app.world(), weapon)
        .into_iter()
        .find(|&e| {
            app.world()
                .get::<Name>(e)
                .is_some_and(|n| n.as_str() == "magazine")
        })
        .unwrap();
    assert_eq!(
        app.world().get::<Visibility>(part),
        Some(&if action.magazine_visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        })
    );
}

fn assert_hand_clearance(app: &App, body: Entity, root: Entity) {
    let lower = descendants(app.world(), root)
        .into_iter()
        .find(|&e| {
            app.world()
                .get::<Name>(e)
                .is_some_and(|n| n.as_str() == "lower")
        })
        .unwrap();
    let hand = descendants(app.world(), body)
        .into_iter()
        .find(|&e| {
            app.world()
                .get::<BoneVisual>(e)
                .is_some_and(|b| b.segment == Segment::LeftHand && b.inner == 0)
        })
        .unwrap();
    crate::render::weapons::grasp::assert_magwell_clear(
        app.world(),
        hand,
        lower,
        app.world().resource::<ClientSession>().players[0]
            .mechanics
            .fraction,
    );
}

#[test]
fn configured_remote_mixed_reload_extremes_preserve_contacts_identity_and_cancelled_locations() {
    use common::catalog::{Attachment, DefinitionId, EquippedWeapon, WeaponKit};
    use common::replication::ConfigurationPublisher;
    for (fps, stock, grip, position) in [(30, 0, 134, 0), (60, 5, 134, 9), (165, 5, 135, 2)] {
        for empty in [false, true] {
            let (mut app, body) = fixture(WeaponKind::Rifle);
            let root = weapon_root(&app, body);
            app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                std::time::Duration::from_secs_f64(1.0 / fps as f64),
            ));
            let mut kit = WeaponKit::from_configuration(
                catalog(),
                catalog().configuration(common::WeaponId::M4A1).unwrap(),
            )
            .unwrap();
            // Exercise both directions, since outgoing and incoming have
            // different grasp heights even though the seating interface agrees.
            let (outgoing_definition, incoming_definition) =
                if empty { (101, 120) } else { (120, 101) };
            for part in &mut kit.weapon.components {
                match part.node {
                    1 => part.definition = DefinitionId(outgoing_definition),
                    2 => {
                        part.definition = DefinitionId(122);
                        part.position = stock;
                    }
                    _ => {}
                }
            }
            kit.weapon.components.push(Attachment {
                node: 11,
                definition: DefinitionId(grip),
                parent: Some(3),
                socket: "bottom".into(),
                position,
            });
            kit.reserves = vec![DefinitionId(incoming_definition)];
            let mut weapon =
                EquippedWeapon::from_kit(catalog(), WeaponInstanceId(77), &kit).unwrap();
            let shots = if empty { weapon.rounds() } else { 4 };
            for _ in 0..shots {
                assert!(weapon.fire());
            }
            let total = weapon.mechanics().total_rounds();
            let old = weapon.mechanics().magazines()[0].id;
            let incoming = weapon.mechanics().magazines()[1].id;
            let fingerprint = catalog_fingerprint(common::catalog::CATALOG_SOURCE);
            let mut publisher = ConfigurationPublisher::new(2, fingerprint).unwrap();
            app.world_mut()
                .resource_mut::<ClientSession>()
                .configurations
                .cache
                .begin_epoch(2)
                .unwrap();
            let publish =
                |app: &mut App, publisher: &mut ConfigurationPublisher, weapon: &EquippedWeapon| {
                    let handle = publisher
                        .register(7, EquipmentSlot::Primary, weapon.instance())
                        .unwrap();
                    let packet = publisher.entries()[(handle - 1) as usize].clone();
                    let mut session = app.world_mut().resource_mut::<ClientSession>();
                    session
                        .configurations
                        .cache
                        .apply(catalog(), fingerprint, packet)
                        .unwrap();
                    let player = &mut session.players[0];
                    player.weapon_instance = weapon.instance().id;
                    player.configuration = handle;
                    player.mechanics = weapon.mechanics().snapshot();
                    player.ammo_in_mag = weapon.rounds();
                    player.reserve_ammo = weapon.reserve();
                    player.pose.reload_fraction = player.mechanics.fraction;
                    player.pose.reload_empty = empty;
                };
            assert!(weapon.mechanics_mut().start_reload(2.4));
            let initial = weapon.instance().clone();
            let mut saw_unseated = false;
            for frame in 0..fps * 4 {
                weapon.advance(0.7 / fps as f32);
                if frame % 3 != 2 {
                    publish(&mut app, &mut publisher, &weapon);
                }
                app.update();
                assert_arms(&app, body, root);
                assert_hand_clearance(&app, body, root);
                assert_eq!(weapon.mechanics().total_rounds(), total);
                let player = &app.world().resource::<ClientSession>().players[0];
                let wanted = if player.mechanics.reloading() && player.mechanics.fraction < 0.41 {
                    old
                } else {
                    incoming
                };
                assert_eq!(
                    app.world().get::<ConfiguredWeapon>(root).unwrap().magazine,
                    Some(wanted)
                );
                let pieces: Vec<_> = descendants(app.world(), root)
                    .into_iter()
                    .filter_map(|e| {
                        app.world()
                            .get::<configured::ConfiguredPiece>(e)
                            .filter(|p| [101, 120].contains(&p.definition))
                    })
                    .collect();
                assert_eq!(pieces.len(), 1);
                assert_eq!(
                    pieces[0].definition,
                    if wanted == old {
                        outgoing_definition
                    } else {
                        incoming_definition
                    }
                );
                saw_unseated |= player.mechanics.reloading()
                    && (0.45..0.69).contains(&player.mechanics.fraction);
            }
            assert!(saw_unseated);
            // Restore independent authoritative actions at each phase. Keep
            // the displayed response and check cancellation on its first frame.
            for fraction in [0.15, 0.30, 0.55, 0.75, 0.95] {
                let (mut app, body) = fixture(WeaponKind::Rifle);
                let root = weapon_root(&app, body);
                app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                    std::time::Duration::from_secs_f64(1.0 / fps as f64),
                ));
                app.world_mut()
                    .resource_mut::<ClientSession>()
                    .configurations
                    .cache
                    .begin_epoch(2)
                    .unwrap();
                let mut publisher = ConfigurationPublisher::new(2, fingerprint).unwrap();
                weapon = EquippedWeapon::restore(catalog(), initial.clone()).unwrap();
                weapon.advance(2.4 * fraction);
                publish(&mut app, &mut publisher, &weapon);
                for _ in 0..8 {
                    app.update();
                }
                assert_arms(&app, body, root);
                let before = app.world().resource::<feedback::Feedback>().responses[&7]
                    .action
                    .unwrap();
                let inserted = weapon.mechanics().inserted().map(|m| m.id);
                weapon.mechanics_mut().cancel();
                publish(&mut app, &mut publisher, &weapon);
                app.update();
                assert_hand_clearance(&app, body, root);
                assert!(
                    app.world()
                        .get::<configured::ConfigurationVisualError>(root)
                        .is_none()
                );
                assert_eq!(weapon.mechanics().total_rounds(), total);
                assert_eq!(
                    app.world().get::<ConfiguredWeapon>(root).unwrap().magazine,
                    inserted
                );
                let after = app.world().resource::<feedback::Feedback>().responses[&7]
                    .action
                    .unwrap();
                assert!(
                    after.root.translation.distance(before.root.translation)
                        <= 1.2 / fps as f32 + 1e-6
                );
                let rest =
                    common::weapon_animation::WeaponRig::for_weapon(WeaponKind::Rifle).support;
                let remaining = after.support.translation.distance(rest);
                let previous = before.support.translation.distance(rest);
                if previous > 0.001 {
                    assert!(
                        remaining > 1e-6 && remaining < previous,
                        "return from the displayed hand, not an idle reset"
                    );
                }
                let part = descendants(app.world(), root)
                    .into_iter()
                    .find(|&e| {
                        app.world()
                            .get::<Name>(e)
                            .is_some_and(|n| n.as_str() == "magazine")
                    })
                    .unwrap();
                assert_eq!(
                    app.world().get::<Visibility>(part),
                    Some(&if inserted.is_some() {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    })
                );
                // The entire committed hierarchy follows the owner's physical
                // visibility. No retained/incoming child may float after a fall.
                for mode in [
                    PhysicalMode::Fallen,
                    PhysicalMode::Recovering,
                    PhysicalMode::Standing,
                ] {
                    app.world_mut().resource_mut::<ClientSession>().players[0]
                        .pose
                        .mode = mode;
                    app.update();
                    assert_eq!(
                        app.world().get::<Visibility>(root),
                        Some(&if mode == PhysicalMode::Standing {
                            Visibility::Inherited
                        } else {
                            Visibility::Hidden
                        })
                    );
                    if mode != PhysicalMode::Standing {
                        for entity in descendants(app.world(), root) {
                            assert!(
                                !app.world()
                                    .get::<InheritedVisibility>(entity)
                                    .is_some_and(|v| v.get())
                            );
                        }
                    }
                    assert_eq!(weapon.mechanics().total_rounds(), total);
                }
                app.world_mut().resource_mut::<ClientSession>().players[0].alive = false;
                app.update();
                assert_eq!(
                    app.world().get::<Visibility>(root),
                    Some(&Visibility::Hidden)
                );
                for entity in descendants(app.world(), root) {
                    assert!(
                        !app.world()
                            .get::<InheritedVisibility>(entity)
                            .is_some_and(|v| v.get())
                    );
                }
            }
        }
    }
}

#[test]
fn configured_remote_inspect_weapon_and_palms_clear_rendered_torso() {
    use bevy::mesh::VertexAttributeValues;
    use common::catalog::{Attachment, DefinitionId};
    for (stock, grip, position) in [(0, 134, 0), (5, 134, 9), (5, 135, 2)] {
        let (mut app, body) = fixture(WeaponKind::Rifle);
        let root = weapon_root(&app, body);
        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            let mut packet = session
                .configurations
                .cache
                .get(1, 1)
                .unwrap()
                .packet
                .clone();
            packet.handle = 2;
            packet.revision = 2;
            let part = packet
                .weapon
                .components
                .iter_mut()
                .find(|p| p.node == 2)
                .unwrap();
            part.definition = DefinitionId(122);
            part.position = stock;
            packet.weapon.components.push(Attachment {
                node: 11,
                definition: DefinitionId(grip),
                parent: Some(3),
                socket: "bottom".into(),
                position,
            });
            session
                .configurations
                .cache
                .apply(catalog(), packet.catalog, packet)
                .unwrap();
            session.players[0].configuration = 2;
        }
        for step in 0..=28 {
            {
                let mut session = app.world_mut().resource_mut::<ClientSession>();
                session.players[0].mechanics.kind =
                    Some(common::catalog::ManipulationKind::Inspect);
                session.players[0].mechanics.fraction = step as f32 / 28.0;
            }
            for _ in 0..6 {
                app.update();
            }
            assert_arms(&app, body, root);
            let world = app.world();
            for torso in descendants(world, body).into_iter().filter(|&e| {
                world.get::<BoneVisual>(e).is_some_and(|b| {
                    matches!(
                        b.segment,
                        Segment::Thorax | Segment::Abdomen | Segment::Pelvis
                    ) && b.inner == 0
                })
            }) {
                let meshes = world.resource::<Assets<Mesh>>();
                let torso_mesh = meshes.get(&world.get::<Mesh3d>(torso).unwrap().0).unwrap();
                let VertexAttributeValues::Float32x3(vertices) =
                    torso_mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap()
                else {
                    panic!("positions");
                };
                // voxel_mesh is a union of indexed cuboids, 24 vertices per voxel.
                // Compare with its actual occupied cells, not an enclosing solid torso.
                let boxes: Vec<_> = vertices
                    .as_chunks::<24>()
                    .0
                    .iter()
                    .map(|cell| {
                        let min = cell
                            .iter()
                            .map(|&p| Vec3::from(p))
                            .fold(Vec3::splat(f32::INFINITY), Vec3::min);
                        let max = cell
                            .iter()
                            .map(|&p| Vec3::from(p))
                            .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max);
                        ((min + max) * 0.5, (max - min) * 0.5 - Vec3::splat(0.001))
                    })
                    .collect();
                assert!(!boxes.is_empty());
                let to_torso = world
                    .get::<GlobalTransform>(torso)
                    .unwrap()
                    .to_matrix()
                    .inverse();
                let hands = descendants(world, body).into_iter().filter(|&e| {
                    world.get::<BoneVisual>(e).is_some_and(|b| {
                        matches!(b.segment, Segment::LeftHand | Segment::RightHand) && b.inner == 0
                    })
                });
                for entity in descendants(world, root).into_iter().chain(hands) {
                    let Some(mesh) = world.get::<Mesh3d>(entity) else {
                        continue;
                    };
                    if !world
                        .get::<InheritedVisibility>(entity)
                        .is_some_and(|v| v.get())
                    {
                        continue;
                    }
                    let mesh = meshes.get(&mesh.0).unwrap();
                    let VertexAttributeValues::Float32x3(vertices) =
                        mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap()
                    else {
                        panic!("positions");
                    };
                    let transform =
                        to_torso * world.get::<GlobalTransform>(entity).unwrap().to_matrix();
                    let points: Vec<_> = vertices
                        .iter()
                        .map(|&p| transform.transform_point3(Vec3::from(p)))
                        .collect();
                    let indices: Vec<_> = mesh.indices().unwrap().iter().collect();
                    for (centre, half) in &boxes {
                        for triangle in indices.as_chunks::<3>().0 {
                            let p = std::array::from_fn(|i| points[triangle[i]] - centre);
                            assert!(
                                !crate::render::weapons::grasp::triangle_intersects_box(p, *half),
                                "inspect torso intersection kit={stock}/{grip}/{position} step={step} centre={centre:?} half={half:?} entity={:?} parent={:?} points={p:?}",
                                world.get::<Name>(entity),
                                world
                                    .get::<ChildOf>(entity)
                                    .and_then(|c| world.get::<Name>(c.0))
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn configured_remote_m4a1_kit_extremes_keep_rendered_contacts_and_piece_identity() {
    use common::catalog::{Attachment, DefinitionId};
    for (short, suppressed, grip, position) in [
        (true, false, 0, 0),
        (false, true, 134, 0),
        (false, true, 134, 9),
        (false, false, 135, 0),
        (false, false, 135, 2),
    ] {
        let (mut app, body) = fixture(WeaponKind::Rifle);
        let mut packet = app
            .world()
            .resource::<ClientSession>()
            .configurations
            .cache
            .get(1, 1)
            .unwrap()
            .packet
            .clone();
        packet.handle = 2;
        packet.revision = 2;
        for part in &mut packet.weapon.components {
            match part.node {
                1 if !suppressed => part.definition = DefinitionId(120),
                2 => {
                    part.definition = DefinitionId(122);
                    part.position = if suppressed { 5 } else { 0 };
                }
                3 if short => part.definition = DefinitionId(124),
                4 if short => part.definition = DefinitionId(126),
                5 if suppressed => part.definition = DefinitionId(128),
                _ => {}
            }
        }
        if !suppressed {
            for magazine in &mut packet.magazines {
                magazine.definition = DefinitionId(120);
            }
        }
        let attachment = |node, definition, parent, socket: &str, position| Attachment {
            node,
            definition: DefinitionId(definition),
            parent,
            socket: socket.into(),
            position,
        };
        if suppressed {
            packet.weapon.components.extend([
                attachment(8, 129, Some(5), "suppressor", 0),
                attachment(9, 132, None, "top", 3),
                attachment(10, 133, Some(9), "optic", 0),
            ]);
        }
        if grip != 0 {
            packet
                .weapon
                .components
                .push(attachment(11, grip, Some(3), "bottom", position));
        }
        let expected_nodes = packet.weapon.components.len() + 1;
        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            session
                .configurations
                .cache
                .apply(catalog(), packet.catalog, packet)
                .unwrap();
            session.players[0].configuration = 2;
        }
        app.update();
        let root = descendants(app.world(), body)
            .into_iter()
            .find(|&e| app.world().get::<ConfiguredWeapon>(e).is_some())
            .unwrap();
        let pieces = descendants(app.world(), root)
            .into_iter()
            .filter(|&e| app.world().get::<configured::ConfiguredPiece>(e).is_some())
            .count();
        assert_eq!(pieces, expected_nodes);
        for aim in [false, true] {
            app.world_mut().resource_mut::<ClientSession>().players[0]
                .pose
                .ads = if aim { 1.0 } else { 0.0 };
            for _ in 0..30 {
                app.update();
                assert_arms(&app, body, root);
            }
        }
        for kind in [
            common::catalog::ManipulationKind::TacticalReload,
            common::catalog::ManipulationKind::EmptyReload,
            common::catalog::ManipulationKind::Inspect,
            common::catalog::ManipulationKind::Unequip,
            common::catalog::ManipulationKind::Equip,
        ] {
            for step in 0..=30 {
                {
                    let mut session = app.world_mut().resource_mut::<ClientSession>();
                    session.players[0].mechanics.kind = Some(kind);
                    session.players[0].mechanics.fraction = step as f32 / 30.0;
                    session.players[0].pose.ads = 0.0;
                }
                app.update();
                assert_arms(&app, body, root);
            }
        }
    }
}

#[test]
fn configured_stock_recoil_moves_plate_and_pocket_together() {
    let (mut app, body) = fixture(WeaponKind::Rifle);
    {
        let mut session = app.world_mut().resource_mut::<ClientSession>();
        session.players[0].pose.ads = 1.0;
        session.players[0].armor = ArmorState::from_loadout(Loadout {
            armor: ArmorKind::Heavy,
            ..default()
        });
    }
    for _ in 0..180 {
        app.update();
    }
    app.world_mut()
        .resource_mut::<feedback::Feedback>()
        .responses
        .get_mut(&7)
        .unwrap()
        .recoil = 0.04;
    app.update();
    let root = weapon_root(&app, body);
    assert_arms(&app, body, root);
    let gear = descendants(app.world(), body)
        .into_iter()
        .find(|&e| {
            app.world()
                .get::<EquipmentVisual>(e)
                .is_some_and(|g| !g.helmet)
        })
        .unwrap();
    let world = app.world();
    let stock =
        WeaponKind::Rifle.spec().assembly.anchors[&common::catalog::AnchorKind::StockContact];
    let contact = world
        .get::<GlobalTransform>(root)
        .unwrap()
        .mul_transform(world.get::<ConfiguredPose>(root).unwrap().solution.root)
        .transform_point(stock.translation);
    let plate_local = world
        .get::<GlobalTransform>(gear)
        .unwrap()
        .to_matrix()
        .inverse()
        .transform_point3(contact);
    assert!(
        (plate_local.z + 0.0275).abs() < 0.001,
        "stock slid off recoiling plate: {plate_local:?}"
    );
    assert!(world.resource::<feedback::Feedback>().responses[&7].recoil > 0.01);
}

#[test]
fn configured_remote_native_producer_keeps_anatomical_contacts_through_reload_and_recoil() {
    for weapon in [WeaponKind::Rifle, WeaponKind::Pistol, WeaponKind::Smg] {
        let (mut app, body) = fixture(weapon);
        let root = weapon_root(&app, body);
        let entities = descendants(app.world(), body);
        let meshes = app.world().resource::<Assets<Mesh>>().len();
        for empty in [false, true] {
            for step in 0..=60 {
                let t = step as f32 / 60.0;
                {
                    let mut session = app.world_mut().resource_mut::<ClientSession>();
                    let player = &mut session.players[0];
                    player.mechanics.kind = Some(if empty {
                        common::catalog::ManipulationKind::EmptyReload
                    } else {
                        common::catalog::ManipulationKind::TacticalReload
                    });
                    player.mechanics.fraction = t;
                    player.pose.reload_fraction = t;
                    player.pose.reload_empty = empty;
                    player.pose.crouch = if empty { 1.0 } else { 0.0 };
                    player.pose.aim_pitch = (t - 0.5) * 1.6;
                    player.pose.aim_roll = (t - 0.5) * 0.3;
                }
                app.world_mut()
                    .resource_mut::<feedback::Feedback>()
                    .responses
                    .get_mut(&7)
                    .unwrap()
                    .recoil = 0.025;
                app.update();
                assert_arms(&app, body, root);
                assert_eq!(descendants(app.world(), body), entities);
                assert_eq!(app.world().resource::<Assets<Mesh>>().len(), meshes);
            }
        }
        let request = app.world().get::<ConfiguredWeapon>(root).unwrap();
        assert_eq!((request.epoch, request.handle), (1, 1));
        assert!(!request.view_model);
    }
}

#[test]
fn configured_remote_native_producer_preserves_trauma_and_fall_visibility() {
    let (mut app, body) = fixture(WeaponKind::Rifle);
    let root = weapon_root(&app, body);
    assert_arms(&app, body, root);
    app.world_mut().resource_mut::<ClientSession>().players[0]
        .body
        .anatomy
        .separate(Segment::LeftUpperArm);
    app.update();
    assert_eq!(
        app.world().get::<Visibility>(root),
        Some(&Visibility::Inherited)
    );
    for child in descendants(app.world(), body) {
        if let Some(bone) = app.world().get::<BoneVisual>(child)
            && matches!(
                bone.segment,
                Segment::LeftUpperArm | Segment::LeftForearm | Segment::LeftHand
            )
        {
            assert_eq!(
                app.world().get::<Visibility>(child),
                Some(&Visibility::Hidden)
            );
        }
    }
    for mode in [PhysicalMode::Fallen, PhysicalMode::Recovering] {
        app.world_mut().resource_mut::<ClientSession>().players[0]
            .pose
            .mode = mode;
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(root),
            Some(&Visibility::Hidden)
        );
    }
    app.world_mut().resource_mut::<ClientSession>().players[0]
        .pose
        .mode = PhysicalMode::Standing;
    app.update();
    assert_eq!(
        app.world().get::<Visibility>(root),
        Some(&Visibility::Inherited)
    );
    app.world_mut().resource_mut::<ClientSession>().players[0]
        .body
        .anatomy
        .separate(Segment::RightUpperArm);
    app.update();
    assert_eq!(
        app.world().get::<Visibility>(root),
        Some(&Visibility::Hidden)
    );
    app.world_mut().resource_mut::<ClientSession>().players[0].alive = false;
    app.update();
    assert_eq!(
        app.world().get::<Visibility>(root),
        Some(&Visibility::Hidden)
    );
}

#[test]
fn configured_remote_native_revision_rebinds_magazine_identity_without_leaks() {
    let (mut app, body) = fixture(WeaponKind::Rifle);
    let root = weapon_root(&app, body);
    let original = descendants(app.world(), root);
    let meshes = app.world().resource::<Assets<Mesh>>().len();
    {
        let mut session = app.world_mut().resource_mut::<ClientSession>();
        let mut packet = session
            .configurations
            .cache
            .get(1, 1)
            .unwrap()
            .packet
            .clone();
        packet.handle = 2;
        packet.revision = 2;
        packet.weapon.components[0].node = 9;
        session
            .configurations
            .cache
            .apply(catalog(), packet.catalog, packet)
            .unwrap();
        session.players[0].configuration = 2;
        session.players[0].mechanics.kind = Some(common::catalog::ManipulationKind::TacticalReload);
        session.players[0].mechanics.fraction = 0.22;
    }
    app.update();
    assert_arms(&app, body, root);
    for old in original {
        assert!(app.world().get_entity(old).is_err());
    }
    let nodes: Vec<_> = descendants(app.world(), root)
        .into_iter()
        .filter_map(|e| {
            app.world()
                .get::<configured::ConfiguredPiece>(e)
                .map(|p| p.node)
        })
        .collect();
    assert_eq!(nodes, [0, 2, 3, 4, 5, 6, 7, 9]);
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), meshes);
    for _ in 0..30 {
        app.update();
    }
    assert_arms(&app, body, root);
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), meshes);
}
