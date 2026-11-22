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
fn configured_local_first_person_pistol_arms_extend_from_behind_the_grips() {
    for aspect in [9.0 / 16.0, 1.6, 21.0 / 9.0] {
        let mut f = Fixture::new(WeaponKind::Pistol, aspect);
        for aim in [false, true] {
            f.app.world_mut().resource_mut::<LocalInput>().aim = aim;
            for _ in 0..90 {
                f.step();
            }
            f.assert_contacts();
            f.assert_pistol_frame(aspect);
            let world = f.app.world();
            let to_eye = world
                .get::<GlobalTransform>(f.camera)
                .unwrap()
                .to_matrix()
                .inverse();
            for side in ["support", "trigger"] {
                let forearm = world
                    .get::<GlobalTransform>(named(
                        &f.app,
                        f.root,
                        &format!("configured-{side}-forearm"),
                    ))
                    .unwrap();
                let elbow = to_eye.transform_point3(forearm.transform_point(Vec3::Y * 0.14));
                let wrist = to_eye.transform_point3(forearm.transform_point(Vec3::NEG_Y * 0.14));
                assert!(
                    elbow.z > wrist.z + 0.10,
                    "{side} forearm must extend toward the gun, aim={aim}: elbow={elbow:?}, wrist={wrist:?}"
                );
                assert!(elbow.y < wrist.y, "{side} elbow must stay below its grip");
            }
        }
    }
}

#[test]
fn configured_local_first_person_rifle_receiver_clears_near_plane_through_recoil() {
    use bevy::mesh::VertexAttributeValues;
    let mut f = Fixture::new(WeaponKind::Rifle, 1.6);
    for pitch in [-1.2, 0.0, 1.2] {
        f.base.rotation = Quat::from_euler(EulerRot::YXZ, 0.2, pitch, -0.18);
        for aim in [false, true] {
            f.app.world_mut().resource_mut::<LocalInput>().aim = aim;
            for _ in 0..90 {
                f.step();
            }
            for recoil in [0.0, 0.025, 0.055, 0.11] {
                f.app
                    .world_mut()
                    .resource_mut::<Feedback>()
                    .responses
                    .get_mut(&7)
                    .unwrap()
                    .recoil = recoil;
                f.step();
                f.assert_contacts();
                let world = f.app.world();
                let to_eye = world
                    .get::<GlobalTransform>(f.camera)
                    .unwrap()
                    .to_matrix()
                    .inverse();
                let Projection::Perspective(projection) =
                    world.get::<Projection>(f.camera).unwrap()
                else {
                    panic!("perspective");
                };
                let meshes = world.resource::<Assets<Mesh>>();
                let mut count = 0;
                for name in ["receiver", "rail", "bolt", "charging_handle", "rear_sight"] {
                    let part = named(&f.app, f.root, name);
                    // Mesh surfaces are direct children; nested part pivots
                    // include the stock extension, which belongs behind the eye.
                    for entity in world.get::<Children>(part).unwrap().iter() {
                        let Some(mesh) = world.get::<Mesh3d>(entity) else {
                            continue;
                        };
                        let Some(VertexAttributeValues::Float32x3(vertices)) = meshes
                            .get(&mesh.0)
                            .unwrap()
                            .attribute(Mesh::ATTRIBUTE_POSITION)
                        else {
                            panic!("positions");
                        };
                        let pose =
                            to_eye * world.get::<GlobalTransform>(entity).unwrap().to_matrix();
                        for vertex in vertices {
                            let point = pose.transform_point3(Vec3::from(*vertex));
                            assert!(
                                point.z <= -projection.near - 0.009,
                                "{name} clipped during recoil={recoil}, aim={aim}, pitch={pitch}: {point:?}"
                            );
                            count += 1;
                        }
                    }
                }
                assert!(count > 100, "actual receiver, action and rear sight meshes");
            }
        }
    }
}

