use super::*;
use common::{
    catalog::Catalog,
    replication::{Configuration, catalog_fingerprint},
};

fn fixture() -> (App, Configuration) {
    let source = common::catalog::CATALOG_SOURCE
        .replacen(
            "sockets: [",
            r#"sockets: [
        (key: "grip", interface: (kind: Rail, standard: "fixture", size: 1),
            accepts: [Grip], positions: 3, pitch_m: 0.02,
            pose: (translation: (0.0, 0.0, -0.15), rotation: (0.0, 0.0, 0.0, 1.0))),
    "#,
            1,
        )
        .replacen(
            "components: [\n",
            r#"components: [
        (id: 104, key: "fixture_grip", kind: Grip, ammo: 201, capacity: 0, mass_kg: 0.1,
            assembly: (interface: (kind: Rail, standard: "fixture", size: 1),
                platform: Some("m4a1"), caliber: Some("556x45"), occupied_positions: 1,
                geometry: (anchors: [
                    (kind: SupportGrip, pose: (translation: (-0.04, -0.04, -0.01),
                        rotation: (0.0, 0.0, 0.24740396, 0.9689124))),
                ]))),
    "#,
            1,
        );
    let catalog = Catalog::parse("fixture", &source).unwrap();
    let fingerprint = catalog_fingerprint(&source);
    let packet = Configuration {
        epoch: 1,
        handle: 1,
        catalog: fingerprint,
        owner: 7,
        slot: common::EquipmentSlot::Primary,
        instance: common::WeaponInstanceId(42),
        revision: 1,
        weapon: catalog.configuration(WeaponId::M4A1).unwrap(),
    };
    let mut session = ClientSession::default();
    session.configurations.cache.begin_epoch(1).unwrap();
    session
        .configurations
        .cache
        .apply(&catalog, fingerprint, packet.clone())
        .unwrap();
    let mut next = packet.clone();
    next.handle = 2;
    next.revision = 2;
    next.weapon.components.push(common::catalog::Attachment {
        node: 7,
        definition: DefinitionId(104),
        parent: None,
        socket: "grip".into(),
        position: 2,
    });
    session
        .configurations
        .cache
        .apply(&catalog, fingerprint, next)
        .unwrap();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin))
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .insert_resource(session)
        .add_plugins(super::super::Plugin);
    let mut bindings = app.world_mut().resource_mut::<VisualBindings>();
    bindings
        .weapons
        .insert(WeaponId::M4A1, "pipeline_fixture".into());
    bindings
        .components
        .insert(DefinitionId(101), "pipeline_fixture".into());
    bindings
        .components
        .insert(DefinitionId(104), "pipeline_fixture".into());
    (app, packet)
}

fn input() -> ConfiguredHandling {
    ConfiguredHandling {
        preferred_root: Transform::from_xyz(0.0, 0.10, -0.12),
        shoulders: [Vec3::new(-0.30, 0.1, 0.0), Vec3::new(0.30, 0.1, 0.0)],
        poles: [Vec3::new(-0.8, -1.0, 0.0), Vec3::new(0.8, -1.0, 0.0)],
        stock_target: None,
        max_adjustment: 0.35,
        support_blends: Vec::new(),
        motions: Vec::new(),
        hidden_parts: Vec::new(),
        visible_hands: [true; 6],
    }
}

fn spawn(app: &mut App, view_model: bool, transform: Transform) -> Entity {
    app.world_mut()
        .spawn((
            ConfiguredWeapon {
                epoch: 1,
                handle: 1,
                view_model,
            },
            input(),
            transform,
            Visibility::Inherited,
        ))
        .id()
}

fn descendants(world: &World, root: Entity) -> Vec<Entity> {
    let mut result = Vec::new();
    if let Some(children) = world.get::<Children>(root) {
        for child in children.iter() {
            result.push(child);
            result.extend(descendants(world, child));
        }
    }
    result
}

