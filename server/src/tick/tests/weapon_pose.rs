use super::*;

#[test]
fn configured_weapon_root_is_published_through_movement_and_cleared_when_unusable() {
    use common::catalog::AnchorKind;
    use common::humanoid::{PackedWeaponRoot, PoseState};

    let (mut app, actor, _) = physical_fixture();
    let mut previous = None;
    for tick in 0..100 {
        {
            let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
            input.forward = tick >= 20;
            input.run = tick >= 50;
            input.camera.yaw = (tick as f32 * 0.004).min(0.3);
        }
        held_steps(&mut app, actor, 1);
        let pose = app.world().get::<PoseState>(actor).unwrap();
        let root = pose.weapon_root.unpack().expect("configured weapon pose");
        let spec = app
            .world()
            .get::<Arsenal>(actor)
            .unwrap()
            .current()
            .presentation_spec();
        let exit = spec
            .assembly
            .anchors
            .get(&AnchorKind::DeviceExit)
            .or_else(|| spec.assembly.anchors.get(&AnchorKind::Bore))
            .unwrap();
        let outlet = root.mul_transform(*exit).translation;
        assert!(outlet.is_finite() && outlet.length() < 3.0);
        if let Some(previous) = previous {
            assert!(
                outlet.distance(previous) < 0.12,
                "weapon root jumped at tick {tick}"
            );
        }
        previous = Some(outlet);
        let decoded: PackedWeaponRoot = data::decode(&data::encode(&pose.weapon_root));
        assert!(
            decoded
                .unpack()
                .unwrap()
                .translation
                .distance(root.translation)
                < 0.0001
        );
    }
    app.world_mut()
        .get_mut::<Participant>(actor)
        .unwrap()
        .status = RaidStatus::Extracted;
    held_steps(&mut app, actor, 1);
    assert!(
        app.world()
            .get::<PoseState>(actor)
            .unwrap()
            .weapon_root
            .unpack()
            .is_none()
    );
}

#[test]
fn configured_weapon_root_supports_rifle_smg_and_sidearm() {
    use common::humanoid::PoseState;
    for primary in [WeaponId::M4A1, WeaponId::MP5] {
        let (mut app, actor, other) = physical_fixture();
        app.world_mut()
            .get_mut::<Transform>(other)
            .unwrap()
            .translation
            .x += 4.0;
        *app.world_mut().get_mut::<Arsenal>(actor).unwrap() = Arsenal::new(Loadout {
            primary,
            ..default()
        });
        for slot in [EquipmentSlot::Primary, EquipmentSlot::Sidearm] {
            app.world_mut()
                .get_mut::<ClientInput>(actor)
                .unwrap()
                .weapon = slot;
            for aiming in [false, true] {
                app.world_mut().get_mut::<ClientInput>(actor).unwrap().aim = aiming;
                held_steps(&mut app, actor, 200);
                let weapon = app.world().get::<Arsenal>(actor).unwrap();
                assert_eq!(weapon.active, slot);
                let pose = app
                    .world()
                    .get::<PoseState>(actor)
                    .unwrap()
                    .weapon_root
                    .unpack()
                    .unwrap_or_else(|| {
                        panic!("missing pose for {primary:?} slot={slot:?} ads={aiming}")
                    });
                let outlet = common::weapon_animation::configured_outlet(weapon.current().spec());
                assert!(pose.mul_transform(outlet).translation.is_finite());
            }
        }
    }
}

#[test]
fn shot_uses_resolved_outlet_and_bore_during_motion_and_ads() {
    use common::{catalog::AnchorKind, humanoid::PoseState};
    let (mut app, actor, other) = physical_fixture();
    app.world_mut()
        .get_mut::<Transform>(other)
        .unwrap()
        .translation
        .x += 4.0;
    let mut shots = [0_usize; 4];
    let mut last_id = app.world().resource::<WorldState>().next_projectile_id;
    for tick in 0..220 {
        let stage = tick / 55;
        {
            let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
            input.fire = true;
            input.forward = stage > 0;
            input.run = stage == 2;
            input.aim = stage == 3;
            input.camera.yaw = if stage == 2 { 0.25 } else { 0.0 };
        }
        held_steps(&mut app, actor, 1);
        let next = app.world().resource::<WorldState>().next_projectile_id;
        if next == last_id {
            continue;
        }
        assert_eq!(next, last_id + 1);
        last_id = next;
        shots[stage] += 1;
        let world = app.world();
        let actor_root = *world.get::<Transform>(actor).unwrap();
        let pose = world.get::<PoseState>(actor).unwrap();
        let root = actor_root.mul_transform(pose.weapon_root.unpack().unwrap());
        let assembly = &world
            .get::<Arsenal>(actor)
            .unwrap()
            .current()
            .spec()
            .assembly;
        let exit = assembly.anchors[&AnchorKind::DeviceExit];
        let bore = assembly.anchors[&AnchorKind::Bore];
        let event = world
            .resource::<WorldState>()
            .fired_projectiles
            .last()
            .unwrap();
        assert!(Vec3::from(event.muzzle).distance(root.transform_point(exit.translation)) < 1e-5);
        assert!(
            Vec3::from(event.direction).distance(root.rotation * bore.rotation * Vec3::NEG_Z)
                < 1e-5
        );
    }
    assert!(
        shots.iter().all(|count| *count > 0),
        "missing shots in one movement stage: {shots:?}"
    );
}