#[test]
fn configured_local_inspect_frames_real_assembly_and_returns_continuously() {
    for aspect in [4.0 / 3.0, 1.6, 16.0 / 9.0, 21.0 / 9.0] {
        for fps in [30, 60, 165] {
            let mut f = Fixture::new(WeaponKind::Rifle, aspect);
            let mut idle = Fixture::new(WeaponKind::Rifle, aspect);
            for fixture in [&mut f, &mut idle] {
                fixture
                    .app
                    .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                        std::time::Duration::from_secs_f64(1.0 / fps as f64),
                    ));
            }
            let before = f.app.world().resource::<ClientSession>().players[0].mechanics;
            let pieces = descendants(f.app.world(), f.root);
            let trigger = named(&f.app, f.root, "configured-trigger-palm");
            let mut previous = Some(
                f.app
                    .world()
                    .get::<GlobalTransform>(trigger)
                    .unwrap()
                    .translation(),
            );
            let mut sides = [false; 2];
            let mut side_range = [1.0_f32, -1.0_f32];
            for frame in 0..=fps * 4 {
                let fraction = (frame as f32 / fps as f32 / 2.8).min(1.0);
                {
                    let mut session = f.app.world_mut().resource_mut::<ClientSession>();
                    session.players[0].mechanics.kind =
                        (fraction < 1.0).then_some(ManipulationKind::Inspect);
                    session.players[0].mechanics.fraction = fraction;
                }
                f.step();
                idle.step();
                f.assert_contacts();
                let world = f.app.world();
                let response = &world.resource::<Feedback>().responses[&7];
                let action = response.action.unwrap();
                let receiver = world
                    .get::<GlobalTransform>(named(&f.app, f.root, "receiver"))
                    .unwrap();
                let eye = world
                    .get::<GlobalTransform>(f.camera)
                    .unwrap()
                    .translation();
                // Test the real receiver faces against the eye direction, not
                // an assumed yaw angle: an external inspection also rolls it.
                let side =
                    (receiver.rotation() * Vec3::X).dot((eye - receiver.translation()).normalize());
                if (0.30..=0.33).contains(&fraction) {
                    let to_eye = world
                        .get::<GlobalTransform>(f.camera)
                        .unwrap()
                        .to_matrix()
                        .inverse();
                    let arm = |name, endpoint| {
                        let bone = world
                            .get::<GlobalTransform>(named(&f.app, f.root, name))
                            .unwrap();
                        to_eye.transform_point3(bone.transform_point(endpoint))
                    };
                    let shoulder = arm("configured-support-upper", Vec3::Y * 0.15);
                    let wrist = arm("configured-support-forearm", Vec3::NEG_Y * 0.14);
                    assert!(
                        shoulder.y < wrist.y,
                        "support sleeve must extend from below its grip during inspect: \
                         shoulder={shoulder:?}, wrist={wrist:?}, fraction={fraction}"
                    );
                }
                sides[0] |= side > 0.15;
                sides[1] |= side < -0.15;
                side_range[0] = side_range[0].min(side);
                side_range[1] = side_range[1].max(side);
                assert_eq!(action.magazine_grip, 0.0);
                assert_eq!(action.control_grip, 0.0);
                assert_eq!(
                    descendants(world, f.root),
                    pieces,
                    "inspect must not replace the assembly"
                );
                let state = world.resource::<ClientSession>().players[0].mechanics;
                assert_eq!(state.magazine, before.magazine);
                assert_eq!(state.chambered, before.chambered);
                let hand = named(&f.app, f.root, "configured-trigger-palm");
                let position = world.get::<GlobalTransform>(hand).unwrap().translation();
                if let Some(previous) = previous {
                    assert!(
                        position.distance(previous) < 2.0 / fps as f32 + 0.001,
                        "inspect/return snaps at fraction={fraction}: {}",
                        position.distance(previous)
                    );
                }
                previous = Some(position);
                if fraction < 1.0 {
                    f.assert_pistol_frame(aspect);
                }
            }
            assert!(
                sides.into_iter().all(|side| side),
                "both receiver sides must be presented: {side_range:?}"
            );
            // Framing at rest also follows breathing. Compare against a
            // non-inspecting rig at the same simulation time.
            let rest_frame = idle.app.world().resource::<Feedback>().responses[&7].frame_offset;
            assert!(
                f.app.world().resource::<Feedback>().responses[&7]
                    .frame_offset
                    .distance(rest_frame)
                    < 0.001
            );
        }
    }
}

#[test]
fn configured_local_inspect_interruptions_keep_continuous_contacts_and_framing() {
    for fps in [30, 60, 165] {
        for fraction in [0.28, 0.70] {
            for next in ["idle", "ads", "reload", "unequip", "sprint"] {
                let mut f = Fixture::new(WeaponKind::Rifle, 4.0 / 3.0);
                f.app
                    .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                        std::time::Duration::from_secs_f64(1.0 / fps as f64),
                    ));
                {
                    let mut session = f.app.world_mut().resource_mut::<ClientSession>();
                    session.players[0].mechanics.kind = Some(ManipulationKind::Inspect);
                    session.players[0].mechanics.fraction = fraction;
                }
                for _ in 0..fps {
                    f.step();
                }
                let hand = named(&f.app, f.root, "configured-trigger-palm");
                let mut previous = f
                    .app
                    .world()
                    .get::<GlobalTransform>(hand)
                    .unwrap()
                    .translation();
                {
                    let mut session = f.app.world_mut().resource_mut::<ClientSession>();
                    session.players[0].mechanics.kind = match next {
                        "reload" => Some(ManipulationKind::TacticalReload),
                        "unequip" => Some(ManipulationKind::Unequip),
                        _ => None,
                    };
                    session.players[0].mechanics.fraction = 0.0;
                    if next == "sprint" {
                        session.players[0].pose.velocity = [0.0, 0.0, -8.0];
                        session.players[0].pose.acceleration = [0.0, -12.0];
                    }
                }
                f.app.world_mut().resource_mut::<LocalInput>().aim = next == "ads";
                for frame in 0..fps {
                    if next == "unequip" {
                        f.app.world_mut().resource_mut::<ClientSession>().players[0]
                            .mechanics
                            .fraction = (frame as f32 / fps as f32 / 0.2).min(1.0);
                    }
                    f.step();
                    f.assert_contacts();
                    let current = f
                        .app
                        .world()
                        .get::<GlobalTransform>(hand)
                        .unwrap()
                        .translation();
                    assert!(
                        current.distance(previous) <= 3.0 / fps as f32 + 0.001,
                        "{next} fps={fps} frame={frame} displacement={}",
                        current.distance(previous)
                    );
                    previous = current;
                }
            }
        }
    }
}

