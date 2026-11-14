use super::*;
use bevy::{time::TimeUpdateStrategy, transform::TransformPlugin};
use std::time::Duration;

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin, common::Plugin))
        .add_message::<ServerEvent>()
        .add_message::<bevy_renet2::netcode::NetcodeTransportError>()
        .insert_resource(RenetServer::new(data::renet_config()))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 128.0,
        )))
        .add_plugins(Plugin);
    app.update();
    app
}

fn add_player(app: &mut App, id: u64, loadout: Loadout) -> Entity {
    let player = app
        .world_mut()
        .spawn((
            character_bundle(id, Vec3::from(common::map::SPAWN_CENTER)),
            Participant::default(),
            ArmorState::default(),
            Arsenal::new(loadout),
        ))
        .id();
    app.world_mut()
        .get_mut::<ClientInput>(player)
        .unwrap()
        .loadout = loadout;
    app.world_mut()
        .resource_mut::<Lobby>()
        .players
        .insert(id, player);
    player
}

fn steps(app: &mut App, count: usize) {
    for _ in 0..count {
        app.update();
    }
}

fn held_steps(app: &mut App, entity: Entity, count: usize) {
    for _ in 0..count {
        let now = app.world().resource::<Time>().elapsed_secs();
        app.world_mut()
            .get_mut::<LastInput>(entity)
            .unwrap()
            .received_at = now;
        app.update();
    }
}

fn ready(app: &mut App, player: Entity) {
    app.world_mut()
        .get_mut::<ClientInput>(player)
        .unwrap()
        .lobby_sequence += 1;
}

#[test]
fn lobby_blocks_movement_and_fire_then_starts_players_at_distinct_spawns() {
    let mut app = app();
    let a = add_player(&mut app, 7, Loadout::default());
    let b = add_player(&mut app, 8, Loadout::default());
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(a).unwrap();
        input.forward = true;
        input.fire = true;
    }
    let before = app.world().get::<Transform>(a).unwrap().translation;
    steps(&mut app, 30);
    assert_eq!(app.world().get::<Transform>(a).unwrap().translation, before);
    assert_eq!(
        app.world_mut()
            .query::<&Projectile>()
            .iter(app.world())
            .count(),
        0
    );
    ready(&mut app, a);
    steps(&mut app, 10);
    assert_eq!(app.world().resource::<RaidState>().phase, RaidPhase::Lobby);
    ready(&mut app, b);
    steps(&mut app, 390);
    assert_eq!(app.world().resource::<RaidState>().phase, RaidPhase::Active);
    let delta = app.world().get::<Transform>(a).unwrap().translation
        - app.world().get::<Transform>(b).unwrap().translation;
    assert!(delta.length() >= 1.4);
    assert_eq!(app.world_mut().query::<&Bot>().iter(app.world()).count(), 2);
}

#[test]
fn server_locks_equipment_and_late_joiners_wait() {
    let mut app = app();
    let loadout = Loadout {
        primary: WeaponKind::Smg,
        armor: ArmorKind::Heavy,
        ammo: AmmoKind::Ap,
        helmet: true,
    };
    let player = add_player(&mut app, 7, loadout);
    ready(&mut app, player);
    steps(&mut app, 390);
    assert_eq!(
        app.world().get::<Participant>(player).unwrap().loadout,
        loadout
    );
    assert_eq!(app.world().get::<Arsenal>(player).unwrap().magazines[2], 25);
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(player).unwrap();
        input.loadout = Loadout::default();
        input.weapon = WeaponKind::Rifle;
    }
    steps(&mut app, 2);
    assert_eq!(
        app.world().get::<Participant>(player).unwrap().loadout,
        loadout
    );
    assert_eq!(
        app.world().get::<Arsenal>(player).unwrap().active,
        WeaponKind::Smg
    );
    let late = add_player(&mut app, 8, Loadout::default());
    ready(&mut app, late);
    steps(&mut app, 3);
    assert_eq!(
        app.world().get::<Participant>(late).unwrap().status,
        RaidStatus::Waiting
    );
    assert!(app.world().get::<ColliderDisabled>(late).is_some());
}

