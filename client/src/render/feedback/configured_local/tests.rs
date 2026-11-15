use super::*;
use crate::render::{
    self,
    voxel::{
        self,
        configured::{ConfigurationVisualError, ConfiguredPiece},
    },
};
use common::{
    catalog::{ManipulationKind, catalog},
    replication::{Configuration, catalog_fingerprint},
};

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

fn named(app: &App, root: Entity, name: &str) -> Entity {
    descendants(app.world(), root)
        .into_iter()
        .find(|&e| {
            app.world()
                .get::<Name>(e)
                .is_some_and(|n| n.as_str() == name)
        })
        .unwrap()
}

struct Fixture {
    app: App,
    view: Entity,
    root: Entity,
    camera: Entity,
    world_camera: Entity,
    base: Transform,
}

#[test]
fn configured_local_stock_and_clothing_keep_contacts_and_ads_sight_line() {
    for weapon in [WeaponKind::Rifle, WeaponKind::Smg] {
        for armor in [
            common::combat::ArmorKind::None,
            common::combat::ArmorKind::Light,
            common::combat::ArmorKind::Heavy,
        ] {
            let mut f = Fixture::new(weapon, 1.6);
            f.app.world_mut().resource_mut::<ClientSession>().players[0].armor =
                common::combat::ArmorState::from_loadout(common::combat::Loadout {
                    armor,
                    ..default()
                });
            for _ in 0..90 {
                f.step();
            }
            f.app.world_mut().resource_mut::<LocalInput>().aim = true;
            for _ in 0..180 {
                f.step();
                f.assert_contacts();
            }
            let pose = f.app.world().get::<ConfiguredPose>(f.root).unwrap();
            let input = f
                .app
                .world()
                .get::<voxel::configured::ConfiguredHandling>(f.root)
                .unwrap();
            let stock = weapon.spec().assembly.anchors[&common::catalog::AnchorKind::StockContact];
            let contact = pose.solution.root.transform_point(stock.translation);
            assert!(contact.distance(input.stock_target.unwrap()) < 2e-6);
            assert!(
                contact.distance(stock.translation) < 2e-6,
                "the camera framing reference must retain the accepted sight line"
            );
            if weapon == WeaponKind::Rifle {
                let world = f.app.world();
                let to_eye = world
                    .get::<GlobalTransform>(f.camera)
                    .unwrap()
                    .to_matrix()
                    .inverse();
                for name in ["sight.front", "sight.rear"] {
                    let actual = world
                        .get::<GlobalTransform>(named(&f.app, f.root, name))
                        .unwrap();
                    let eye = to_eye.transform_point3(actual.translation());
                    assert!(
                        eye.x.abs() < 0.00025 && eye.y.abs() < 0.00025,
                        "{name}: {eye:?}"
                    );
                    assert!(eye.z < -0.1);
                }
                let outlet = world
                    .get::<GlobalTransform>(named(&f.app, f.root, "muzzle_device.exit"))
                    .unwrap();
                let muzzle = world
                    .get::<GlobalTransform>(f.root)
                    .unwrap()
                    .mul_transform(pose.muzzle);
                assert!(muzzle.translation().distance(outlet.translation()) < 2e-6);
            }
            let depth = match armor {
                common::combat::ArmorKind::None => 0.145,
                common::combat::ArmorKind::Light => 0.1975,
                common::combat::ArmorKind::Heavy => 0.203,
            };
            let rotation = Quat::from_rotation_y(-1.05);
            let torso = stock.translation - rotation * Vec3::new(0.14, 0.1, -depth);
            for (side, shoulder) in [-1.0, 1.0].into_iter().zip(pose.shoulders) {
                assert!(
                    shoulder.distance(torso + rotation * Vec3::new(side * 0.30, 0.1, 0.0)) < 2e-6
                );
            }
            f.app.world_mut().resource_mut::<LocalInput>().aim = false;
            for _ in 0..180 {
                f.step();
                f.assert_contacts();
            }
        }
    }
}