#[test]
fn configured_local_inspect_restarts_continuously_after_sparse_confirmation() {
    for fps in [30, 60, 165] {
        let mut f = Fixture::new(WeaponKind::Rifle, 4.0 / 3.0);
        f.use_m4a1_kit(false, true, 134, 9);
        f.app
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                std::time::Duration::from_secs_f64(1.0 / f64::from(fps)),
            ));
        for _ in 0..fps {
            f.step();
        }
        let trigger = named(&f.app, f.root, "configured-trigger-palm");
        let support = named(&f.app, f.root, "configured-support-palm");
        let receiver = named(&f.app, f.root, "receiver");
        let before = f.app.world().resource::<ClientSession>().players[0].mechanics;
        let identities = descendants(f.app.world(), f.root);
        let mut last = [trigger, support, receiver]
            .map(|entity| *f.app.world().get::<GlobalTransform>(entity).unwrap());
        let mut next_packet = 0_u32;
        let mut skipped_at_cancellation = false;
        let mut saw_partial_ads = false;
        let mut saw_restarted_inspect = false;
        for frame in 0..=5 * fps {
            let t = f64::from(frame) / f64::from(fps);
            // At low FPS several updates can arrive in one render frame. The
            // presentation producer sees only the latest confirmed snapshot.
            while f64::from(next_packet) / 60.0 <= t {
                let packet = next_packet;
                next_packet += 1;
                let (action_id, kind, fraction) = match packet {
                    0..=119 => (1, Some(ManipulationKind::Inspect), packet as f32 / 168.0),
                    120..=140 => (1, None, 0.0),
                    141..=200 => (
                        2,
                        Some(ManipulationKind::Inspect),
                        (packet - 141) as f32 / 168.0,
                    ),
                    _ => (2, None, 0.0),
                };
                if packet % 3 == 0 || (120..126).contains(&packet) {
                    skipped_at_cancellation |= packet == 120;
                    continue;
                }
                let mut session = f.app.world_mut().resource_mut::<ClientSession>();
                session.last_sequence = u64::from(packet);
                session.players[0].mechanics = common::catalog::MechanicalSnapshot {
                    action_id,
                    kind,
                    fraction,
                    ..before
                };
            }
            f.app.world_mut().resource_mut::<LocalInput>().aim =
                (2.0..2.35).contains(&t) || t >= 3.35;
            f.step();
            f.assert_contacts();
            let world = f.app.world();
            let current = [trigger, support, receiver]
                .map(|entity| *world.get::<GlobalTransform>(entity).unwrap());
            for (a, b) in current.iter().zip(last) {
                assert!(
                    a.translation().distance(b.translation()) <= 3.0 / fps as f32 + 0.001,
                    "inspect interruption at {t:.3}s and {fps}Hz jumped by {}",
                    a.translation().distance(b.translation())
                );
            }
            assert!(
                current[2].rotation().angle_between(last[2].rotation()) < 10.0 / fps as f32 + 0.002
            );
            last = current;
            assert_eq!(descendants(world, f.root), identities);
            let mechanics = world.resource::<ClientSession>().players[0].mechanics;
            assert_eq!(mechanics.magazine, before.magazine);
            assert_eq!(mechanics.chambered, before.chambered);
            let response = &world.resource::<Feedback>().responses[&7];
            saw_partial_ads |= (2.2..2.35).contains(&t) && (0.02..0.7).contains(&response.ads);
            saw_restarted_inspect |= (2.85..3.30).contains(&t)
                && response.displayed_mechanics.action_id == 2
                && response
                    .action
                    .unwrap()
                    .root
                    .rotation
                    .angle_between(Quat::IDENTITY)
                    > 0.2;
            if t > 4.5 {
                assert!(response.ads > 0.98, "ADS did not resume after cancellation");
            }
        }
        assert!(skipped_at_cancellation && saw_partial_ads && saw_restarted_inspect);
    }
}