#[test]
fn ecs_lobby_to_loot_extraction_debrief_and_second_raid() {
    let mut app = app();
    let player = add_player(&mut app, 7, Loadout::default());
    ready(&mut app, player);
    steps(&mut app, 390);
    app.world_mut()
        .get_mut::<Transform>(player)
        .unwrap()
        .translation = Vec3::from(common::map::LOOT_POSITIONS[0]) + Vec3::Y * 0.5;
    app.world_mut()
        .get_mut::<ClientInput>(player)
        .unwrap()
        .interact = true;
    held_steps(&mut app, player, 2);
    assert_eq!(app.world().get::<Participant>(player).unwrap().carried, 1);
    app.world_mut()
        .get_mut::<Transform>(player)
        .unwrap()
        .translation = Vec3::from(common::map::EXTRACTION_CENTER) + Vec3::Y * 0.9;
    held_steps(&mut app, player, 650);
    let result = app.world().get::<Participant>(player).unwrap();
    assert_eq!(result.status, RaidStatus::Extracted);
    assert_eq!(result.secured, 1);
    assert_eq!(result.carried, 0);
    assert_eq!(
        app.world().resource::<RaidState>().phase,
        RaidPhase::Debrief
    );
    ready(&mut app, player);
    steps(&mut app, 2);
    assert_eq!(app.world().resource::<RaidState>().phase, RaidPhase::Lobby);
    let result = app.world().get::<Participant>(player).unwrap();
    assert!(!result.ready);
    assert_eq!(result.status, RaidStatus::Waiting);
    assert_eq!(result.secured, 1);
    assert_eq!(app.world_mut().query::<&Bot>().iter(app.world()).count(), 0);
    ready(&mut app, player);
    steps(&mut app, 390);
    assert_eq!(app.world().resource::<RaidState>().raid_id, 2);
    assert_eq!(
        app.world().resource::<RaidState>().loot_available,
        [true; 3]
    );
    assert_eq!(app.world().get::<Participant>(player).unwrap().secured, 1);
    assert_eq!(
        app.world().get::<BodyHealth>(player).unwrap().parts,
        [35.0, 85.0, 60.0, 70.0]
    );
    assert_eq!(app.world_mut().query::<&Bot>().iter(app.world()).count(), 2);
}

#[test]
fn death_and_timeout_lose_loot_and_space_cannot_respawn() {
    let mut app = app();
    let player = add_player(&mut app, 7, Loadout::default());
    ready(&mut app, player);
    steps(&mut app, 390);
    app.world_mut()
        .get_mut::<Participant>(player)
        .unwrap()
        .carried = 2;
    app.world_mut().get_mut::<BodyHealth>(player).unwrap().parts[0] = 0.0;
    app.world_mut().get_mut::<ClientInput>(player).unwrap().jump = true;
    steps(&mut app, 3);
    assert_eq!(
        app.world().get::<Participant>(player).unwrap().status,
        RaidStatus::Dead
    );
    assert_eq!(app.world().get::<Participant>(player).unwrap().carried, 0);
    assert_eq!(app.world().get::<BodyHealth>(player).unwrap().parts[0], 0.0);
    ready(&mut app, player);
    steps(&mut app, 2);
    ready(&mut app, player);
    steps(&mut app, 390);
    app.world_mut()
        .get_mut::<Participant>(player)
        .unwrap()
        .carried = 2;
    app.world_mut().resource_mut::<RaidState>().seconds_left = 0.01;
    steps(&mut app, 3);
    assert_eq!(
        app.world().get::<Participant>(player).unwrap().status,
        RaidStatus::Missing
    );
    assert_eq!(app.world().get::<Participant>(player).unwrap().carried, 0);
    assert_eq!(app.world().get::<Participant>(player).unwrap().secured, 0);
}

#[test]
fn invalid_packets_and_non_finite_camera_are_rejected_and_stale_input_is_neutralized() {
    assert!(validated_input(&[1, 2, 3]).is_none());
    let mut bad = ClientInput::default();
    bad.camera.yaw = f32::NAN;
    assert!(validated_input(&data::encode(&bad)).is_none());
    bad.camera.yaw = 0.0;
    assert!(validated_input(&data::encode(&bad)).is_some());
    let mut app = app();
    let player = add_player(&mut app, 7, Loadout::default());
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(player).unwrap();
        input.forward = true;
        input.fire = true;
        input.interact = true;
    }
    steps(&mut app, 80);
    let input = app.world().get::<ClientInput>(player).unwrap();
    assert!(!input.forward);
    assert!(!input.fire);
    assert!(!input.interact);
}