impl Fixture {
    fn new(weapon: WeaponKind, aspect: f32) -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            bevy::camera::visibility::VisibilityPlugin,
        ))
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f64(1.0 / 120.0),
        ))
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
        .init_resource::<Feedback>()
        .insert_resource(PlayerId(7))
        .init_resource::<LocalInput>()
        .add_plugins(voxel::Plugin)
        .add_systems(
            Update,
            (
                advance,
                render::fit_view_aspect,
                animate_local,
                prepare.in_set(PrepareLocal),
                render::sync_view_weapon_visibility,
            )
                .chain(),
        )
        .add_systems(Update, flash.after(voxel::configured::PoseConfigured));
        let mut session = ClientSession::default();
        session.configurations.cache.begin_epoch(1).unwrap();
        let fingerprint = catalog_fingerprint(common::catalog::CATALOG_SOURCE);
        let slot = if weapon == WeaponKind::Pistol {
            EquipmentSlot::Sidearm
        } else {
            EquipmentSlot::Primary
        };
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
                    slot,
                    instance: WeaponInstanceId(42),
                    revision: 1,
                    weapon: catalog().configuration(weapon.definition()).unwrap(),
                },
            )
            .unwrap();
        let mut player = ClientData {
            id: 7,
            alive: true,
            weapon,
            weapon_id: weapon.definition(),
            weapon_slot: slot,
            weapon_instance: WeaponInstanceId(42),
            configuration: 1,
            ..default()
        };
        player.mechanics.magazine_inserted = true;
        player.mechanics.equipped = true;
        session.players.push(player);
        app.insert_resource(session);
        app.world_mut().resource_mut::<LocalInput>().weapon = slot;
        app.world_mut().spawn((
            LocalPlayer,
            PlayerVisualState {
                alive: true,
                weapon,
                ..default()
            },
        ));
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let view = app
            .world_mut()
            .spawn(render::weapon_view_model(
                &mut meshes,
                &mut materials,
                weapon,
                Visibility::Inherited,
            ))
            .id();
        app.insert_resource(meshes).insert_resource(materials);
        let root = descendants(app.world(), view)
            .into_iter()
            .find(|&e| app.world().get::<WeaponMeshRoot>(e).is_some())
            .unwrap();
        let base = Transform::from_xyz(0.0, PLAYER_VIEW_HEIGHT, 0.0)
            .with_rotation(Quat::from_euler(EulerRot::YXZ, 0.2, 0.85, -0.18));
        let camera = app
            .world_mut()
            .spawn((
                ViewModelCamera,
                base,
                Projection::from(PerspectiveProjection {
                    fov: 65.0_f32.to_radians(),
                    aspect_ratio: aspect,
                    ..default()
                }),
            ))
            .id();
        let world_camera = app.world_mut().spawn((WorldModelCamera, base)).id();
        let mut fixture = Self {
            app,
            view,
            root,
            camera,
            world_camera,
            base,
        };
        fixture.step();
        fixture
    }

    fn step(&mut self) {
        for entity in [self.view, self.camera, self.world_camera] {
            self.app.world_mut().entity_mut(entity).insert(self.base);
        }
        self.app.update();
    }

    fn assert_contacts(&self) {
        let world = self.app.world();
        assert!(
            world.get::<ConfigurationVisualError>(self.root).is_none(),
            "{:?}",
            world
                .get::<ConfigurationVisualError>(self.root)
                .map(|e| &e.0)
        );
        let solution = world.get::<ConfiguredPose>(self.root).unwrap();
        let global = world.get::<GlobalTransform>(self.root).unwrap();
        for (side, shoulder) in ["support", "trigger"].into_iter().zip(solution.shoulders) {
            let poses = ["upper", "forearm", "palm"].map(|segment| {
                *world
                    .get::<GlobalTransform>(named(
                        &self.app,
                        self.root,
                        &format!("configured-{side}-{segment}"),
                    ))
                    .unwrap()
            });
            assert!(
                poses[0]
                    .transform_point(Vec3::Y * 0.15)
                    .distance(global.transform_point(shoulder))
                    < 3e-6
            );
            assert!(
                poses[0]
                    .transform_point(Vec3::NEG_Y * 0.15)
                    .distance(poses[1].transform_point(Vec3::Y * 0.14))
                    < 3e-6
            );
            assert!(
                poses[1]
                    .transform_point(Vec3::NEG_Y * 0.14)
                    .distance(poses[2].translation())
                    < 3e-6
            );
        }
        let action = world.resource::<Feedback>().responses[&7].action.unwrap();
        let player = &world.resource::<ClientSession>().players[0];
        let rig = WeaponRig::for_weapon(player.weapon);
        let root = global.mul_transform(solution.solution.root);
        let support = world
            .get::<GlobalTransform>(named(&self.app, self.root, "configured-support-palm"))
            .unwrap();
        let trigger = world
            .get::<GlobalTransform>(named(&self.app, self.root, "configured-trigger-palm"))
            .unwrap();
        let mag = world
            .get::<GlobalTransform>(named(&self.app, self.root, "magazine.grasp"))
            .unwrap();
        let control = world
            .get::<GlobalTransform>(named(&self.app, self.root, "control.grasp"))
            .unwrap();
        let rest_support = if player.weapon == WeaponKind::Rifle {
            let grip_root = descendants(world, self.root)
                .into_iter()
                .find(|&e| {
                    world
                        .get::<ConfiguredPiece>(e)
                        .is_some_and(|p| matches!(p.definition, 134 | 135))
                })
                .unwrap_or(self.root);
            world
                .get::<GlobalTransform>(named(&self.app, grip_root, "grip.support"))
                .unwrap()
                .translation()
        } else {
            root.transform_point(rig.support)
        };
        let rest_trigger = if player.weapon == WeaponKind::Rifle {
            world
                .get::<GlobalTransform>(named(&self.app, self.root, "grip.trigger"))
                .unwrap()
                .translation()
        } else {
            root.transform_point(rig.trigger)
        };
        assert!(
            support.translation().distance(
                rest_support
                    .lerp(mag.translation(), action.magazine_grip)
                    .lerp(control.translation(), action.control_grip)
            ) < 3e-6
        );
        assert!(trigger.translation().distance(rest_trigger) < 3e-6);
    }

    fn assert_pistol_frame(&self, aspect: f32) {
        use bevy::mesh::VertexAttributeValues;
        let world = self.app.world();
        let meshes = world.resource::<Assets<Mesh>>();
        let to_eye = world
            .get::<GlobalTransform>(self.camera)
            .unwrap()
            .to_matrix()
            .inverse();
        let mut count = 0;
        for entity in descendants(world, self.root) {
            let Some(mesh) = world.get::<Mesh3d>(entity) else {
                continue;
            };
            let name = world.get::<Name>(entity).map(|n| n.as_str()).unwrap_or("");
            if name.ends_with("upper")
                || name.ends_with("forearm")
                || world.get::<Flash>(entity).is_some()
                || !world.get::<InheritedVisibility>(entity).unwrap().get()
            {
                continue;
            }
            let VertexAttributeValues::Float32x3(vertices) = meshes
                .get(&mesh.0)
                .unwrap()
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
            else {
                panic!("mesh vertices")
            };
            let pose = to_eye * world.get::<GlobalTransform>(entity).unwrap().to_matrix();
            for vertex in vertices {
                let p = pose.transform_point3(Vec3::from(*vertex));
                let vertical = (65.0_f32.to_radians() * 0.5).tan();
                assert!(
                    p.z < -0.1
                        && p.x.abs() <= -p.z * vertical * aspect * 0.941
                        && p.y.abs() <= -p.z * vertical * 0.941,
                    "clipped aspect={aspect} {p:?}"
                );
                count += 1;
            }
        }
        assert!(count > 1000, "actual baked weapon and both palms");
    }
}