#[test]
fn configured_local_mixed_reload_uses_owned_incoming_mesh_before_insertion_without_duplicates() {
    use common::catalog::{DefinitionId, EquippedWeapon, WeaponInstanceId, WeaponKit};
    use common::replication::ConfigurationPublisher;
    for fps in [30, 60, 165] {
        for empty in [false, true] {
            let mut f = Fixture::new(WeaponKind::Rifle, 1.6);
            f.app
                .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                    std::time::Duration::from_secs_f64(1.0 / fps as f64),
                ));
            let mut kit = WeaponKit::from_configuration(
                catalog(),
                catalog().configuration(common::WeaponId::M4A1).unwrap(),
            )
            .unwrap();
            let (outgoing_definition, incoming_definition) =
                if empty { (101, 120) } else { (120, 101) };
            kit.weapon
                .components
                .iter_mut()
                .find(|p| p.node == 1)
                .unwrap()
                .definition = DefinitionId(outgoing_definition);
            kit.reserves = vec![DefinitionId(incoming_definition)];
            let mut weapon =
                EquippedWeapon::from_kit(catalog(), WeaponInstanceId(77), &kit).unwrap();
            for _ in 0..if empty { weapon.rounds() } else { 4 } {
                assert!(weapon.fire());
            }
            let total = weapon.mechanics().total_rounds();
            let old = weapon.mechanics().magazines()[0].id;
            let incoming = weapon.mechanics().magazines()[1].id;
            let fingerprint = catalog_fingerprint(common::catalog::CATALOG_SOURCE);
            let mut publisher = ConfigurationPublisher::new(2, fingerprint).unwrap();
            f.app
                .world_mut()
                .resource_mut::<ClientSession>()
                .configurations
                .cache
                .begin_epoch(2)
                .unwrap();
            assert!(weapon.mechanics_mut().start_reload(2.4));
            let mut saw_incoming_unseated = false;
            let mut previously_visible = true;
            for frame in 0..(fps * 4) {
                // A slower shared mechanical clock models injury. Lose every
                // third update; the next snapshot reconstructs the actual object.
                weapon.advance(0.7 / fps as f32);
                if frame % 3 != 2 {
                    let handle = publisher
                        .register(7, EquipmentSlot::Primary, weapon.instance())
                        .unwrap();
                    let packet = publisher.entries()[(handle - 1) as usize].clone();
                    let mut session = f.app.world_mut().resource_mut::<ClientSession>();
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
                }
                f.step();
                f.assert_contacts();
                f.assert_support_clears_magwell();
                assert_eq!(weapon.mechanics().total_rounds(), total);
                let world = f.app.world();
                let player = &world.resource::<ClientSession>().players[0];
                let fraction = player.mechanics.fraction;
                let action = world.resource::<Feedback>().responses[&7].action.unwrap();
                if !action.magazine_visible && previously_visible {
                    f.assert_magazine_below_viewport();
                }
                previously_visible = action.magazine_visible;
                let wanted = if player.mechanics.reloading() && fraction < 0.41 {
                    old
                } else {
                    incoming
                };
                let request = world.get::<ConfiguredWeapon>(f.root).unwrap();
                assert_eq!(request.magazine, Some(wanted));
                let magazines: Vec<_> = descendants(world, f.root)
                    .into_iter()
                    .filter_map(|entity| {
                        world
                            .get::<ConfiguredPiece>(entity)
                            .filter(|p| [101, 120].contains(&p.definition))
                    })
                    .collect();
                assert_eq!(magazines.len(), 1, "never render installed and hand copies");
                assert_eq!(
                    magazines[0].definition,
                    if wanted == old {
                        outgoing_definition
                    } else {
                        incoming_definition
                    }
                );
                if player.mechanics.reloading() && (0.45..0.69).contains(&fraction) {
                    assert!(!player.mechanics.magazine_inserted);
                    assert_eq!(
                        magazines[0].definition, incoming_definition,
                        "incoming mesh must not wait for the installed revision"
                    );
                    saw_incoming_unseated = true;
                }
            }
            assert!(saw_incoming_unseated);
        }
    }
}