fn cover_hit(surface: SurfaceKind, half_thickness: f32) -> (f32, usize) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin, common::Plugin))
        .add_message::<bevy_renet2::netcode::NetcodeTransportError>()
        .insert_resource(Time::<Fixed>::from_hz(128.0))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 128.0,
        )))
        .init_resource::<WorldState>()
        .add_systems(FixedUpdate, projectiles_tick.after(PhysicsSet::Writeback));
    app.update();
    let owner = app.world_mut().spawn_empty().id();
    let mut p = Participant::default();
    p.begin();
    let target = app
        .world_mut()
        .spawn((
            Client { id: 7 },
            BodyHealth::default(),
            ArmorState::default(),
            MovementState::default(),
            p,
            Collider::capsule_y(PLAYER_COLLIDER_HALF_HEIGHT, PLAYER_COLLIDER_RADIUS),
            Transform::from_xyz(0.0, 1.0, 4.0),
        ))
        .id();
    app.world_mut().spawn((
        surface,
        Collider::cuboid(1.0, 1.0, half_thickness),
        Transform::from_xyz(0.0, 1.0, 2.0),
    ));
    // Allow Rapier to register the newly spawned colliders before launching the shot.
    steps(&mut app, 3);
    app.world_mut().spawn((
        Projectile {
            id: 1,
            velocity: Vec3::Z * 100.0,
            damage: 50.0,
            penetration: 30.0,
            lifetime: PROJECTILE_LIFETIME,
            owner_entity: owner,
            penetrated: Vec::new(),
        },
        Transform::from_xyz(0.0, 1.0, 0.0),
    ));
    steps(&mut app, 12);
    let health = app.world().get::<BodyHealth>(target).unwrap().parts[1];
    let projectiles = app
        .world_mut()
        .query::<&Projectile>()
        .iter(app.world())
        .count();
    (health, projectiles)
}

#[test]
fn real_rapier_projectile_penetrates_thin_wood_and_loses_energy_once() {
    let (health, projectiles) = cover_hit(SurfaceKind::Wood, 0.12);
    // 0.24 m wood costs 4.8 of 30; 50 * (25.2/30) = 42 damage; 85 - 42 = 43.
    assert!(
        (health - 43.0).abs() < 0.1,
        "actual thorax health: {health}"
    );
    assert_eq!(projectiles, 0);
}

#[test]
fn real_rapier_concrete_and_thick_wood_stop_projectiles() {
    assert_eq!(cover_hit(SurfaceKind::Concrete, 0.12), (85.0, 0));
    assert_eq!(cover_hit(SurfaceKind::Wood, 1.0), (85.0, 0));
}

#[test]
fn empty_server_returns_to_lobby() {
    let mut app = app();
    let player = add_player(&mut app, 7, Loadout::default());
    ready(&mut app, player);
    steps(&mut app, 390);
    app.world_mut().entity_mut(player).despawn();
    app.world_mut().resource_mut::<Lobby>().players.clear();
    steps(&mut app, 2);
    assert_eq!(app.world().resource::<RaidState>().phase, RaidPhase::Lobby);
    assert_eq!(app.world_mut().query::<&Bot>().iter(app.world()).count(), 0);
}

// TK-05..10: fixtures change initial placement only; shots run Arsenal -> projectile -> Rapier/anatomy.
fn physical_fixture() -> (App, Entity, Entity) {
    let mut app = app();
    let loadout = Loadout {
        armor: ArmorKind::None,
        helmet: false,
        ..default()
    };
    let a = add_player(&mut app, 7, loadout);
    let b = add_player(&mut app, 8, loadout);
    ready(&mut app, a);
    ready(&mut app, b);
    steps(&mut app, 390);
    let bots: Vec<_> = app
        .world_mut()
        .query_filtered::<Entity, With<Bot>>()
        .iter(app.world())
        .collect();
    for bot in bots {
        app.world_mut().entity_mut(bot).despawn();
    }
    app.world_mut().get_mut::<Transform>(a).unwrap().translation = Vec3::new(-10.0, 1.05, 12.0);
    app.world_mut().get_mut::<Transform>(b).unwrap().translation = Vec3::new(-10.0, 1.05, 8.0);
    steps(&mut app, 30);
    (app, a, b)
}