#[test]
fn configured_visual_revisions_share_assets_and_replace_owned_nodes_atomically() {
    let (mut app, _) = fixture();
    let world = spawn(&mut app, false, Transform::default());
    let view = spawn(&mut app, true, Transform::default());
    app.update();
    for root in [world, view] {
        assert!(app.world().get::<ConfigurationVisualError>(root).is_none());
        let model = app.world().get::<Instance>(root).unwrap();
        let nodes: Vec<_> = model
            .pieces
            .values()
            .map(|p| {
                let identity = app.world().get::<ConfiguredPiece>(p.entity).unwrap();
                (identity.node, identity.definition)
            })
            .collect();
        assert_eq!(nodes, [(0, 1), (1, 101)]);
    }
    let handles = |app: &App, root| {
        descendants(app.world(), root)
            .into_iter()
            .filter_map(|e| app.world().get::<Mesh3d>(e).map(|m| m.0.id()))
            .collect::<Vec<_>>()
    };
    assert_eq!(handles(&app, world), handles(&app, view));
    let first_hand = {
        let model = app.world().get::<Instance>(world).unwrap();
        *app.world().get::<Transform>(model.hands[2]).unwrap()
    };
    let mesh_count = app.world().resource::<Assets<Mesh>>().len();
    let material_count = app.world().resource::<Assets<StandardMaterial>>().len();
    let original = descendants(app.world(), world);
    for _ in 0..60 {
        app.update();
    }
    assert_eq!(descendants(app.world(), world), original);
    app.world_mut()
        .get_mut::<ConfiguredWeapon>(world)
        .unwrap()
        .handle = 999;
    app.update();
    assert!(
        app.world()
            .get::<ConfigurationVisualError>(world)
            .unwrap()
            .0
            .contains("not available")
    );
    assert_eq!(descendants(app.world(), world), original);
    app.world_mut()
        .get_mut::<ConfiguredWeapon>(world)
        .unwrap()
        .handle = 2;
    app.update();
    assert_eq!(app.world().get::<Instance>(world).unwrap().revision, 2);
    let model = app.world().get::<Instance>(world).unwrap();
    assert_eq!(model.pieces.keys().copied().collect::<Vec<_>>(), [0, 1, 7]);
    let grip = &model.pieces[&7];
    assert_eq!(
        app.world()
            .get::<ConfiguredPiece>(grip.entity)
            .unwrap()
            .definition,
        104
    );
    assert!(grip.pose.translation.distance(Vec3::new(0.0, 0.0, -0.11)) < 1e-6);
    let hand = app.world().get::<Transform>(model.hands[2]).unwrap();
    assert!(hand.translation.distance(first_hand.translation) > 0.1);
    assert!(
        (hand
            .rotation
            .normalize()
            .angle_between(first_hand.rotation.normalize())
            - 0.5)
            .abs()
            < 1e-5
    );
    let root_pose = app.world().get::<Transform>(model.weapon_root).unwrap();
    // Oracle from rail placement and authored grip, independent of the resolver.
    assert!(
        hand.translation
            .distance(root_pose.transform_point(Vec3::new(-0.04, -0.04, -0.12)))
            < 2e-6
    );
    let trigger = app.world().get::<Transform>(model.hands[5]).unwrap();
    assert!(
        trigger
            .translation
            .distance(root_pose.transform_point(Vec3::new(0.025, -0.09, 0.035)))
            < 2e-6
    );
    for old in original {
        assert!(app.world().get_entity(old).is_err());
    }
    app.world_mut()
        .get_mut::<ConfiguredWeapon>(view)
        .unwrap()
        .handle = 2;
    app.update();
    assert_eq!(handles(&app, world), handles(&app, view));
    app.world_mut()
        .get_mut::<ConfiguredWeapon>(world)
        .unwrap()
        .handle = 1;
    app.update();
    assert_eq!(app.world().get::<Instance>(world).unwrap().revision, 2);
    assert!(
        app.world()
            .get::<ConfigurationVisualError>(world)
            .unwrap()
            .0
            .contains("stale")
    );
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), mesh_count);
    assert_eq!(
        app.world().resource::<Assets<StandardMaterial>>().len(),
        material_count
    );
    let children = descendants(app.world(), world);
    app.world_mut()
        .entity_mut(world)
        .remove::<ConfiguredWeapon>();
    app.update();
    for old in children {
        assert!(app.world().get_entity(old).is_err());
    }
    assert!(descendants(app.world(), world).is_empty());
}

