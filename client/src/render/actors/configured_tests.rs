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

fn assert_arms(app: &App, body: Entity, weapon: Entity) {
    assert!(
        app.world()
            .get::<configured::ConfigurationVisualError>(weapon)
            .is_none()
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
        let entity = descendants(app.world(), weapon)
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
    let spec = &session
        .configurations
        .cache
        .get(request.epoch, request.handle)
        .unwrap()
        .resolved;
    let recoil = app.world().resource::<feedback::Feedback>().responses[&7].recoil;
    let logical = handling::configured_contacts(spec, action, recoil, &player.mechanics).unwrap();
    let logical_support = world_root.mul_transform(logical.support);
    let actual_support = world_root.mul_transform(Transform::from_translation(rig.support));
    let expected_rotation = actual_support
        .rotation()
        .slerp(magazine.rotation(), action.magazine_grip)
        .slerp(control.rotation(), action.control_grip);
    assert!(
        logical_support.translation().distance(
            world_root
                .transform_point(rig.support)
                .lerp(magazine.translation(), action.magazine_grip)
                .lerp(control.translation(), action.control_grip)
        ) < 2e-6
    );
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        assert!(
            (logical_support.rotation() * axis - expected_rotation * axis).length() < 2e-6,
            "logical authority contact must match baked mechanism socket"
        );
    }
    let expected = world_root
        .transform_point(rig.support)
        .lerp(magazine.translation(), action.magazine_grip)
        .lerp(control.translation(), action.control_grip);
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
    assert_eq!(nodes, [0, 9]);
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), meshes);
    for _ in 0..30 {
        app.update();
    }
    assert_arms(&app, body, root);
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), meshes);
}
