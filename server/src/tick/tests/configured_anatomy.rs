use super::*;
use common::{
    catalog::{CATALOG_SOURCE, Catalog},
    humanoid::*,
};

#[test]
fn configured_stock_authority_braces_against_equipped_cover_and_preserves_damage_rig() {
    for armor in [ArmorKind::None, ArmorKind::Light, ArmorKind::Heavy] {
        let (mut app, _, target) = physical_fixture();
        *app.world_mut().get_mut::<ArmorState>(target).unwrap() =
            ArmorState::from_loadout(Loadout { armor, ..default() });
        {
            let mut input = app.world_mut().get_mut::<ClientInput>(target).unwrap();
            input.aim = true;
            input.crouch = true;
            input.camera.pitch = -0.6;
        }
        for _ in 0..150 {
            held_steps(&mut app, target, 1);
            assert!(
                app.world()
                    .get::<ragdoll::Motion>(target)
                    .unwrap()
                    .handling_error
                    .is_none()
            );
        }
        let motion = app.world().get::<ragdoll::Motion>(target).unwrap();
        let grip =
            WeaponKind::Rifle.spec().assembly.anchors[&common::catalog::AnchorKind::TriggerGrip];
        let stock =
            WeaponKind::Rifle.spec().assembly.anchors[&common::catalog::AnchorKind::StockContact];
        let hand = motion.world[Segment::RightHand.index()];
        let weapon = hand.mul_transform(Transform::from_matrix(grip.to_matrix().inverse()));
        let depth = match armor {
            ArmorKind::None => 0.145,
            ArmorKind::Light => 0.1975,
            ArmorKind::Heavy => 0.203,
        };
        let pocket =
            motion.world[Segment::Thorax.index()].transform_point(Vec3::new(0.14, 0.1, -depth));
        assert!(
            weapon.transform_point(stock.translation).distance(pocket) < 0.001,
            "authoritative stock must meet the equipped surface"
        );
        assert!(app.world().get::<PoseState>(target).unwrap().ads > 0.999);
    }
}

fn change_grip(app: &mut App, player: Entity, x: f32, node: u16) {
    // Controlled server registry, as in assembly_authority_*: no mesh or client
    // contacts participate. Change a real configuration node to resolve revision.
    let mut document: common::catalog::CatalogDocument = ron::from_str(CATALOG_SOURCE).unwrap();
    document.weapons[0]
        .assembly
        .geometry
        .anchors
        .iter_mut()
        .find(|a| a.kind == common::catalog::AnchorKind::SupportGrip)
        .unwrap()
        .pose
        .translation = [x, 0.02, -0.20];
    let catalog = Catalog::validate("configured-anatomy-fixture", document).unwrap();
    let mut arsenal = app.world_mut().get_mut::<Arsenal>(player).unwrap();
    let mut config = arsenal.current().instance().configuration.clone();
    config.components[0].node = node;
    let revision = arsenal.current().instance().revision;
    assert_eq!(
        arsenal
            .reconfigure(&catalog, EquipmentSlot::Primary, revision, config)
            .unwrap(),
        revision + 1
    );
}

#[test]
fn configured_anatomy_authority_hits_revised_hand_with_a_normal_projectile() {
    let (mut app, shooter, target) = physical_fixture();
    change_grip(&mut app, target, -0.46, 9);
    held_steps(&mut app, target, 2);
    let motion = app.world().get::<ragdoll::Motion>(target).unwrap();
    assert!(motion.valid && motion.handling_error.is_none());
    let hand = motion.world[Segment::LeftHand.index()].translation;
    let root = app.world().get::<Transform>(target).unwrap();
    let old = evaluate_pose(app.world().get::<PoseState>(target).unwrap(), 1.0, 1.0);
    assert!(
        hand.distance(root.transform_point(old[Segment::LeftHand.index()].position.into())) > 0.3
    );
    normal_shot(&mut app, shooter, hand);
    let health = app.world().get::<BodyHealth>(target).unwrap();
    assert_eq!(health.anatomy.last_region, Some(Region::LeftArm));
    assert!(health.anatomy.capacity(Region::LeftArm) < 1.0);
}

#[test]
fn configured_anatomy_authority_preserves_rejected_pose_and_hands_it_to_real_ragdoll() {
    let (mut app, _, target) = physical_fixture();
    change_grip(&mut app, target, -0.46, 9);
    app.world_mut()
        .get_mut::<ClientInput>(target)
        .unwrap()
        .crouch = true;
    held_steps(&mut app, target, 30);
    let captured = app.world().get::<ragdoll::Motion>(target).unwrap().world;
    let captured_root = app.world().get::<ragdoll::Motion>(target).unwrap().root;
    change_grip(&mut app, target, 4.0, 10);
    held_steps(&mut app, target, 1);
    let rejected = app.world().get::<ragdoll::Motion>(target).unwrap();
    assert!(rejected.handling_error.is_some());
    assert_eq!(
        rejected.world, captured,
        "unreachable revision must not silently clamp anatomy"
    );
    app.world_mut()
        .get_mut::<ClientInput>(target)
        .unwrap()
        .forward = true;
    held_steps(&mut app, target, 4);
    let motion = app.world().get::<ragdoll::Motion>(target).unwrap();
    assert!(motion.root.translation.distance(captured_root.translation) > 0.001);
    for (before, after) in captured.into_iter().zip(motion.world) {
        let local =
            captured_root.rotation.inverse() * (before.translation - captured_root.translation);
        assert!(
            after
                .translation
                .distance(motion.root.transform_point(local))
                < 3e-6,
            "rejection retains local anatomy, not stranded world-space tissue"
        );
    }
    app.world_mut()
        .get_mut::<ClientInput>(target)
        .unwrap()
        .forward = false;
    // Restore a reachable configuration before asking the existing controller
    // to enter a surviving physical fall.
    change_grip(&mut app, target, -0.46, 11);
    held_steps(&mut app, target, 2);
    let captured = app.world().get::<ragdoll::Motion>(target).unwrap().world;
    assert!(
        app.world()
            .get::<ragdoll::Motion>(target)
            .unwrap()
            .handling_error
            .is_none()
    );
    app.world_mut()
        .get_mut::<PoseState>(target)
        .unwrap()
        .impulse = [3.0, 0.0, 0.0];
    steps(&mut app, 1);
    assert!(app.world().get::<live_body::Body>(target).is_some());
    let mut count = 0;
    for (piece, transform) in app
        .world_mut()
        .query::<(&physical::PhysicsPiece, &Transform)>()
        .iter(app.world())
    {
        if piece.owner == 8 {
            assert!(
                transform
                    .translation
                    .distance(captured[piece.segment.index()].translation)
                    < 0.065,
                "configured handoff jumped: {:?}",
                piece.segment
            );
            count += 1;
        }
    }
    assert_eq!(count, SEGMENT_COUNT);
}