#[test]
fn unavailable_weapon_pose_blocks_fire_without_spending_ammo_or_replaying_semi_edge() {
    use common::humanoid::PoseState;
    let (mut app, actor, other) = physical_fixture();
    app.world_mut()
        .get_mut::<Transform>(other)
        .unwrap()
        .translation
        .x += 4.0;
    app.world_mut()
        .get_mut::<ClientInput>(actor)
        .unwrap()
        .weapon = EquipmentSlot::Sidearm;
    held_steps(&mut app, actor, 180);
    let origin = app.world().get::<Transform>(actor).unwrap().translation;
    let obstacle = app
        .world_mut()
        .spawn((
            SurfaceKind::Concrete,
            Collider::cuboid(1.0, 1.5, 1.0),
            Transform::from_translation(origin),
        ))
        .id();
    held_steps(&mut app, actor, 3);
    assert!(
        app.world()
            .get::<PoseState>(actor)
            .unwrap()
            .weapon_root
            .unpack()
            .is_none()
    );
    let count = app
        .world()
        .get::<Arsenal>(actor)
        .unwrap()
        .current()
        .rounds();
    let first_id = app.world().resource::<WorldState>().next_projectile_id;
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
        input.fire = true;
        input.fire_pressed_sequence += 1;
    }
    held_steps(&mut app, actor, 3);
    assert_eq!(
        app.world()
            .get::<Arsenal>(actor)
            .unwrap()
            .current()
            .rounds(),
        count
    );
    assert_eq!(
        app.world().resource::<WorldState>().next_projectile_id,
        first_id
    );
    app.world_mut().despawn(obstacle);
    held_steps(&mut app, actor, 8);
    assert!(
        app.world()
            .get::<PoseState>(actor)
            .unwrap()
            .weapon_root
            .unpack()
            .is_some()
    );
    assert_eq!(
        app.world().resource::<WorldState>().next_projectile_id,
        first_id,
        "blocked semi-auto press must not replay after clearance"
    );
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
        input.fire_pressed_sequence += 1;
    }
    held_steps(&mut app, actor, 1);
    assert_eq!(
        app.world().resource::<WorldState>().next_projectile_id,
        first_id + 1
    );
}

#[test]
fn configured_weapon_root_remains_available_while_prone() {
    use common::humanoid::PoseState;
    let (mut app, actor, other) = physical_fixture();
    app.world_mut()
        .get_mut::<Transform>(other)
        .unwrap()
        .translation
        .x += 4.0;
    app.world_mut()
        .get_mut::<ClientInput>(actor)
        .unwrap()
        .prone_sequence = 1;
    held_steps(&mut app, actor, 80);
    let state = app.world().get::<PoseState>(actor).unwrap();
    assert!(state.prone);
    assert!(
        state.weapon_root.unpack().is_some(),
        "weapon pose unavailable while prone"
    );
}

#[test]
fn configured_weapon_root_follows_a_jump_and_landing_without_losing_reach() {
    use common::humanoid::PoseState;
    let (mut app, actor, other) = physical_fixture();
    app.world_mut()
        .get_mut::<Transform>(other)
        .unwrap()
        .translation
        .x += 4.0;
    app.world_mut().get_mut::<ClientInput>(actor).unwrap().jump = true;
    let start = app.world().get::<Transform>(actor).unwrap().translation.y;
    let mut peak = start;
    for tick in 0..120 {
        held_steps(&mut app, actor, 1);
        let root = app.world().get::<Transform>(actor).unwrap();
        peak = peak.max(root.translation.y);
        assert!(
            app.world()
                .get::<PoseState>(actor)
                .unwrap()
                .weapon_root
                .unpack()
                .is_some(),
            "weapon lost its connected pose at jump tick {tick}"
        );
    }
    assert!(peak > start + 0.2, "fixture must actually jump");
}