#[test]
fn configured_visual_moving_socket_drives_both_hands_and_rigid_part_tree() {
    let (mut app, _) = fixture();
    let world = spawn(
        &mut app,
        false,
        Transform::from_xyz(2.0, 1.0, -3.0).with_rotation(Quat::from_rotation_y(0.8)),
    );
    let view = spawn(&mut app, true, Transform::default());
    app.update();
    for drop in [0.0, 0.04, 0.10] {
        for root in [world, view] {
            let mut handling = app.world_mut().get_mut::<ConfiguredHandling>(root).unwrap();
            handling.support_blends = vec![SocketBlend {
                address: SocketAddress {
                    node: 1,
                    socket: "magazine.grasp".into(),
                },
                weight: 1.0,
            }];
            handling.motions = vec![PartMotion {
                node: 1,
                part: "magazine".into(),
                delta: Transform::from_xyz(0.0, -drop, 0.0)
                    .with_rotation(Quat::from_rotation_x(0.2)),
            }];
        }
        app.update();
        for root in [world, view] {
            assert!(app.world().get::<ConfigurationVisualError>(root).is_none());
            let instance = app.world().get::<Instance>(root).unwrap();
            let piece = &instance.pieces[&1];
            let socket = descendants(app.world(), piece.entity)
                .into_iter()
                .find(|&e| {
                    app.world()
                        .get::<Name>(e)
                        .is_some_and(|n| n.as_str() == "magazine.grasp")
                })
                .unwrap();
            let socket_pose = app.world().get::<GlobalTransform>(socket).unwrap();
            let hand_pose = app
                .world()
                .get::<GlobalTransform>(instance.hands[2])
                .unwrap();
            assert!(socket_pose.translation().distance(hand_pose.translation()) < 2e-6);
            // Affine decomposition returns nearly-unit quaternions. angle_between
            // assumes unit inputs; identical length-0.9999999 values report 1.2 mrad.
            assert!(
                socket_pose
                    .rotation()
                    .normalize()
                    .angle_between(hand_pose.rotation().normalize())
                    < 0.001
            );
            for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
                assert!(
                    (socket_pose.affine().transform_vector3(axis)
                        - hand_pose.affine().transform_vector3(axis))
                    .length()
                        < 2e-6
                );
            }
            for i in [0, 3] {
                let upper = app
                    .world()
                    .get::<GlobalTransform>(instance.hands[i])
                    .unwrap();
                let lower = app
                    .world()
                    .get::<GlobalTransform>(instance.hands[i + 1])
                    .unwrap();
                let hand = app
                    .world()
                    .get::<GlobalTransform>(instance.hands[i + 2])
                    .unwrap();
                let elbow = upper.transform_point(Vec3::NEG_Y * 0.15);
                assert!(elbow.distance(lower.transform_point(Vec3::Y * 0.14)) < 2e-6);
                assert!(
                    hand.translation()
                        .distance(lower.transform_point(Vec3::NEG_Y * 0.14))
                        < 2e-6
                );
                assert!(upper.scale().distance(Vec3::ONE) < 1e-6);
                assert!(lower.scale().distance(Vec3::ONE) < 1e-6);
            }
        }
    }
    let before: Vec<_> = descendants(app.world(), view)
        .into_iter()
        .filter_map(|e| app.world().get::<Transform>(e).map(|t| (e, *t)))
        .collect();
    app.world_mut()
        .get_mut::<ConfiguredHandling>(view)
        .unwrap()
        .preferred_root
        .translation
        .z = -10.0;
    app.update();
    assert!(
        app.world()
            .get::<ConfigurationVisualError>(view)
            .unwrap()
            .0
            .contains("handling")
    );
    for (entity, pose) in before {
        assert_eq!(app.world().get::<Transform>(entity).unwrap(), &pose);
    }
}

fn local_poses(app: &App, root: Entity) -> Vec<(Entity, Transform)> {
    descendants(app.world(), root)
        .into_iter()
        .filter_map(|e| app.world().get::<Transform>(e).map(|t| (e, *t)))
        .collect()
}

fn assert_error(app: &App, root: Entity, expected: &str) {
    let message = &app.world().get::<ConfigurationVisualError>(root).unwrap().0;
    assert!(message.contains(expected), "{message}");
}