#[test]
fn configured_local_reload_skipped_commit_and_resume_keep_the_magazine_seated() {
    use common::catalog::{DefinitionId, EquippedWeapon, ManipulationPhase, WeaponKit};
    use common::replication::ConfigurationPublisher;
    for fps in [30, 60, 165] {
        let mut f = Fixture::new(WeaponKind::Rifle, 1.6);
        f.app
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                std::time::Duration::from_secs_f64(1.0 / f64::from(fps)),
            ));
        let mut kit = WeaponKit::from_configuration(
            catalog(),
            catalog().configuration(common::WeaponId::M4A1).unwrap(),
        )
        .unwrap();
        kit.reserves = vec![DefinitionId(120)];
        let mut weapon = EquippedWeapon::from_kit(catalog(), WeaponInstanceId(77), &kit).unwrap();
        for _ in 0..weapon.rounds() {
            assert!(weapon.fire());
        }
        let total = weapon.mechanics().total_rounds();
        let incoming = weapon.mechanics().magazines()[1].id;
        let fingerprint = catalog_fingerprint(common::catalog::CATALOG_SOURCE);
        let mut publisher = ConfigurationPublisher::new(2, fingerprint).unwrap();
        f.app
            .world_mut()
            .resource_mut::<ClientSession>()
            .configurations
            .cache
            .begin_epoch(2)
            .unwrap();
        let mut deliver = |f: &mut Fixture, weapon: &EquippedWeapon| {
            let handle = publisher
                .register(7, EquipmentSlot::Primary, weapon.instance())
                .unwrap();
            let packet = publisher.entries()[(handle - 1) as usize].clone();
            let mut session = f.app.world_mut().resource_mut::<ClientSession>();
            session
                .configurations
                .cache
                .apply(catalog(), fingerprint, packet)
                .unwrap();
            let player = &mut session.players[0];
            player.weapon_instance = weapon.instance().id;
            player.configuration = handle;
            player.mechanics = weapon.mechanics().snapshot();
            f.step();
            f.assert_contacts();
        };
        deliver(&mut f, &weapon);
        assert!(weapon.mechanics_mut().start_reload(2.4));
        while weapon.mechanics().snapshot().fraction < 0.53 {
            weapon.advance(0.7 / fps as f32);
            deliver(&mut f, &weapon);
        }
        let before = weapon.mechanics().snapshot();
        assert_eq!(before.phase, Some(ManipulationPhase::Insert));
        assert!(!before.magazine_inserted);
        let part = named(&f.app, f.root, "magazine");
        let unseated = *f.app.world().get::<Transform>(part).unwrap();
        // Lose every update around the commit. The next delivered state is
        // already canceled, so presentation never sees Feed for this action.
        while weapon.mechanics().snapshot().phase != Some(ManipulationPhase::Feed) {
            weapon.advance(0.005);
        }
        weapon.mechanics_mut().cancel();
        let assert_seated = |f: &Fixture| {
            let world = f.app.world();
            let part = named(&f.app, f.root, "magazine");
            let pose = *world.get::<Transform>(part).unwrap();
            assert!(world.get::<InheritedVisibility>(part).unwrap().get());
            assert_eq!(
                world.get::<ConfiguredWeapon>(f.root).unwrap().magazine,
                Some(incoming)
            );
            let magazines: Vec<_> = descendants(world, f.root)
                .into_iter()
                .filter_map(|entity| world.get::<ConfiguredPiece>(entity))
                .filter(|piece| [101, 120].contains(&piece.definition))
                .collect();
            assert_eq!(magazines.len(), 1);
            assert_eq!(magazines[0].definition, 120);
            pose
        };
        deliver(&mut f, &weapon);
        let first = assert_seated(&f);
        for _ in 0..fps {
            f.step();
        }
        let settled = assert_seated(&f);
        assert!(unseated.translation.distance(settled.translation) > 0.03);
        assert!(
            first.translation.distance(settled.translation) < 0.001,
            "{fps} Hz first={first:?}, seated={settled:?}"
        );

        assert!(!weapon.mechanics().snapshot().chambered);
        assert!(weapon.mechanics_mut().start_reload(2.4));
        assert_eq!(
            weapon.mechanics().snapshot().phase,
            Some(ManipulationPhase::Feed)
        );
        while weapon.mechanics().snapshot().reloading() {
            deliver(&mut f, &weapon);
            let resumed = assert_seated(&f);
            assert!(resumed.translation.distance(settled.translation) < 0.001);
            weapon.advance(0.7 / fps as f32);
        }
        deliver(&mut f, &weapon);
        assert!(weapon.mechanics().snapshot().chambered);
        assert_eq!(weapon.mechanics().total_rounds(), total);
    }
}