#[test]
fn configured_weapon_root_retracts_at_wall_and_returns_after_retreat() {
    use common::{catalog::AnchorKind, humanoid::PoseState};

    let (mut app, actor, other) = physical_fixture();
    app.world_mut()
        .get_mut::<Transform>(other)
        .unwrap()
        .translation
        .x += 4.0;
    let origin = app.world().get::<Transform>(actor).unwrap().translation;
    let outlet = |app: &App| {
        let root = app
            .world()
            .get::<PoseState>(actor)
            .unwrap()
            .weapon_root
            .unpack()?;
        let actor_pose = *app.world().get::<Transform>(actor).unwrap();
        let spec = app
            .world()
            .get::<Arsenal>(actor)
            .unwrap()
            .current()
            .presentation_spec();
        let exit = spec
            .assembly
            .anchors
            .get(&AnchorKind::DeviceExit)
            .or_else(|| spec.assembly.anchors.get(&AnchorKind::Bore))?;
        Some(
            actor_pose
                .mul_transform(root)
                .mul_transform(*exit)
                .translation,
        )
    };
    let open = outlet(&app).expect("unobstructed configured root");
    let face = origin.z - 0.55;
    let wall = app
        .world_mut()
        .spawn((
            SurfaceKind::Concrete,
            Collider::cuboid(1.5, 1.5, 0.05),
            Transform::from_xyz(origin.x, origin.y, face - 0.05),
        ))
        .id();
    held_steps(&mut app, actor, 8);
    let blocked = outlet(&app).expect("reachable retracted pose");
    assert!(
        blocked.z >= face + 0.06,
        "outlet {blocked:?} beyond face {face}"
    );
    assert!(
        blocked.z > open.z + 0.10,
        "weapon did not retract: {open:?} -> {blocked:?}"
    );
    app.world_mut().despawn(wall);
    let mut prior = blocked;
    for tick in 0..32 {
        held_steps(&mut app, actor, 1);
        let current = outlet(&app).expect("weapon stays usable during recovery");
        assert!(
            current.distance(prior) < 0.035,
            "recovery jumped at tick {tick}"
        );
        prior = current;
    }
    let recovered = outlet(&app).expect("weapon should extend after wall removal");
    assert!(
        recovered.distance(open) < 0.04,
        "weapon did not recover: {open:?} -> {recovered:?}"
    );
}

#[test]
fn configured_weapon_root_avoids_an_approaching_actor() {
    use common::{catalog::AnchorKind, humanoid::PoseState};
    let (mut app, actor, other) = physical_fixture();
    let origin = app.world().get::<Transform>(actor).unwrap().translation;
    let exit = |app: &App| {
        let root = app
            .world()
            .get::<PoseState>(actor)
            .unwrap()
            .weapon_root
            .unpack()?;
        let actor_pose = *app.world().get::<Transform>(actor).unwrap();
        let spec = app
            .world()
            .get::<Arsenal>(actor)
            .unwrap()
            .current()
            .presentation_spec();
        let anchor = spec
            .assembly
            .anchors
            .get(&AnchorKind::DeviceExit)
            .or_else(|| spec.assembly.anchors.get(&AnchorKind::Bore))?;
        Some(
            actor_pose
                .mul_transform(root)
                .mul_transform(*anchor)
                .translation,
        )
    };
    let mut first = None;
    let mut last = Vec3::ZERO;
    for step in 0..=25 {
        let distance = 1.80 - 0.95 * step as f32 / 25.0;
        app.world_mut()
            .get_mut::<Transform>(other)
            .unwrap()
            .translation = origin + Vec3::new(0.35, 0.0, -distance);
        held_steps(&mut app, actor, 3);
        let muzzle = exit(&app).unwrap_or_else(|| {
            panic!("reachable pose while another player approaches, step={step}")
        });
        let other_root = app.world().get::<Transform>(other).unwrap().translation;
        let axis_y = muzzle.y.clamp(
            other_root.y - PLAYER_COLLIDER_HALF_HEIGHT,
            other_root.y + PLAYER_COLLIDER_HALF_HEIGHT,
        );
        let distance_to_axis = muzzle.distance(Vec3::new(other_root.x, axis_y, other_root.z));
        assert!(
            distance_to_axis > PLAYER_COLLIDER_RADIUS + 0.055,
            "outlet inside player at step {step}: {distance_to_axis}"
        );
        first.get_or_insert(muzzle);
        last = muzzle;
    }
    assert!(
        last.distance(first.unwrap()) > 0.08,
        "weapon did not retract or move aside as player approached"
    );
}