#[test]
fn configured_visual_rejected_replacement_preserves_the_entire_previous_pose() {
    let (mut app, _) = fixture();
    let root = spawn(&mut app, false, Transform::default());
    app.update();
    let original = local_poses(&app, root);
    app.world_mut()
        .get_mut::<ConfiguredWeapon>(root)
        .unwrap()
        .handle = 2;
    app.world_mut()
        .resource_mut::<VisualBindings>()
        .components
        .remove(&DefinitionId(104));
    app.update();
    assert_error(&app, root, "104 visual binding is missing");
    assert_eq!(local_poses(&app, root), original);
    app.world_mut()
        .resource_mut::<VisualBindings>()
        .components
        .insert(DefinitionId(104), "absent".into());
    app.update();
    assert_error(&app, root, "unknown voxel asset");
    assert_eq!(local_poses(&app, root), original);
    app.world_mut()
        .resource_mut::<VisualBindings>()
        .components
        .insert(DefinitionId(104), "pipeline_fixture".into());
    app.world_mut()
        .get_mut::<ConfiguredHandling>(root)
        .unwrap()
        .preferred_root
        .translation
        .z = -10.0;
    app.update();
    assert_error(&app, root, "handling");
    assert_eq!(local_poses(&app, root), original);
    assert_eq!(app.world().get::<Instance>(root).unwrap().revision, 1);
    *app.world_mut().get_mut::<ConfiguredHandling>(root).unwrap() = input();
    app.update();
    assert!(app.world().get::<ConfigurationVisualError>(root).is_none());
    assert_eq!(app.world().get::<Instance>(root).unwrap().revision, 2);
    for (old, _) in original {
        assert!(app.world().get_entity(old).is_err());
    }
}

#[test]
fn configured_visual_rejects_invalid_nodes_motions_and_missing_targets_atomically() {
    let (mut app, _) = fixture();
    let root = spawn(&mut app, true, Transform::default());
    app.update();
    let original = local_poses(&app, root);
    for (node, part, delta, message) in [
        (
            44,
            "magazine",
            Transform::IDENTITY,
            "unknown configuration node",
        ),
        (1, "absent", Transform::IDENTITY, "unknown moving part"),
        (1, "receiver", Transform::IDENTITY, "invalid motion"),
        (
            1,
            "magazine",
            Transform::from_scale(Vec3::splat(2.0)),
            "invalid motion",
        ),
    ] {
        app.world_mut()
            .get_mut::<ConfiguredHandling>(root)
            .unwrap()
            .motions = vec![PartMotion {
            node,
            part: part.into(),
            delta,
        }];
        app.update();
        assert_error(&app, root, message);
        assert_eq!(local_poses(&app, root), original);
    }
    *app.world_mut().get_mut::<ConfiguredHandling>(root).unwrap() = input();
    let piece = app.world().get::<Instance>(root).unwrap().pieces[&1].entity;
    app.world_mut()
        .get_mut::<ConfiguredPiece>(piece)
        .unwrap()
        .node = 99;
    app.update();
    assert_error(&app, root, "node identity");
    assert_eq!(local_poses(&app, root), original);
    app.world_mut()
        .get_mut::<ConfiguredPiece>(piece)
        .unwrap()
        .node = 1;
    app.world_mut()
        .get_mut::<ConfiguredPiece>(piece)
        .unwrap()
        .definition = 999;
    app.update();
    assert_error(&app, root, "node identity");
    assert_eq!(local_poses(&app, root), original);
    app.world_mut()
        .get_mut::<ConfiguredPiece>(piece)
        .unwrap()
        .definition = 101;
    let hand = app.world().get::<Instance>(root).unwrap().hands[5];
    app.world_mut().entity_mut(hand).remove::<Transform>();
    let before = local_poses(&app, root);
    app.world_mut()
        .get_mut::<ConfiguredHandling>(root)
        .unwrap()
        .preferred_root
        .translation
        .x += 0.01;
    app.update();
    assert_error(&app, root, "pose target is missing");
    assert_eq!(local_poses(&app, root), before);
}