#[test]
fn configured_local_m4a1_kit_extremes_keep_rendered_contacts_through_ads_and_reload() {
    use common::catalog::{Attachment, DefinitionId};
    for (short, suppressed, grip) in [(true, false, 0), (false, true, 134), (false, false, 135)] {
        let mut f = Fixture::new(WeaponKind::Rifle, 1.6);
        let mut packet = f
            .app
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
            packet.weapon.components.push(attachment(
                11,
                grip,
                Some(3),
                "bottom",
                if grip == 134 { 4 } else { 1 },
            ));
        }
        {
            let mut session = f.app.world_mut().resource_mut::<ClientSession>();
            session
                .configurations
                .cache
                .apply(catalog(), packet.catalog, packet)
                .unwrap();
            session.players[0].configuration = 2;
        }
        for _ in 0..90 {
            f.step();
        }
        f.assert_contacts();
        for aim in [true, false] {
            f.app.world_mut().resource_mut::<LocalInput>().aim = aim;
            for _ in 0..90 {
                f.step();
                f.assert_contacts();
            }
        }
        for kind in [
            ManipulationKind::TacticalReload,
            ManipulationKind::EmptyReload,
        ] {
            for step in 0..=30 {
                {
                    let mut session = f.app.world_mut().resource_mut::<ClientSession>();
                    session.players[0].mechanics.kind = Some(kind);
                    session.players[0].mechanics.fraction = step as f32 / 30.0;
                }
                f.step();
                f.assert_contacts();
            }
        }
    }
}