#[test]
fn configured_weapon_root_avoids_ragdoll_and_rubble_collision_groups() {
    use common::humanoid::PoseState;
    for group in [Group::GROUP_2, Group::GROUP_4] {
        let (mut app, actor, other) = physical_fixture();
        app.world_mut()
            .get_mut::<Transform>(other)
            .unwrap()
            .translation
            .x += 4.0;
        let world_outlet = |app: &App| {
            let root = app
                .world()
                .get::<PoseState>(actor)
                .unwrap()
                .weapon_root
                .unpack()?;
            let actor_root = *app.world().get::<Transform>(actor).unwrap();
            let spec = app
                .world()
                .get::<Arsenal>(actor)
                .unwrap()
                .current()
                .presentation_spec();
            Some(
                actor_root
                    .mul_transform(root)
                    .mul_transform(common::weapon_animation::configured_outlet(spec))
                    .translation,
            )
        };
        let open = world_outlet(&app).unwrap();
        let obstacle = app
            .world_mut()
            .spawn((
                RigidBody::KinematicPositionBased,
                Collider::ball(0.08),
                CollisionGroups::new(group, Group::GROUP_1 | group),
                Transform::from_translation(open),
            ))
            .id();
        held_steps(&mut app, actor, 5);
        let resolved = world_outlet(&app).expect("pose around moving debris");
        let center = app.world().get::<Transform>(obstacle).unwrap().translation;
        assert!(
            resolved.distance(center) > 0.13,
            "outlet overlaps debris in {group:?}"
        );
        assert!(resolved.distance(open) > 0.08);
    }
}

#[test]
fn configured_weapon_root_stays_clear_while_approaching_sliding_and_retreating() {
    use common::{catalog::AnchorKind, humanoid::PoseState};
    let (mut app, actor, other) = physical_fixture();
    app.world_mut()
        .get_mut::<Transform>(other)
        .unwrap()
        .translation
        .x += 4.0;
    let origin = app.world().get::<Transform>(actor).unwrap().translation;
    let face = origin.z - 1.35;
    app.world_mut().spawn((
        SurfaceKind::Concrete,
        Collider::cuboid(3.0, 1.5, 0.05),
        Transform::from_xyz(origin.x, origin.y, face - 0.05),
    ));
    held_steps(&mut app, actor, 4);
    let mut previous = None;
    for tick in 0..220 {
        {
            let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
            input.forward = tick < 90;
            input.right = (90..150).contains(&tick);
            input.backward = tick >= 150;
        }
        held_steps(&mut app, actor, 1);
        let pose = app
            .world()
            .get::<PoseState>(actor)
            .unwrap()
            .weapon_root
            .unpack()
            .unwrap_or_else(|| {
                panic!(
                    "pose unavailable at tick {tick}, state={:?}, root={:?}",
                    app.world().get::<PoseState>(actor).unwrap(),
                    app.world().get::<Transform>(actor).unwrap()
                )
            });
        let actor_pose = *app.world().get::<Transform>(actor).unwrap();
        let spec = app
            .world()
            .get::<Arsenal>(actor)
            .unwrap()
            .current()
            .presentation_spec();
        let anchor = spec
            .assembly
            .anchors
            .get(&AnchorKind::DeviceExit)
            .or_else(|| spec.assembly.anchors.get(&AnchorKind::Bore))
            .unwrap();
        let outlet = actor_pose
            .mul_transform(pose)
            .mul_transform(*anchor)
            .translation;
        let weapon_world = actor_pose.mul_transform(pose);
        for bounds in &spec.assembly.obstruction {
            for x in [bounds.min[0], bounds.max[0]] {
                for y in [bounds.min[1], bounds.max[1]] {
                    for z in [bounds.min[2], bounds.max[2]] {
                        let point = weapon_world.transform_point(Vec3::new(x, y, z));
                        assert!(
                            point.z >= face - 0.005,
                            "configured part crossed wall at tick {tick}: {point:?}"
                        );
                    }
                }
            }
        }
        let state = app.world().get::<PoseState>(actor).unwrap();
        let body = app.world().get::<BodyHealth>(actor).unwrap();
        let armor = app.world().get::<ArmorState>(actor).unwrap();
        let weapon = app.world().get::<Arsenal>(actor).unwrap().current();
        let mechanics = weapon.mechanics().snapshot();
        let rig = common::humanoid::configured::evaluate_with_carry(
            state,
            [Region::LeftLeg, Region::RightLeg].map(|r| body.anatomy.capacity(r)),
            spec,
            &mechanics,
            None,
            Some(common::humanoid::configured::shoulder_cover(armor)),
        )
        .unwrap();
        let shoulders = rig.posture.unwrap().shoulders;
        let contacts =
            common::humanoid::handling::WeaponContacts::from_assembly(&spec.assembly).unwrap();
        for (shoulder, grip) in shoulders
            .into_iter()
            .zip([contacts.support, contacts.trigger])
        {
            let reach = shoulder.distance(pose.transform_point(grip.translation));
            assert!(
                (0.095..=0.581).contains(&reach),
                "disconnected grip at tick {tick}: {reach}"
            );
        }
        assert!(
            outlet.z >= face + 0.05,
            "wall crossed at tick {tick}: {outlet:?}"
        );
        if let Some(last) = previous {
            assert!(
                outlet.distance(last) < 0.10,
                "discontinuous outlet at tick {tick}"
            );
        }
        previous = Some(outlet);
    }
}