fn normal_shot(app: &mut App, shooter: Entity, target: Vec3) {
    let t = *app.world().get::<Transform>(shooter).unwrap();
    let eye = t.translation + Vec3::Y * PLAYER_VIEW_HEIGHT;
    let mut rotation = Transform::from_translation(eye)
        .looking_at(target, Vec3::Y)
        .rotation;
    for _ in 0..8 {
        let muzzle = eye + rotation * Vec3::from(WeaponKind::Rifle.spec().barrel_offset);
        rotation = Transform::from_translation(muzzle)
            .looking_at(target, Vec3::Y)
            .rotation;
    }
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(shooter).unwrap();
        input.camera = rotation.into();
        input.fire = true;
    }
    held_steps(app, shooter, 1);
    app.world_mut()
        .get_mut::<ClientInput>(shooter)
        .unwrap()
        .fire = false;
    held_steps(app, shooter, 18);
}

#[test]
fn normal_projectiles_resolve_left_right_and_pass_the_leg_gap() {
    use common::humanoid::*;
    for (offset, region) in [(-0.13, Region::LeftLeg), (0.13, Region::RightLeg)] {
        let (mut app, a, b) = physical_fixture();
        let t = app.world().get::<Transform>(b).unwrap().translation;
        normal_shot(&mut app, a, t + Vec3::new(offset, -0.68, 0.0));
        let body = app.world().get::<BodyHealth>(b).unwrap();
        assert_eq!(body.anatomy.last_region, Some(region));
        assert!(body.anatomy.capacity(region) < 0.6);
        assert_ne!(
            app.world().get::<PoseState>(b).unwrap().left_support,
            app.world().get::<PoseState>(b).unwrap().right_support
        );
    }
    let (mut app, a, b) = physical_fixture();
    let t = app.world().get::<Transform>(b).unwrap().translation;
    normal_shot(&mut app, a, t + Vec3::new(0.0, -0.68, 0.0));
    assert_eq!(
        app.world().get::<BodyHealth>(b).unwrap().anatomy.regions,
        REGION_MAX
    );
    assert!(
        app.world()
            .resource::<WorldState>()
            .impact_marks
            .iter()
            .any(|m| m.pos[2] < 7.5)
    );
}

#[test]
fn live_separation_spawns_once_and_death_transfers_articulated_pose() {
    use common::humanoid::*;
    let (mut app, a, b) = physical_fixture();
    for _ in 0..3 {
        let t = app.world().get::<Transform>(b).unwrap().translation;
        normal_shot(&mut app, a, t + Vec3::new(-0.13, -0.68, 0.0));
    }
    let missing = app.world().get::<BodyHealth>(b).unwrap().anatomy.missing;
    assert!(
        missing & (1 << Segment::LeftShin.index()) != 0,
        "missing mask {missing}"
    );
    let count = app
        .world_mut()
        .query::<&physical::PhysicsPiece>()
        .iter(app.world())
        .filter(|p| p.detached)
        .count();
    assert_eq!(count, 2);
    steps(&mut app, 30);
    assert_eq!(
        app.world_mut()
            .query::<&physical::PhysicsPiece>()
            .iter(app.world())
            .filter(|p| p.detached)
            .count(),
        2
    );
    // A separate normal brain shot on an intact fixture tests immediate control loss and joint creation.
    let (mut app, a, b) = physical_fixture();
    let t = app.world().get::<Transform>(b).unwrap().translation;
    normal_shot(&mut app, a, t + Vec3::new(0.0, 0.68, 0.0));
    assert!(!app.world().get::<BodyHealth>(b).unwrap().alive());
    assert_eq!(
        app.world().get::<PoseState>(b).unwrap().mode,
        PhysicalMode::Dead
    );
    assert_eq!(
        app.world_mut()
            .query::<&physical::PhysicsPiece>()
            .iter(app.world())
            .count(),
        SEGMENT_COUNT
    );
    assert_eq!(
        app.world_mut()
            .query::<&ImpulseJoint>()
            .iter(app.world())
            .count(),
        SEGMENT_COUNT - 1
    );
    assert_eq!(
        app.world().get::<MovementState>(b).unwrap().velocity,
        Vec3::ZERO
    );
}