#[test]
fn configured_local_native_reload_contacts_and_pistol_framing_use_actual_baked_hierarchy() {
    for weapon in [WeaponKind::Pistol, WeaponKind::Rifle, WeaponKind::Smg] {
        for aspect in [9.0 / 16.0, 1.6, 21.0 / 9.0] {
            let mut f = Fixture::new(weapon, aspect);
            let meshes = f.app.world().resource::<Assets<Mesh>>().len();
            let entities = descendants(f.app.world(), f.root);
            // Finish the initial draw before measuring the framing envelope.
            for _ in 0..90 {
                f.step();
            }
            for empty in [false, true] {
                let mut previous = None;
                for step in 0..=180 {
                    {
                        let mut session = f.app.world_mut().resource_mut::<ClientSession>();
                        let p = &mut session.players[0];
                        p.mechanics.kind = Some(if empty {
                            ManipulationKind::EmptyReload
                        } else {
                            ManipulationKind::TacticalReload
                        });
                        p.mechanics.fraction = step as f32 / 180.0;
                        p.pose.crouch = if empty { 1.0 } else { 0.0 };
                    }
                    f.app
                        .world_mut()
                        .resource_mut::<Feedback>()
                        .responses
                        .get_mut(&7)
                        .unwrap()
                        .recoil = 0.025;
                    f.step();
                    f.assert_contacts();
                    if weapon == WeaponKind::Pistol {
                        f.assert_pistol_frame(aspect);
                        let view = f.app.world().get::<Transform>(f.view).unwrap();
                        if let Some(previous) = previous {
                            assert!(
                                view.translation.distance(previous) < 0.035,
                                "framing must not snap when the magazine changes visibility"
                            );
                        }
                        previous = Some(view.translation);
                    }
                    assert_eq!(f.app.world().resource::<Assets<Mesh>>().len(), meshes);
                    assert_eq!(descendants(f.app.world(), f.root), entities);
                }
            }
            f.app.world_mut().resource_mut::<ClientSession>().players[0]
                .mechanics
                .kind = None;
            f.app.world_mut().resource_mut::<LocalInput>().aim = true;
            for _ in 0..180 {
                f.step();
            }
            f.assert_contacts();
            let solution = f.app.world().get::<ConfiguredPose>(f.root).unwrap();
            assert!(
                solution.solution.root.translation.length() < 1e-6,
                "default ADS must not move the authored sight line"
            );
            if weapon == WeaponKind::Pistol {
                let eye = f.app.world().get::<GlobalTransform>(f.camera).unwrap();
                let root = f
                    .app
                    .world()
                    .get::<GlobalTransform>(f.root)
                    .unwrap()
                    .mul_transform(solution.solution.root);
                let sight = (eye.to_matrix().inverse() * root.to_matrix())
                    .transform_point3(Vec3::new(0.0, 0.118, 0.005));
                assert!(
                    sight.x.abs() < 0.0001 && sight.y.abs() < 0.0001,
                    "ADS {sight:?}"
                );
            }
        }
    }
}