#[test]
fn configured_weapon_root_handles_a_corner_and_retreat() {
    use common::humanoid::PoseState;
    let (mut app, actor, other) = physical_fixture();
    app.world_mut()
        .get_mut::<Transform>(other)
        .unwrap()
        .translation
        .x -= 4.0;
    let origin = app.world().get::<Transform>(actor).unwrap().translation;
    let front = origin.z - 1.35;
    let side = origin.x + 1.15;
    app.world_mut().spawn((
        SurfaceKind::Concrete,
        Collider::cuboid(3.0, 1.5, 0.05),
        Transform::from_xyz(origin.x, origin.y, front - 0.05),
    ));
    app.world_mut().spawn((
        SurfaceKind::Concrete,
        Collider::cuboid(0.05, 1.5, 3.0),
        Transform::from_xyz(side + 0.05, origin.y, origin.z),
    ));
    held_steps(&mut app, actor, 4);
    let mut previous = None;
    for tick in 0..200 {
        {
            let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
            input.forward = tick < 110;
            input.right = tick < 110;
            input.backward = tick >= 110;
            input.left = tick >= 110;
        }
        held_steps(&mut app, actor, 1);
        let pose = app
            .world()
            .get::<PoseState>(actor)
            .unwrap()
            .weapon_root
            .unpack()
            .unwrap_or_else(|| panic!("pose unavailable in corner at tick {tick}"));
        let actor_root = *app.world().get::<Transform>(actor).unwrap();
        let spec = app
            .world()
            .get::<Arsenal>(actor)
            .unwrap()
            .current()
            .presentation_spec();
        let weapon = actor_root.mul_transform(pose);
        for bounds in &spec.assembly.obstruction {
            for x in [bounds.min[0], bounds.max[0]] {
                for y in [bounds.min[1], bounds.max[1]] {
                    for z in [bounds.min[2], bounds.max[2]] {
                        let point = weapon.transform_point(Vec3::new(x, y, z));
                        assert!(
                            point.z >= front - 0.005 && point.x <= side + 0.005,
                            "configured part crossed corner at tick {tick}: {point:?}"
                        );
                    }
                }
            }
        }
        let outlet = weapon
            .mul_transform(common::weapon_animation::configured_outlet(spec))
            .translation;
        if let Some(last) = previous {
            assert!(
                outlet.distance(last) < 0.10,
                "corner transition jumped at tick {tick}: {last:?} -> {outlet:?}, delta={}",
                outlet.distance(last)
            );
        }
        previous = Some(outlet);
    }
}