#[test]
fn configured_local_switch_during_reload_keeps_outgoing_magazine_location() {
    use common::catalog::{DefinitionId, EquippedWeapon, WeaponInstanceId, WeaponKit};
    use common::replication::ConfigurationPublisher;
    for fraction in [0.15, 0.30, 0.55, 0.75, 0.95] {
        let mut f = Fixture::new(WeaponKind::Rifle, 1.6);
        let mut kit = WeaponKit::from_configuration(
            catalog(),
            catalog().configuration(common::WeaponId::M4A1).unwrap(),
        )
        .unwrap();
        kit.reserves = vec![DefinitionId(120)];
        let mut rifle = EquippedWeapon::from_kit(catalog(), WeaponInstanceId(77), &kit).unwrap();
        for _ in 0..30 {
            assert!(rifle.fire());
        }
        assert!(rifle.mechanics_mut().start_reload(2.4));
        rifle.advance(2.4 * fraction);
        let pistol = EquippedWeapon::new(
            catalog(),
            WeaponInstanceId(78),
            catalog().configuration(common::WeaponId::M1911).unwrap(),
        )
        .unwrap();
        let fingerprint = catalog_fingerprint(common::catalog::CATALOG_SOURCE);
        let mut publisher = ConfigurationPublisher::new(2, fingerprint).unwrap();
        let rifle_handle = publisher
            .register(7, EquipmentSlot::Primary, rifle.instance())
            .unwrap();
        let pistol_handle = publisher
            .register(7, EquipmentSlot::Sidearm, pistol.instance())
            .unwrap();
        {
            let mut session = f.app.world_mut().resource_mut::<ClientSession>();
            session.configurations.cache.begin_epoch(2).unwrap();
            for packet in publisher.entries() {
                session
                    .configurations
                    .cache
                    .apply(catalog(), fingerprint, packet.clone())
                    .unwrap();
            }
            let player = &mut session.players[0];
            player.weapon_instance = rifle.instance().id;
            player.configuration = rifle_handle;
            player.mechanics = rifle.mechanics().snapshot();
        }
        for _ in 0..30 {
            f.step();
        }
        let inserted = rifle.mechanics().inserted().map(|m| m.id);
        rifle.mechanics_mut().cancel();
        assert!(
            rifle
                .mechanics_mut()
                .start_unequip(common::catalog::HandlingTiming::UNEQUIP)
        );
        f.app.world_mut().resource_mut::<ClientSession>().players[0].mechanics =
            rifle.mechanics().snapshot();
        f.step();
        f.assert_support_clears_magwell();
        let world = f.app.world();
        assert_eq!(
            world.resource::<Feedback>().responses[&7].displayed,
            Some(WeaponKind::Rifle)
        );
        assert_eq!(
            world.get::<ConfiguredWeapon>(f.root).unwrap().magazine,
            inserted,
            "lowering rifle must not borrow the pistol magazine or keep the uncommitted incoming"
        );
        let part = named(&f.app, f.root, "magazine");
        assert_eq!(
            world.get::<Visibility>(part),
            Some(&if inserted.is_some() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            }),
            "switch at {fraction} revealed a detached magazine"
        );
        // The outgoing slot remains selected until the actual unequip commit,
        // including a slowed clock. The incoming snapshot changes the display
        // immediately at that marker, with no independent triangular timer.
        for _ in 0..20 {
            rifle.advance(0.005);
            f.app.world_mut().resource_mut::<ClientSession>().players[0].mechanics =
                rifle.mechanics().snapshot();
            f.step();
            assert_eq!(
                f.app.world().resource::<Feedback>().responses[&7].displayed,
                Some(WeaponKind::Rifle)
            );
            assert_eq!(
                f.app
                    .world()
                    .get::<ConfiguredWeapon>(f.root)
                    .unwrap()
                    .magazine,
                inserted
            );
        }
        {
            let mut session = f.app.world_mut().resource_mut::<ClientSession>();
            let player = &mut session.players[0];
            player.weapon = WeaponKind::Pistol;
            player.weapon_id = common::WeaponId::M1911;
            player.weapon_slot = EquipmentSlot::Sidearm;
            player.weapon_instance = pistol.instance().id;
            player.configuration = pistol_handle;
            player.mechanics = pistol.mechanics().snapshot();
            player.mechanics.kind = Some(common::catalog::ManipulationKind::Equip);
            player.mechanics.fraction = 0.0;
            player.pose.weapon = WeaponKind::Pistol;
        }
        f.step();
        assert_eq!(
            f.app.world().resource::<Feedback>().responses[&7].displayed,
            Some(WeaponKind::Pistol)
        );
    }
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
                    magazines: (1..=4)
                        .map(|id| common::replication::MagazineDefinition {
                            id: common::catalog::MagazineInstanceId(id),
                            definition: catalog()
                                .configuration(weapon.definition())
                                .unwrap()
                                .components[0]
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
        let to_eye = world
            .get::<GlobalTransform>(self.camera)
            .unwrap()
            .to_matrix()
            .inverse();
        let Projection::Perspective(lens) = world.get::<Projection>(self.camera).unwrap() else {
            panic!("perspective");
        };
        for (i, (side, arm)) in [
            ("support", solution.solution.support),
            ("trigger", solution.solution.trigger),
        ]
        .into_iter()
        .enumerate()
        {
            let shoulder = global.transform_point(solution.shoulders[i]);
            let elbow = global.transform_point(arm.elbow);
            let wrist = global.transform_point(arm.wrist.translation);
            let palm = named(&self.app, self.root, &format!("configured-{side}-palm"));
            assert!(
                world
                    .get::<GlobalTransform>(palm)
                    .unwrap()
                    .translation()
                    .distance(wrist)
                    < 3e-6
            );
            for (name, a, b, half) in [
                ("upper", elbow, shoulder, Vec3::new(0.035, 0.15, 0.0325)),
                ("forearm", wrist, elbow, Vec3::new(0.0325, 0.14, 0.030)),
            ] {
                assert!(((b - a).length() - 2.0 * half.y).abs() < 3e-6);
                let sleeve = named(&self.app, self.root, &format!("configured-{side}-{name}"));
                if *world.get::<Visibility>(sleeve).unwrap() == Visibility::Hidden {
                    continue;
                }
                let pose = world.get::<GlobalTransform>(sleeve).unwrap();
                let axis = (b - a).normalize();
                for y in [-1.0, 1.0] {
                    let offset = pose.transform_point(Vec3::Y * y * half.y) - a;
                    let distance = offset.dot(axis);
                    assert!((offset - axis * distance).length() < 3e-6);
                    assert!((-3e-6..=2.0 * half.y + 3e-6).contains(&distance));
                    for x in [-1.0, 1.0] {
                        for z in [-1.0, 1.0] {
                            let corner = pose.transform_point(half * Vec3::new(x, y, z));
                            assert!(
                                to_eye.transform_point3(corner).z
                                    <= -(lens.near + 0.01).max(0.16) + 3e-6
                            );
                        }
                    }
                }
            }
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
                    + root.rotation()
                        * if player.weapon == WeaponKind::Rifle {
                            common::weapon_animation::m4a1::support_clearance(&action)
                        } else {
                            Vec3::ZERO
                        }
            ) < 3e-6
        );
        assert!(trigger.translation().distance(rest_trigger) < 3e-6);
    }

    fn assert_support_clears_magwell(&self) {
        let world = self.app.world();
        let lower = named(&self.app, self.root, "lower");
        let palm = named(&self.app, self.root, "configured-support-palm");
        crate::render::weapons::grasp::assert_magwell_clear(
            world,
            palm,
            lower,
            world.resource::<ClientSession>().players[0]
                .mechanics
                .fraction,
        );
    }

    fn assert_magazine_below_viewport(&self) {
        use bevy::mesh::VertexAttributeValues;
        let world = self.app.world();
        let part = named(&self.app, self.root, "magazine");
        let to_eye = world
            .get::<GlobalTransform>(self.camera)
            .unwrap()
            .to_matrix()
            .inverse();
        let meshes = world.resource::<Assets<Mesh>>();
        let mut count = 0;
        for entity in std::iter::once(part).chain(descendants(world, part)) {
            let Some(mesh) = world.get::<Mesh3d>(entity) else {
                continue;
            };
            let VertexAttributeValues::Float32x3(vertices) = meshes
                .get(&mesh.0)
                .unwrap()
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
            else {
                panic!("positions");
            };
            let transform = to_eye * world.get::<GlobalTransform>(entity).unwrap().to_matrix();
            for vertex in vertices {
                let point = transform.transform_point3(Vec3::from(*vertex));
                let lower_edge = point.z * (65.0_f32.to_radians() * 0.5).tan();
                assert!(
                    point.y < lower_edge,
                    "retention must leave the viewport before hiding: {point:?}, edge={lower_edge}"
                );
                count += 1;
            }
        }
        assert!(count > 0);
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
fn armed_sprint_replays_two_kits_through_snapshot_gaps_and_ads() {
    use common::{catalog::AnchorKind, humanoid::stride_phase_rate};
    for fps in [30, 165] {
        for suppressed in [false, true] {
            let mut f = Fixture::new(WeaponKind::Rifle, 1.6);
            f.use_m4a1_kit(
                !suppressed,
                suppressed,
                if suppressed { 134 } else { 0 },
                if suppressed { 9 } else { 0 },
            );
            f.app
                .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                    std::time::Duration::from_secs_f64(1.0 / f64::from(fps)),
                ));
            f.app.world_mut().resource_mut::<LocalInput>().aim = true;
            for _ in 0..fps {
                f.step();
            }
            assert!(f.app.world().resource::<Feedback>().responses[&7].ads > 0.98);
            let hand = named(&f.app, f.root, "configured-trigger-palm");
            let mut previous = f
                .app
                .world()
                .get::<GlobalTransform>(hand)
                .unwrap()
                .translation();
            let initial_eye = *f.app.world().get::<Transform>(f.camera).unwrap();
            let initial_world_camera = *f.app.world().get::<Transform>(f.world_camera).unwrap();
            {
                let session = f.app.world().resource::<ClientSession>();
                let spec = &session.configurations.cache.get(1, 2).unwrap().resolved;
                let carry = common::humanoid::armed_stride::sample(
                    &common::humanoid::PoseState {
                        velocity: [0.0, 0.0, -PLAYER_RUN_SPEED],
                        phase: std::f32::consts::FRAC_PI_4, // neutral vertical stride term
                        ..default()
                    },
                    spec.assembly.anchors[&AnchorKind::SupportGrip],
                    reload_action(WeaponKind::Rifle, 0.0, false),
                );
                let (height, pitch) = if suppressed {
                    (-0.027, 0.11)
                } else {
                    (-0.040, 0.16)
                };
                assert!(
                    (carry.translation.y - height).abs() < 0.001,
                    "RVG={suppressed}, socket={:?}, weight={}, carry_y={}",
                    spec.assembly.anchors[&AnchorKind::SupportGrip].translation,
                    carry.weight,
                    carry.translation.y,
                );
                assert!((carry.angles.x - pitch).abs() < 0.004);
            }
            let mut phase = 5.8;
            let mut next_tick = 0;
            let mut z = 0.0;
            let mut previous_world: Option<Transform> = None;
            for frame in 0..=3 * fps {
                let time = f.app.world().resource::<Time<Real>>().elapsed_secs_f64();
                let route_time = f64::from(frame) / f64::from(fps);
                let mut session = f.app.world_mut().resource_mut::<ClientSession>();
                while f64::from(next_tick) / 60.0 <= route_time {
                    let t = next_tick as f32 / 60.0;
                    let speed = PLAYER_WALK_SPEED * (t / 0.35).min(1.0)
                        + (PLAYER_RUN_SPEED - PLAYER_WALK_SPEED)
                            * ((t - 0.45) / 0.45).clamp(0.0, 1.0)
                        - PLAYER_RUN_SPEED * ((t - 1.50) / 0.36).clamp(0.0, 1.0);
                    let turn =
                        (std::f32::consts::PI * ((t - 0.95) / 0.45).clamp(0.0, 1.0)).sin() * 4.0;
                    let push = if t < 1.45 { -5.0 } else { 7.0 };
                    phase = (phase + stride_phase_rate(speed, 0.0) / 60.0) % std::f32::consts::TAU;
                    z -= speed / 60.0;
                    let stamp = time - (route_time - f64::from(t));
                    if next_tick == 0 || next_tick % 3 != 0 {
                        let motion = crate::sync::LocalMotionPose {
                            position: Vec3::Z * z,
                            stride_phase: phase,
                            stride_speed: speed,
                            velocity: Vec3::NEG_Z * speed,
                            acceleration: Vec2::new(turn, push),
                            turn_rate: turn,
                            ..default()
                        };
                        let pose = &mut session.players[0].pose;
                        pose.phase = phase;
                        pose.velocity = motion.velocity.into();
                        pose.acceleration = motion.acceleration.into();
                        pose.turn_rate = turn;
                        pose.grounded = true;
                        session.local_motion.record(t, motion, stamp, false);
                    }
                    next_tick += 1;
                }
                f.app.world_mut().resource_mut::<LocalInput>().run =
                    (0.45..1.50).contains(&route_time);
                f.step();
                f.assert_contacts();
                let world = f.app.world();
                let current = world.get::<GlobalTransform>(hand).unwrap().translation();
                assert!(
                    current.distance(previous) < 2.5 / fps as f32 + 0.001,
                    "{fps} Hz RVG={suppressed} frame={frame}: palm jump {}",
                    current.distance(previous)
                );
                previous = current;
                assert_eq!(*world.get::<Transform>(f.camera).unwrap(), initial_eye);
                assert_eq!(
                    *world.get::<Transform>(f.world_camera).unwrap(),
                    initial_world_camera
                );
                let ads = world.resource::<Feedback>().responses[&7].ads;
                if (1.1..1.5).contains(&route_time) {
                    assert!(ads < 0.01, "sprint blocks held ADS: {ads}");
                    let forward = world.get::<Transform>(f.view).unwrap().rotation * Vec3::NEG_Z;
                    assert!(forward.y > 0.07, "POV should show the sprint carry pitch");
                }
                if route_time > 2.7 {
                    assert!(ads > 0.98, "ADS must resume after braking");
                }
                let session = world.resource::<ClientSession>();
                let player = &session.players[0];
                let spec = &session.configurations.cache.get(1, 2).unwrap().resolved;
                let motion = session.local_motion.sample(time).unwrap();
                let pose = common::humanoid::PoseState {
                    ads,
                    phase: motion.stride_phase,
                    velocity: motion.velocity.into(),
                    acceleration: motion.acceleration.into(),
                    turn_rate: motion.turn_rate,
                    ..player.pose.clone()
                };
                let rig = common::humanoid::configured::evaluate_with_carry(
                    &pose,
                    [1.0; 2],
                    spec,
                    &player.mechanics,
                    None,
                    Some(0.058),
                )
                .unwrap();
                let root = rig.weapon.unwrap().root;
                if (1.1..1.5).contains(&route_time) {
                    assert!((root.rotation * Vec3::NEG_Z).y > 0.07);
                }
                for (segment, socket) in [
                    (common::humanoid::Segment::LeftHand, AnchorKind::SupportGrip),
                    (
                        common::humanoid::Segment::RightHand,
                        AnchorKind::TriggerGrip,
                    ),
                ] {
                    let palm = rig.segments[segment.index()].transform();
                    let contact = root.mul_transform(spec.assembly.anchors[&socket]);
                    assert!(palm.translation.distance(contact.translation) < 2e-6);
                    assert!(palm.rotation.angle_between(contact.rotation) < 0.002);
                }
                if let Some(previous) = previous_world {
                    assert!(
                        root.translation.distance(previous.translation) < 2.5 / fps as f32 + 0.001
                            && root.rotation.angle_between(previous.rotation)
                                < 6.0 / fps as f32 + 0.002,
                        "{fps} Hz RVG={suppressed} frame={frame}: world rig jump"
                    );
                }
                previous_world = Some(root);
            }
        }
    }
}