#[test]
fn support_controller_corrects_small_impulse_and_recovers_from_large_impulse() {
    use common::humanoid::*;
    let (mut app, _, b) = physical_fixture();
    app.world_mut().get_mut::<PoseState>(b).unwrap().impulse = [0.5, 0.0, 0.0];
    steps(&mut app, 2);
    assert_eq!(
        app.world().get::<PoseState>(b).unwrap().mode,
        PhysicalMode::Stumbling
    );
    steps(&mut app, 128);
    assert_eq!(
        app.world().get::<PoseState>(b).unwrap().mode,
        PhysicalMode::Standing
    );
    app.world_mut().get_mut::<PoseState>(b).unwrap().impulse = [3.0, 0.0, 0.0];
    steps(&mut app, 2);
    assert_eq!(
        app.world().get::<PoseState>(b).unwrap().mode,
        PhysicalMode::Fallen
    );
    steps(&mut app, 450);
    assert_eq!(
        app.world().get::<PoseState>(b).unwrap().mode,
        PhysicalMode::Standing
    );
    assert!(app.world().get::<Transform>(b).unwrap().translation.y > 0.9);
}

#[test]
fn normal_reload_blocked_triggers_do_not_emit_confirmed_shots() {
    let (mut app, a, _) = physical_fixture();
    app.world_mut().get_mut::<Arsenal>(a).unwrap().magazines[0] = 0;
    app.world_mut().get_mut::<ClientInput>(a).unwrap().fire = true;
    held_steps(&mut app, a, 20);
    assert!(
        app.world()
            .resource::<WorldState>()
            .fired_projectiles
            .is_empty()
    );
    app.world_mut()
        .get_mut::<ClientInput>(a)
        .unwrap()
        .reload_sequence += 1;
    held_steps(&mut app, a, 128);
    assert!(app.world().get::<Arsenal>(a).unwrap().reload_remaining > 0.0);
    assert!(
        app.world()
            .resource::<WorldState>()
            .fired_projectiles
            .is_empty()
    );
    app.world_mut().get_mut::<ClientInput>(a).unwrap().weapon = WeaponKind::Pistol;
    held_steps(&mut app, a, 2);
    assert_eq!(app.world().get::<Arsenal>(a).unwrap().reload_remaining, 0.0);
    assert_eq!(app.world().get::<Arsenal>(a).unwrap().magazines[0], 0);
}

#[test]
fn full_physical_load_stays_bounded_and_reset_removes_physics() {
    use common::humanoid::*;
    let (mut app, a, b) = physical_fixture();
    // Representative worst-case initial fixture: bodies are subsequently owned by the normal physics system.
    for i in 0..20 {
        let mut body = BodyHealth::default();
        body.parts[0] = 0.0;
        let mut p = Participant::default();
        p.begin();
        app.world_mut()
            .spawn((
                character_bundle(
                    100 + i,
                    Vec3::new(
                        -15.0 + (i % 5) as f32 * 2.0,
                        1.05,
                        -15.0 + (i / 5) as f32 * 2.0,
                    ),
                ),
                p,
                ArmorState::default(),
                Arsenal::new(Loadout::default()),
            ))
            .insert(body);
    }
    steps(&mut app, 5);
    assert_eq!(
        app.world_mut()
            .query::<&physical::PhysicsPiece>()
            .iter(app.world())
            .count(),
        physical::MAX_PIECES
    );
    let mut samples = Vec::new();
    for _ in 0..640 {
        let start = std::time::Instant::now();
        app.update();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "TK LOAD 320 pieces tick_ms mean={:.3} p95={:.3} max={:.3}",
        samples.iter().sum::<f64>() / 640.0,
        samples[608],
        samples[639]
    );
    assert!(
        samples[608] < 7.8,
        "p95 tick over budget: {} ms",
        samples[608]
    );
    app.world_mut().get_mut::<BodyHealth>(a).unwrap().parts[0] = 0.0;
    app.world_mut().get_mut::<BodyHealth>(b).unwrap().parts[0] = 0.0;
    steps(&mut app, 3);
    ready(&mut app, a);
    steps(&mut app, 2);
    assert_eq!(app.world().resource::<RaidState>().phase, RaidPhase::Lobby);
    assert_eq!(
        app.world_mut()
            .query::<&physical::PhysicsPiece>()
            .iter(app.world())
            .count(),
        0
    );
    assert_eq!(
        app.world().get::<PoseState>(a).unwrap().mode,
        PhysicalMode::Standing
    );
}