#[test]
fn configured_weapon_root_does_not_tunnel_through_thin_wall() {
    use common::humanoid::PoseState;
    let (mut app, actor, other) = physical_fixture();
    app.world_mut()
        .get_mut::<Transform>(other)
        .unwrap()
        .translation
        .x += 4.0;
    let origin = app.world().get::<Transform>(actor).unwrap().translation;
    app.world_mut().spawn((
        SurfaceKind::Concrete,
        Collider::cuboid(2.0, 1.5, 0.005),
        Transform::from_xyz(origin.x, origin.y, origin.z - 1.10),
    ));
    held_steps(&mut app, actor, 3);
    assert!(
        app.world()
            .get::<PoseState>(actor)
            .unwrap()
            .weapon_root
            .unpack()
            .is_some()
    );
    // Simulate a large position correction within the same life. The two end
    // poses are clear, but the weapon would have swept through the thin wall.
    app.world_mut()
        .get_mut::<Transform>(actor)
        .unwrap()
        .translation
        .z -= 1.8;
    held_steps(&mut app, actor, 1);
    assert!(
        app.world()
            .get::<PoseState>(actor)
            .unwrap()
            .weapon_root
            .unpack()
            .is_none()
    );
    // A new life resets sweep history and can adopt the clear end pose. This
    // distinguishes crossing rejection from an obstructed endpoint.
    app.world_mut()
        .get_mut::<super::super::arena::Life>(actor)
        .unwrap()
        .generation += 1;
    held_steps(&mut app, actor, 3);
    assert!(
        app.world()
            .get::<PoseState>(actor)
            .unwrap()
            .weapon_root
            .unpack()
            .is_some(),
        "the far-side endpoint should be clear after lifecycle reset"
    );
}

#[test]
fn configured_weapon_root_overlap_does_not_disable_sweep_against_other_wall() {
    use common::humanoid::PoseState;
    let (mut app, actor, other) = physical_fixture();
    app.world_mut()
        .get_mut::<Transform>(other)
        .unwrap()
        .translation
        .x += 4.0;
    let origin = app.world().get::<Transform>(actor).unwrap().translation;
    let root = app
        .world()
        .get::<PoseState>(actor)
        .unwrap()
        .weapon_root
        .unpack()
        .unwrap();
    let spec = app
        .world()
        .get::<Arsenal>(actor)
        .unwrap()
        .current()
        .presentation_spec();
    let outlet = app
        .world()
        .get::<Transform>(actor)
        .unwrap()
        .mul_transform(root)
        .mul_transform(common::weapon_animation::configured_outlet(spec))
        .translation;
    app.world_mut().spawn((
        SurfaceKind::Concrete,
        Collider::ball(0.05),
        Transform::from_translation(outlet),
    ));
    app.world_mut().spawn((
        SurfaceKind::Concrete,
        Collider::cuboid(2.0, 1.5, 0.005),
        Transform::from_xyz(origin.x, origin.y, origin.z - 1.10),
    ));
    // One update publishes both obstacles into Rapier after pose resolution.
    held_steps(&mut app, actor, 1);
    app.world_mut()
        .get_mut::<Transform>(actor)
        .unwrap()
        .translation
        .z -= 1.8;
    held_steps(&mut app, actor, 1);
    assert!(
        app.world()
            .get::<PoseState>(actor)
            .unwrap()
            .weapon_root
            .unpack()
            .is_none(),
        "an initial overlap must not permit tunnelling through a different wall"
    );
}

#[test]
fn ads_sight_stays_on_eye_axis_during_strafe_and_reversal() {
    use common::{catalog::AnchorKind, humanoid::PoseState};

    let (mut app, shooter, other) = physical_fixture();
    app.world_mut()
        .get_mut::<Transform>(other)
        .unwrap()
        .translation
        .z -= 5.0;
    app.world_mut().get_mut::<ClientInput>(shooter).unwrap().aim = true;
    held_steps(&mut app, shooter, 100);
    let mut worst = 0.0_f32;
    for (left, right, frames) in [(true, false, 48), (false, true, 96), (false, false, 48)] {
        let mut input = app.world_mut().get_mut::<ClientInput>(shooter).unwrap();
        input.left = left;
        input.right = right;
        let mut peak = 0.0_f32;
        for _ in 0..frames {
            held_steps(&mut app, shooter, 1);
            let world = app.world();
            let pose = world.get::<PoseState>(shooter).unwrap();
            let root = pose.weapon_root.unpack().expect("clear strafing pose");
            let arsenal = world.get::<Arsenal>(shooter).unwrap();
            let spec = arsenal.current().spec();
            let sight = root.transform_point(spec.assembly.anchors[&AnchorKind::Sight].translation);
            let error = (sight - Vec3::Y * PLAYER_VIEW_HEIGHT)
                .cross(Vec3::NEG_Z)
                .length();
            worst = worst.max(error);
            let direction = if left { -1.0 } else { 1.0 };
            peak = peak.max(pose.velocity[0] * direction);
        }
        if left || right {
            assert!(peak > 2.5, "fixture must reach lateral walking speed");
        }
    }
    assert!(worst < 0.01, "strafe displaced the sight by {worst:.3} m");
}