impl Fixture {
    fn use_m4a1_kit(&mut self, short: bool, suppressed: bool, grip: u32, position: u16) {
        use common::catalog::{Attachment, DefinitionId};
        let mut packet = self
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
        {
            let mut session = self.app.world_mut().resource_mut::<ClientSession>();
            session
                .configurations
                .cache
                .apply(catalog(), packet.catalog, packet)
                .unwrap();
            session.players[0].configuration = 2;
        }
    }
}

#[test]
fn configured_local_m4a1_kit_extremes_keep_rendered_contacts_through_ads_and_reload() {
    for (short, suppressed, grip, position) in [
        (true, false, 0, 0),
        (false, true, 134, 0),
        (false, true, 134, 9),
        (false, false, 135, 0),
        (false, false, 135, 2),
    ] {
        let mut f = Fixture::new(WeaponKind::Rifle, 1.6);
        f.use_m4a1_kit(short, suppressed, grip, position);
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
            if aim {
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
                    assert!(eye.z < -0.1, "{name} near clipping: {eye:?}, short={short}");
                    if !suppressed {
                        assert!(
                            eye.x.abs() < 0.00025 && eye.y.abs() < 0.00025,
                            "{name}: {eye:?}"
                        );
                    }
                }
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
        // Inspect all actual kit bounds, including the long suppressor, at the
        // four acceptance aspect ratios without changing the configured FOV.
        for aspect in [4.0 / 3.0, 1.6, 16.0 / 9.0, 21.0 / 9.0] {
            let mut projection = f.app.world_mut().get_mut::<Projection>(f.camera).unwrap();
            let Projection::Perspective(projection) = &mut *projection else {
                panic!("perspective");
            };
            projection.aspect_ratio = aspect;
            for step in 0..=168 {
                {
                    let mut session = f.app.world_mut().resource_mut::<ClientSession>();
                    session.players[0].mechanics.kind = Some(ManipulationKind::Inspect);
                    session.players[0].mechanics.fraction = step as f32 / 168.0;
                }
                f.step();
                f.assert_contacts();
                f.assert_pistol_frame(aspect);
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