#[test]
fn normal_projectiles_follow_standing_crouched_and_fallen_anatomy() {
    use common::humanoid::*;
    for stance in 0..3 {
        let (mut app, a, b) = physical_fixture();
        app.world_mut().get_mut::<Transform>(a).unwrap().translation = Vec3::new(-14.0, 1.05, 8.0);
        if stance == 1 {
            app.world_mut().get_mut::<ClientInput>(b).unwrap().crouch = true;
            held_steps(&mut app, b, 20);
        }
        if stance == 2 {
            app.world_mut().get_mut::<PoseState>(b).unwrap().impulse = [3.0, 0.0, 0.0];
            steps(&mut app, 60);
        }
        let t = *app.world().get::<Transform>(b).unwrap();
        let pose = app.world().get::<PoseState>(b).unwrap();
        if stance == 1 {
            assert_eq!(pose.crouch, 1.0);
        }
        if stance == 2 {
            assert_eq!(pose.mode, PhysicalMode::Fallen);
        }
        let rig = evaluate_pose(pose, 1.0, 1.0);
        normal_shot(
            &mut app,
            a,
            t.translation + t.rotation * Vec3::from(rig[Segment::Head.index()].position),
        );
        let body = app.world().get::<BodyHealth>(b).unwrap();
        assert_eq!(
            body.anatomy.last_region,
            Some(Region::Head),
            "stance={stance} anatomy={:?}",
            body.anatomy
        );
        assert!(!body.alive(), "stance={stance}");
    }
}

#[test]
fn normal_armor_impact_is_once_and_prevents_internal_heart_damage() {
    use common::humanoid::*;
    let (mut app, a, b) = physical_fixture();
    app.world_mut()
        .entity_mut(b)
        .insert(ArmorState::from_loadout(Loadout {
            armor: ArmorKind::Heavy,
            ..default()
        }));
    let t = *app.world().get::<Transform>(b).unwrap();
    let pose = app.world().get::<PoseState>(b).unwrap();
    let rig = evaluate_pose(pose, 1.0, 1.0);
    let (segment, offset, _) = Organ::Heart.volume();
    let bone = rig[segment.index()];
    let target = t.translation
        + t.rotation * (Vec3::from(bone.position) + Quat::from_array(bone.rotation) * offset);
    normal_shot(&mut app, a, target);
    let body = app.world().get::<BodyHealth>(b).unwrap();
    assert_eq!(body.anatomy.organs[Organ::Heart as usize], 100.0);
    assert_eq!(body.anatomy.last_region, Some(Region::Thorax));
    assert!((app.world().get::<ArmorState>(b).unwrap().vest - 86.4).abs() < 0.01);
    assert!((body.anatomy.regions[Region::Thorax as usize] - 78.2).abs() < 0.01);
    assert_eq!(body.anatomy.wounds, 0);
}