#[test]
fn configured_visual_failed_initial_request_cleans_error_without_an_instance() {
    let (mut app, _) = fixture();
    let root = spawn(&mut app, false, Transform::default());
    app.world_mut()
        .get_mut::<ConfiguredWeapon>(root)
        .unwrap()
        .handle = 999;
    app.update();
    assert_error(&app, root, "not available");
    assert!(app.world().get::<Instance>(root).is_none());
    assert!(descendants(app.world(), root).is_empty());
    app.world_mut()
        .entity_mut(root)
        .remove::<ConfiguredWeapon>();
    app.update();
    assert!(app.world().get::<ConfigurationVisualError>(root).is_none());
    assert!(descendants(app.world(), root).is_empty());
}

#[test]
fn configured_visual_baseline_bindings_preserve_native_geometry_and_contacts() {
    use common::catalog::catalog;
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin))
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<ClientSession>()
        .add_plugins(super::super::Plugin);
    app.world_mut()
        .resource_mut::<ClientSession>()
        .configurations
        .cache
        .begin_epoch(1)
        .unwrap();
    let mut roots = Vec::new();
    for (index, definition) in [WeaponId::M4A1, WeaponId::M1911, WeaponId::MP5]
        .into_iter()
        .enumerate()
    {
        let handle = index as u32 + 1;
        let config = Configuration {
            epoch: 1,
            handle,
            catalog: catalog_fingerprint(common::catalog::CATALOG_SOURCE),
            owner: 7,
            slot: if definition == WeaponId::M1911 {
                common::EquipmentSlot::Sidearm
            } else {
                common::EquipmentSlot::Primary
            },
            instance: common::WeaponInstanceId(index as u64 + 1),
            revision: 1,
            weapon: catalog().configuration(definition).unwrap(),
        };
        app.world_mut()
            .resource_mut::<ClientSession>()
            .configurations
            .cache
            .apply(catalog(), config.catalog, config)
            .unwrap();
        for view_model in [false, true] {
            let entity = spawn(&mut app, view_model, Transform::IDENTITY);
            app.world_mut()
                .get_mut::<ConfiguredWeapon>(entity)
                .unwrap()
                .handle = handle;
            roots.push((entity, definition, view_model));
        }
    }
    app.update();
    for (entity, definition, view_model) in roots {
        assert!(
            app.world()
                .get::<ConfigurationVisualError>(entity)
                .is_none()
        );
        let model = app.world().get::<Instance>(entity).unwrap();
        let rig = common::weapon_animation::WeaponRig::for_weapon(
            catalog().weapon(definition).unwrap().class,
        );
        let root = app
            .world()
            .get::<GlobalTransform>(model.weapon_root)
            .unwrap();
        for (index, contact) in [(2, rig.support), (5, rig.trigger)] {
            let hand = app
                .world()
                .get::<GlobalTransform>(model.hands[index])
                .unwrap();
            assert!(hand.translation().distance(root.transform_point(contact)) < 2e-6);
        }
        let magazine = &model.pieces[&1];
        assert!(magazine.pose.translation.distance(rig.magazine) < 1e-6);
        let root_asset = &app.world().resource::<VoxelCache>().assets[&model.pieces[&0].asset];
        let muzzle = root_asset
            .sockets
            .iter()
            .find(|s| s.id == "muzzle")
            .unwrap();
        assert!(muzzle.pose.transform().translation.distance(rig.muzzle) < 1e-6);
        assert!(root_asset.parts.iter().any(|p| p.id == "bolt" && p.movable));
        assert!(
            root_asset
                .parts
                .iter()
                .any(|p| p.id == "control" && p.movable)
        );
        for child in descendants(app.world(), entity) {
            if app.world().get::<Mesh3d>(child).is_some() {
                assert_eq!(
                    app.world().get::<RenderLayers>(child).unwrap(),
                    &RenderLayers::layer(if view_model {
                        VIEW_MODEL_RENDER_LAYER
                    } else {
                        0
                    })
                );
            }
        }
    }
    // Six assets, each loaded once for two perspectives; four surfaces per root
    // and one per magazine, plus three shared arm/palm meshes.
    assert_eq!(app.world().resource::<VoxelCache>().assets.len(), 6);
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 18);
}