#[test]
fn configured_local_native_revision_and_trauma_preserve_identity_and_visibility() {
    let mut f = Fixture::new(WeaponKind::Rifle, 1.6);
    let old: Vec<_> = descendants(f.app.world(), f.root)
        .into_iter()
        .filter(|&e| f.app.world().get::<ConfiguredPiece>(e).is_some())
        .collect();
    {
        let mut session = f.app.world_mut().resource_mut::<ClientSession>();
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
        session.players[0]
            .body
            .anatomy
            .separate(common::humanoid::Segment::LeftUpperArm);
    }
    f.step();
    for entity in old {
        assert!(f.app.world().get_entity(entity).is_err());
    }
    let nodes: Vec<_> = descendants(f.app.world(), f.root)
        .into_iter()
        .filter_map(|e| f.app.world().get::<ConfiguredPiece>(e).map(|p| p.node))
        .collect();
    assert_eq!(nodes, [0, 2, 3, 4, 5, 6, 7, 9]);
    for part in ["upper", "forearm", "palm"] {
        let e = named(&f.app, f.root, &format!("configured-support-{part}"));
        assert!(!f.app.world().get::<InheritedVisibility>(e).unwrap().get());
    }
    f.app.world_mut().resource_mut::<ClientSession>().players[0]
        .pose
        .mode = PhysicalMode::Fallen;
    f.app.world_mut().resource_mut::<ClientSession>().players[0]
        .pose
        .mode_time = 0.3;
    f.step();
    assert_eq!(
        f.app.world().get::<Visibility>(f.root),
        Some(&Visibility::Hidden)
    );
}

#[test]
fn configured_local_rifle_open_bolt_keeps_handle_and_outlet_fixed() {
    let mut f = Fixture::new(WeaponKind::Rifle, 1.6);
    for _ in 0..90 {
        f.step();
    }
    let bolt = named(&f.app, f.root, "bolt");
    let handle = named(&f.app, f.root, "charging_handle");
    let outlet = named(&f.app, f.root, "muzzle_device.exit");
    let rest_bolt = *f.app.world().get::<Transform>(bolt).unwrap();
    let rest_handle = *f.app.world().get::<Transform>(handle).unwrap();
    let rest_outlet = *f.app.world().get::<Transform>(outlet).unwrap();
    f.app.world_mut().resource_mut::<ClientSession>().players[0]
        .mechanics
        .bolt_open = true;
    for _ in 0..30 {
        f.step();
        f.assert_contacts();
        let moved = f.app.world().get::<Transform>(bolt).unwrap();
        assert!((moved.translation - rest_bolt.translation - Vec3::Z * 0.055).length() < 1e-6);
        assert_eq!(
            *f.app.world().get::<Transform>(handle).unwrap(),
            rest_handle
        );
        assert_eq!(
            *f.app.world().get::<Transform>(outlet).unwrap(),
            rest_outlet
        );
    }
}

#[test]
fn configured_local_native_interruption_keeps_removed_magazine_hidden_and_empty_slide_open() {
    let mut f = Fixture::new(WeaponKind::Pistol, 1.6);
    for _ in 0..90 {
        f.step();
    }
    {
        let mut session = f.app.world_mut().resource_mut::<ClientSession>();
        session.players[0].mechanics.kind = Some(ManipulationKind::EmptyReload);
        session.players[0].mechanics.fraction = 0.3;
    }
    for _ in 0..30 {
        f.step();
    }
    {
        let mut session = f.app.world_mut().resource_mut::<ClientSession>();
        session.players[0].mechanics.kind = None;
        session.players[0].mechanics.magazine_inserted = false;
        session.players[0].mechanics.bolt_open = true;
    }
    for _ in 0..90 {
        f.step();
    }
    let mag = named(&f.app, f.root, "magazine");
    assert!(!f.app.world().get::<InheritedVisibility>(mag).unwrap().get());
    let bolt = named(&f.app, f.root, "bolt");
    assert!((f.app.world().get::<Transform>(bolt).unwrap().translation.z - 0.045).abs() < 1e-6);
    f.assert_contacts();
}