#[test]
fn normal_projectiles_damage_internal_organs_with_distinct_consequences() {
    use common::humanoid::*;
    for organ in [Organ::Heart, Organ::Liver, Organ::Stomach] {
        let (mut app, a, b) = physical_fixture();
        let t = *app.world().get::<Transform>(b).unwrap();
        let pose = app.world().get::<PoseState>(b).unwrap();
        let rig = evaluate_pose(pose, 1.0, 1.0);
        let (segment, offset, _) = organ.volume();
        let bone = rig[segment.index()];
        let target = t.translation
            + t.rotation * (Vec3::from(bone.position) + Quat::from_array(bone.rotation) * offset);
        normal_shot(&mut app, a, target);
        let anatomy = &app.world().get::<BodyHealth>(b).unwrap().anatomy;
        assert!(
            anatomy.organs[organ as usize] < 50.0,
            "{organ:?}: {anatomy:?}"
        );
        assert_eq!(anatomy.last_organ, Some(organ));
        assert!(app.world().get::<BodyHealth>(b).unwrap().alive());
        match organ {
            Organ::Heart => {
                assert!(anatomy.bleed_rate() > 3.0);
                steps(&mut app, 128 * 18);
                assert!(app.world().get::<BodyHealth>(b).unwrap().anatomy.blood < BLOOD_COLLAPSE);
            }
            Organ::Liver => assert!(anatomy.bleed_rate() > 1.5),
            Organ::Stomach => {
                assert!(anatomy.bleed_rate() < 0.2);
                {
                    let mut input = app.world_mut().get_mut::<ClientInput>(b).unwrap();
                    input.run = true;
                    input.forward = true;
                }
                held_steps(&mut app, b, 256);
                assert!(app.world().get::<BodyHealth>(b).unwrap().anatomy.vigor < 75.0);
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn injured_fall_uses_grounded_prone_collision_and_recovers_without_floor_penetration() {
    use common::humanoid::*;
    let (mut app, _, b) = physical_fixture();
    app.world_mut()
        .get_mut::<BodyHealth>(b)
        .unwrap()
        .anatomy
        .separate(Segment::LeftShin);
    app.world_mut().get_mut::<PoseState>(b).unwrap().impulse = [3.0, 0.0, 0.0];
    steps(&mut app, 90);
    let pose = app.world().get::<PoseState>(b).unwrap();
    let t = app.world().get::<Transform>(b).unwrap();
    assert_eq!(pose.mode, PhysicalMode::Fallen);
    assert!(app.world().get::<MovementState>(b).unwrap().grounded);
    assert!(
        (0.38..0.45).contains(&t.translation.y),
        "prone root {:?}",
        t.translation
    );
    let rig = evaluate_pose(pose, 0.0, 1.0);
    assert!(t.translation.y + rig[Segment::Head.index()].position[1] > 0.25);
    steps(&mut app, 400);
    assert_eq!(
        app.world().get::<PoseState>(b).unwrap().mode,
        PhysicalMode::Standing
    );
    assert!(app.world().get::<Transform>(b).unwrap().translation.y > 0.98);
}

#[test]
fn living_fall_waits_for_headroom_before_recovery() {
    use common::humanoid::*;
    let (mut app, _, b) = physical_fixture();
    app.world_mut().get_mut::<PoseState>(b).unwrap().impulse = [3.0, 0.0, 0.0];
    steps(&mut app, 60);
    let position = app.world().get::<Transform>(b).unwrap().translation;
    let roof = app
        .world_mut()
        .spawn((
            SurfaceKind::Concrete,
            Collider::cuboid(0.7, 0.1, 0.8),
            Transform::from_xyz(position.x, 1.4, position.z),
        ))
        .id();
    steps(&mut app, 450);
    assert_eq!(
        app.world().get::<PoseState>(b).unwrap().mode,
        PhysicalMode::Fallen
    );
    assert!(app.world().get::<BodyHealth>(b).unwrap().alive());
    assert!(app.world().get::<MovementState>(b).unwrap().grounded);
    app.world_mut().entity_mut(roof).despawn();
    steps(&mut app, 400);
    assert_eq!(
        app.world().get::<PoseState>(b).unwrap().mode,
        PhysicalMode::Standing
    );
}

#[test]
fn destroyed_trigger_arm_disables_firing_and_reload_while_actor_remains_alive() {
    use common::humanoid::*;
    let (mut app, a, b) = physical_fixture();
    app.world_mut().get_mut::<ClientInput>(b).unwrap().fire = true;
    held_steps(&mut app, b, 1);
    app.world_mut().get_mut::<ClientInput>(b).unwrap().fire = false;
    assert_eq!(app.world().get::<Arsenal>(b).unwrap().magazines[0], 29);
    for _ in 0..2 {
        let t = *app.world().get::<Transform>(b).unwrap();
        let pose = app.world().get::<PoseState>(b).unwrap();
        let rig = evaluate_pose(pose, 1.0, 1.0);
        normal_shot(
            &mut app,
            a,
            t.translation + t.rotation * Vec3::from(rig[Segment::RightUpperArm.index()].position),
        );
    }
    steps(&mut app, 128);
    let body = app.world().get::<BodyHealth>(b).unwrap();
    assert!(body.alive());
    assert_eq!(body.anatomy.capacity(Region::RightArm), 0.0);
    app.world_mut().get_mut::<ClientInput>(b).unwrap().fire = true;
    held_steps(&mut app, b, 20);
    assert_eq!(app.world().get::<Arsenal>(b).unwrap().magazines[0], 29);
    assert!(
        !app.world()
            .resource::<WorldState>()
            .fired_projectiles
            .iter()
            .any(|s| s.shooter == 8)
    );
    app.world_mut()
        .get_mut::<ClientInput>(b)
        .unwrap()
        .reload_sequence += 1;
    held_steps(&mut app, b, 2);
    assert_eq!(app.world().get::<Arsenal>(b).unwrap().reload_remaining, 0.0);
}

#[test]
fn last_participant_death_still_transfers_a_ragdoll_after_entering_debrief() {
    use common::humanoid::*;
    let (mut app, a, b) = physical_fixture();
    let t = app.world().get::<Transform>(b).unwrap().translation;
    normal_shot(&mut app, a, t + Vec3::Y * 0.68);
    assert!(!app.world().get::<BodyHealth>(b).unwrap().alive());
    let t = *app.world().get::<Transform>(a).unwrap();
    let rig = evaluate_pose(app.world().get::<PoseState>(a).unwrap(), 1.0, 1.0);
    let head = t.translation + t.rotation * Vec3::from(rig[Segment::Head.index()].position);
    // A previously fired projectile lands after physical_tick, before objectives enters debrief.
    app.world_mut().spawn((
        Projectile {
            id: 9999,
            velocity: -Vec3::X * 200.0,
            damage: 34.0,
            penetration: 100.0,
            lifetime: 3.0,
            owner_entity: b,
            penetrated: Vec::new(),
        },
        Transform::from_translation(head + Vec3::X),
    ));
    steps(&mut app, 4);
    assert_eq!(
        app.world().resource::<RaidState>().phase,
        RaidPhase::Debrief
    );
    assert_eq!(
        app.world().get::<PoseState>(a).unwrap().mode,
        PhysicalMode::Dead
    );
    assert_eq!(
        app.world_mut()
            .query::<&physical::PhysicsPiece>()
            .iter(app.world())
            .filter(|p| p.owner == 7)
            .count(),
        SEGMENT_COUNT
    );
    steps(&mut app, 4);
    assert_eq!(
        app.world_mut()
            .query::<&physical::PhysicsPiece>()
            .iter(app.world())
            .filter(|p| p.owner == 7)
            .count(),
        SEGMENT_COUNT
    );
}

#[test]
fn next_deploy_restores_separated_anatomy_and_clears_fragments_corpses_and_effects() {
    use common::humanoid::*;
    let (mut app, a, b) = physical_fixture();
    for _ in 0..3 {
        let t = app.world().get::<Transform>(b).unwrap().translation;
        normal_shot(&mut app, a, t + Vec3::new(-0.13, -0.68, 0.0));
    }
    assert_ne!(app.world().get::<BodyHealth>(b).unwrap().anatomy.missing, 0);
    assert!(
        app.world_mut()
            .query::<&physical::PhysicsPiece>()
            .iter(app.world())
            .any(|p| p.detached)
    );
    let t = app.world().get::<Transform>(b).unwrap().translation;
    normal_shot(&mut app, a, t + Vec3::Y * 0.68);
    app.world_mut().get_mut::<BodyHealth>(a).unwrap().parts[0] = 0.0;
    steps(&mut app, 4);
    assert_eq!(
        app.world().resource::<RaidState>().phase,
        RaidPhase::Debrief
    );
    assert!(
        app.world_mut()
            .query::<&physical::PhysicsPiece>()
            .iter(app.world())
            .any(|p| !p.detached)
    );
    ready(&mut app, a);
    steps(&mut app, 2);
    assert_eq!(app.world().resource::<RaidState>().phase, RaidPhase::Lobby);
    ready(&mut app, a);
    ready(&mut app, b);
    steps(&mut app, 390);
    assert_eq!(app.world().resource::<RaidState>().phase, RaidPhase::Active);
    assert_eq!(app.world().resource::<RaidState>().raid_id, 2);
    let body = app.world().get::<BodyHealth>(b).unwrap();
    assert_eq!(body.anatomy.missing, 0);
    assert_eq!(body.anatomy.wounds, 0);
    assert_eq!(body.anatomy.organs, [100.0; 4]);
    assert_eq!(body.anatomy.muscle, [100.0; 7]);
    assert_eq!(body.anatomy.blood, 100.0);
    assert_eq!(
        app.world().get::<PoseState>(b).unwrap().mode,
        PhysicalMode::Standing
    );
    assert_eq!(
        app.world_mut()
            .query::<&physical::PhysicsPiece>()
            .iter(app.world())
            .count(),
        0
    );
    assert_eq!(
        app.world_mut()
            .query::<&Projectile>()
            .iter(app.world())
            .count(),
        0
    );
    assert!(app.world().resource::<WorldState>().impact_marks.is_empty());
    assert!(
        app.world()
            .resource::<WorldState>()
            .fired_projectiles
            .is_empty()
    );
}
