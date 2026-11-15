use super::*;
mod configured_anatomy;
mod weapon_replication;

#[test]
fn catalog_input_rejects_unknown_definition_without_substituting_a_weapon() {
    let input = ClientInput {
        loadout: Loadout {
            primary: WeaponId(99999),
            ..default()
        },
        ..default()
    };
    assert!(super::validated_input(&data::encode(&input)).is_none());
    let input = ClientInput {
        loadout: Loadout {
            primary: WeaponId::MP5,
            ..default()
        },
        weapon: EquipmentSlot::Sidearm,
        ..default()
    };
    let decoded = super::validated_input(&data::encode(&input)).unwrap();
    assert_eq!(decoded.loadout.primary, WeaponId::MP5);
    assert_eq!(decoded.weapon, EquipmentSlot::Sidearm);
}
use bevy::{time::TimeUpdateStrategy, transform::TransformPlugin};
use std::time::Duration;

#[test]
fn assembly_authority_rejects_invalid_changes_and_fires_from_cached_properties() {
    use common::catalog::{CATALOG_SOURCE, Catalog, DefinitionId, WeaponConfiguration};
    let (mut app, player, _) = physical_fixture();
    // Controlled registry fixture: configuration packets contain only node IDs
    // and placements. Capacity/velocity must come from this server's registry.
    let source = CATALOG_SOURCE
        .replace("capacity: 30", "capacity: 40")
        .replace(
            "occupied_positions: 1, geometry: (",
            "occupied_positions: 1, modifiers: (velocity: 1.2), geometry: (",
        );
    let catalog = Catalog::parse("server-fixture.ron", &source).unwrap();
    // Physical magazines are created from this registry, never resized by a
    // subsequent configuration request.
    let arsenal = Arsenal::from_slots(
        &catalog,
        Loadout::default(),
        &[
            (EquipmentSlot::Primary, WeaponId::M4A1),
            (EquipmentSlot::Sidearm, WeaponId::M1911),
        ],
    )
    .unwrap();
    app.world_mut().entity_mut(player).insert(arsenal);
    let mut configuration = catalog.configuration(WeaponId::M4A1).unwrap();
    configuration.components[0].node = 2;
    let received: WeaponConfiguration = data::decode(&data::encode(&configuration));
    {
        let mut arsenal = app.world_mut().get_mut::<Arsenal>(player).unwrap();
        let before = data::encode(arsenal.current().instance());
        let mut invalid = received.clone();
        invalid.components[0].definition = DefinitionId(u32::MAX);
        assert!(
            arsenal
                .reconfigure(&catalog, EquipmentSlot::Primary, 0, invalid)
                .is_err()
        );
        assert_eq!(data::encode(arsenal.current().instance()), before);
        assert_eq!(
            arsenal
                .reconfigure(&catalog, EquipmentSlot::Primary, 0, received.clone())
                .unwrap(),
            1
        );
        assert_eq!(arsenal.current().spec().magazine_size, 40);
        assert!((arsenal.current().spec().muzzle_speed - 858.0).abs() < 0.001);
        assert_eq!(
            (arsenal.current().rounds(), arsenal.current().reserve()),
            (40, 120)
        );
    }
    app.world_mut().get_mut::<ClientInput>(player).unwrap().fire = true;
    held_steps(&mut app, player, 1);
    assert_eq!(
        app.world()
            .get::<Arsenal>(player)
            .unwrap()
            .current()
            .rounds(),
        39
    );
    assert!(
        app.world()
            .resource::<WorldState>()
            .fired_projectiles
            .iter()
            .any(|shot| shot.shooter == 7)
    );
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(player).unwrap();
        input.fire = false;
        input.reload_sequence += 1;
    }
    held_steps(&mut app, player, 1);
    let mut arsenal = app.world_mut().get_mut::<Arsenal>(player).unwrap();
    assert!(arsenal.reload_remaining > 0.0);
    assert!(
        arsenal
            .reconfigure(&catalog, EquipmentSlot::Primary, 1, received)
            .unwrap_err()
            .message
            .contains("reload")
    );
    assert_eq!(arsenal.current().instance().revision, 1);
}

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

#[test]
fn impact_mark_size_scales_with_authoritative_projectile_weapon() {
    assert_eq!(simulation::impact_mark_size_mm(WeaponId::M1911), 25);
    assert_eq!(simulation::impact_mark_size_mm(WeaponId::MP5), 35);
    assert_eq!(simulation::impact_mark_size_mm(WeaponId::M4A1), 50);

    let mut world = WorldState {
        next_mark_id: u32::MAX - 1,
        ..default()
    };
    for _ in 0..258 {
        world.impact(Vec3::ZERO, Vec3::Y, 25);
    }
    let mut retained_ids: Vec<_> = world.impact_marks.iter().map(|mark| mark.id).collect();
    retained_ids.sort_unstable();
    retained_ids.dedup();
    assert_eq!(retained_ids.len(), 256);
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
        primary: WeaponId::MP5,
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
    assert_eq!(
        app.world()
            .get::<Arsenal>(player)
            .unwrap()
            .current()
            .rounds(),
        25
    );
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(player).unwrap();
        input.loadout = Loadout::default();
        input.weapon = EquipmentSlot::Primary;
    }
    steps(&mut app, 2);
    assert_eq!(
        app.world().get::<Participant>(player).unwrap().loadout,
        loadout
    );
    assert_eq!(
        app.world()
            .get::<Arsenal>(player)
            .unwrap()
            .current()
            .spec()
            .definition,
        WeaponId::MP5
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
            weapon: WeaponId::M4A1,
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
    app.world_mut()
        .get_mut::<Arsenal>(a)
        .unwrap()
        .equipped_mut(EquipmentSlot::Primary)
        .unwrap()
        .set_ammunition(0, 90)
        .unwrap();
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
    app.world_mut().get_mut::<ClientInput>(a).unwrap().weapon = EquipmentSlot::Sidearm;
    held_steps(&mut app, a, 2);
    assert_eq!(app.world().get::<Arsenal>(a).unwrap().reload_remaining, 0.0);
    assert_eq!(
        app.world()
            .get::<Arsenal>(a)
            .unwrap()
            .equipped(EquipmentSlot::Primary)
            .unwrap()
            .rounds(),
        0
    );
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
    assert_eq!(
        app.world().get::<Arsenal>(b).unwrap().current().rounds(),
        29
    );
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
    assert_eq!(
        app.world().get::<Arsenal>(b).unwrap().current().rounds(),
        29
    );
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
fn mechanics_gates_cancel_before_and_after_magazine_commits_without_rollback() {
    use common::humanoid::{PhysicalMode, PoseState};
    for gate in ["stale", "fall", "arm", "death"] {
        for fraction in [0.24, 0.26, 0.69, 0.71, 0.91] {
            let (mut app, player, _) = physical_fixture();
            let (magazines, chamber, inserted, total) = {
                let mut arsenal = app.world_mut().get_mut::<Arsenal>(player).unwrap();
                let weapon = arsenal.equipped_mut(EquipmentSlot::Primary).unwrap();
                weapon.set_ammunition(0, 90).unwrap();
                assert!(weapon.mechanics_mut().start_reload(2.4));
                weapon.advance(2.4 * fraction);
                let state = weapon.mechanics();
                (
                    state.magazines().to_vec(),
                    state.chamber(),
                    state.inserted().map(|m| m.id),
                    state.total_rounds(),
                )
            };
            match gate {
                "stale" => {
                    app.world_mut()
                        .get_mut::<LastInput>(player)
                        .unwrap()
                        .received_at = -100.0
                }
                "fall" => {
                    let mut pose = app.world_mut().get_mut::<PoseState>(player).unwrap();
                    pose.mode = PhysicalMode::Recovering;
                    pose.mode_time = 0.0;
                }
                "arm" => {
                    app.world_mut()
                        .get_mut::<BodyHealth>(player)
                        .unwrap()
                        .anatomy
                        .regions[Region::RightArm as usize] = 0.0
                }
                "death" => {
                    app.world_mut()
                        .get_mut::<BodyHealth>(player)
                        .unwrap()
                        .anatomy
                        .organs[0] = 0.0
                }
                _ => unreachable!(),
            }
            if gate == "stale" {
                steps(&mut app, 2);
            } else {
                held_steps(&mut app, player, 2);
            }
            let state = app
                .world()
                .get::<Arsenal>(player)
                .unwrap()
                .current()
                .mechanics();
            assert!(state.action().is_none(), "{gate} at {fraction}");
            assert_eq!(state.magazines(), magazines);
            assert_eq!(state.chamber(), chamber);
            assert_eq!(state.inserted().map(|m| m.id), inserted);
            assert_eq!(state.total_rounds(), total);
            assert!(
                !app.world()
                    .resource::<WorldState>()
                    .fired_projectiles
                    .iter()
                    .any(|s| s.shooter == 7)
            );
        }
    }
}

#[test]
fn mechanics_injured_handling_uses_one_clock_for_commits_and_pose() {
    use common::humanoid::PoseState;
    let (mut app, healthy, injured) = physical_fixture();
    app.world_mut()
        .get_mut::<BodyHealth>(injured)
        .unwrap()
        .anatomy
        .regions[Region::LeftArm as usize] = 0.0;
    for player in [healthy, injured] {
        let mut arsenal = app.world_mut().get_mut::<Arsenal>(player).unwrap();
        let weapon = arsenal.equipped_mut(EquipmentSlot::Primary).unwrap();
        weapon.set_ammunition(0, 90).unwrap();
        assert!(weapon.mechanics_mut().start_reload(2.4));
    }
    for _ in 0..90 {
        let now = app.world().resource::<Time>().elapsed_secs();
        for player in [healthy, injured] {
            app.world_mut()
                .get_mut::<LastInput>(player)
                .unwrap()
                .received_at = now;
        }
        steps(&mut app, 1);
    }
    let healthy_state = app
        .world()
        .get::<Arsenal>(healthy)
        .unwrap()
        .current()
        .mechanics()
        .snapshot();
    let injured_state = app
        .world()
        .get::<Arsenal>(injured)
        .unwrap()
        .current()
        .mechanics()
        .snapshot();
    assert!(healthy_state.fraction > injured_state.fraction);
    assert!(!healthy_state.magazine_inserted);
    assert!(injured_state.magazine_inserted);
    assert!((injured_state.fraction / healthy_state.fraction - 0.725).abs() < 0.01);
    for (player, state) in [(healthy, healthy_state), (injured, injured_state)] {
        let pose = app.world().get::<PoseState>(player).unwrap();
        assert!(pose.reload_empty);
        assert!((pose.reload_fraction - state.fraction).abs() < 0.01);
        assert_eq!(
            app.world()
                .get::<Arsenal>(player)
                .unwrap()
                .current()
                .mechanics()
                .total_rounds(),
            90
        );
    }
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
            weapon: WeaponId::M4A1,
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
    // Approved fixture correction: debris no longer holds the injured actor
    // standing. Aim the ordinary shot at its actual head, including during fall.
    let root = *app.world().get::<Transform>(b).unwrap();
    let rig = evaluate_pose(app.world().get::<PoseState>(b).unwrap(), 0.0, 1.0);
    let head = root.transform_point(rig[Segment::Head.index()].position.into());
    normal_shot(&mut app, a, head);
    let old_magazines = {
        let mut arsenal = app.world_mut().get_mut::<Arsenal>(a).unwrap();
        let weapon = arsenal.equipped_mut(EquipmentSlot::Primary).unwrap();
        assert!(weapon.mechanics_mut().start_reload(2.4));
        weapon.advance(0.7);
        assert!(weapon.mechanics().inserted().is_none());
        weapon
            .mechanics()
            .magazines()
            .iter()
            .map(|m| m.id)
            .collect::<Vec<_>>()
    };
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
    let mechanics = app.world().get::<Arsenal>(a).unwrap().current().mechanics();
    assert!(mechanics.action().is_none());
    assert!(mechanics.equipped() && mechanics.chamber().is_some());
    assert_eq!(mechanics.loaded_rounds(), 30);
    assert_eq!(mechanics.total_rounds(), 120);
    assert!(
        mechanics
            .magazines()
            .iter()
            .all(|m| !old_magazines.contains(&m.id))
    );
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

#[test]
fn stance_contact_survives_actual_running_braking_and_turning() {
    use common::humanoid::*;
    let (mut app, _, b) = physical_fixture();
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(b).unwrap();
        input.forward = true;
        input.run = true;
    }
    held_steps(&mut app, b, 50);
    let mut checked = 0;
    for _ in 0..60 {
        let before = app.world().get::<PoseState>(b).unwrap().clone();
        let root = *app.world().get::<Transform>(b).unwrap();
        let i = if before.foot_weight[0] > 0.95 { 0 } else { 1 };
        let foot = root.transform_point(before.feet[i].into());
        held_steps(&mut app, b, 1);
        let after = app.world().get::<PoseState>(b).unwrap();
        if before.foot_weight[i] > 0.95 && after.foot_weight[i] > 0.95 {
            let root = app.world().get::<Transform>(b).unwrap();
            assert!(
                foot.distance(root.transform_point(after.feet[i].into())) < 0.003,
                "stance foot slid"
            );
            checked += 1;
        }
    }
    assert!(checked > 10, "must exercise real planted running contacts");
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(b).unwrap();
        input.forward = false;
        input.run = false;
        input.camera.yaw = 0.6;
    }
    held_steps(&mut app, b, 6);
    let pose = app.world().get::<PoseState>(b).unwrap();
    assert!(pose.turn_rate.abs() > 0.1);
    assert!(
        pose.acceleration[1] > 0.1,
        "braking must have opposite acceleration from forward travel"
    );
    held_steps(&mut app, b, 120);
    assert!(
        app.world()
            .get::<PoseState>(b)
            .unwrap()
            .foot_weight
            .iter()
            .all(|w| *w > 0.95)
    );
}

#[test]
fn ragdoll_running_and_resting_contact_matrix_settles_without_floor_escape_or_joint_explosion() {
    use common::humanoid::*;
    let mut final_positions = Vec::new();
    for running in [false, true] {
        for obstacle in 0..4 {
            let (mut app, _, b) = physical_fixture();
            if running {
                {
                    let mut input = app.world_mut().get_mut::<ClientInput>(b).unwrap();
                    input.forward = true;
                    input.run = true;
                }
                held_steps(&mut app, b, 45);
            }
            let start = app.world().get::<Transform>(b).unwrap().translation;
            if obstacle == 1 || obstacle == 2 {
                app.world_mut().spawn((
                    Collider::cuboid(1.5, 1.5, 0.12),
                    SurfaceKind::Concrete,
                    Transform::from_xyz(start.x, 1.6, start.z - 1.2),
                ));
            }
            if obstacle == 2 {
                app.world_mut().spawn((
                    Collider::cuboid(0.12, 1.5, 1.4),
                    SurfaceKind::Concrete,
                    Transform::from_xyz(start.x + 0.7, 1.6, start.z - 1.1),
                ));
            }
            if obstacle == 3 {
                app.world_mut().spawn((
                    Collider::cuboid(1.0, 0.12, 0.4),
                    SurfaceKind::Concrete,
                    Transform::from_xyz(start.x, 0.22, start.z - 0.9),
                ));
            }
            let motion = app.world().get::<ragdoll::Motion>(b).unwrap();
            assert!(motion.valid);
            if running {
                assert!(
                    motion.linear[Segment::Pelvis.index()].z < -4.0,
                    "actual run momentum must be captured before control loss"
                );
                assert!(
                    motion.angular[Segment::LeftThigh.index()].length() > 0.1,
                    "articulated leg motion must not be discarded"
                );
            }
            app.world_mut().get_mut::<ragdoll::Impact>(b).unwrap().add(
                Vec3::new(if obstacle % 2 == 0 { 3.0 } else { -3.0 }, 0.0, -1.0),
                start + Vec3::Y * 0.4,
            );
            app.world_mut().get_mut::<BodyHealth>(b).unwrap().parts[0] = 0.0;
            steps(&mut app, 128 * 8);
            let pieces: Vec<_> = app
                .world_mut()
                .query::<(&physical::PhysicsPiece, &Transform, &Velocity)>()
                .iter(app.world())
                .filter(|(piece, _, _)| piece.owner == 8 && !piece.detached)
                .map(|(piece, t, v)| (piece.segment, *t, *v))
                .collect();
            assert_eq!(pieces.len(), SEGMENT_COUNT);
            let mut centre = Vec3::ZERO;
            let mut total_mass = 0.0;
            let mut max_speed = 0.0_f32;
            let mut max_gap = 0.0_f32;
            for (segment, t, v) in &pieces {
                assert!(t.translation.is_finite() && t.rotation.is_normalized());
                let half = segment.half_size() * 0.88;
                let extent = (t.rotation * Vec3::X).y.abs() * half.x
                    + (t.rotation * Vec3::Y).y.abs() * half.y
                    + (t.rotation * Vec3::Z).y.abs() * half.z;
                assert!(
                    t.translation.y - extent > 0.065,
                    "floor escape {running}/{obstacle}/{segment:?}: {:?}",
                    t.translation
                );
                max_speed = max_speed.max(v.linear.length());
                assert!(
                    v.angular.length() < 1.0,
                    "persistent angular jitter {running}/{obstacle}/{segment:?}: {:?}",
                    v.angular
                );
                if let Some(parent) = segment.parent() {
                    let p = pieces.iter().find(|(s, _, _)| *s == parent).unwrap();
                    max_gap = max_gap.max(t.translation.distance(p.1.translation));
                }
                let mass = ragdoll::mass(*segment);
                centre += t.translation * mass;
                total_mass += mass;
            }
            centre /= total_mass;
            assert!(
                max_speed < 0.15,
                "body did not rest {running}/{obstacle}: {max_speed}"
            );
            assert!(
                max_gap < 0.75,
                "joint explosion {running}/{obstacle}: {max_gap}"
            );
            assert!(centre.distance(start) < 5.0, "unphysical solver launch");
            println!(
                "RAGDOLL MATRIX running={running} obstacle={obstacle} start={start:?} rest={centre:?} max_speed={max_speed:.4} max_gap={max_gap:.4}"
            );
            final_positions.push((running, obstacle, start, centre));
            // A single quiet frame can be a turning point in an oscillation.
            // Keep the same limits throughout a further two seconds at 128 Hz.
            for _ in 0..256 {
                steps(&mut app, 1);
                let mut count = 0;
                for (piece, transform, velocity) in app
                    .world_mut()
                    .query::<(&physical::PhysicsPiece, &Transform, &Velocity)>()
                    .iter(app.world())
                {
                    if piece.owner != 8 || piece.detached {
                        continue;
                    }
                    assert!(
                        velocity.linear.length() < 0.15 && velocity.angular.length() < 1.0,
                        "late oscillation {running}/{obstacle}/{:?}: {velocity:?}",
                        piece.segment
                    );
                    let rest = pieces
                        .iter()
                        .find(|(s, _, _)| *s == piece.segment)
                        .unwrap()
                        .1;
                    assert!(
                        transform.translation.distance(rest.translation) < 0.05,
                        "late drift {running}/{obstacle}/{:?}",
                        piece.segment
                    );
                    count += 1;
                }
                assert_eq!(count, SEGMENT_COUNT);
            }
        }
    }
    let rest = &final_positions[0];
    let run = &final_positions[4];
    assert!(
        run.2.z - run.3.z > rest.2.z - rest.3.z + 0.4,
        "run must continue farther along actual travel than rest"
    );
    let wall = &final_positions[5];
    assert!(
        (run.3.z - run.2.z - (wall.3.z - wall.2.z)).abs() > 0.15,
        "wall must change the running trajectory"
    );
}

#[test]
fn fatal_hit_materializes_the_body_before_the_same_tick_snapshot() {
    use common::humanoid::*;
    let (mut app, a, b) = physical_fixture();
    let root = *app.world().get::<Transform>(b).unwrap();
    let rig = evaluate_pose(app.world().get::<PoseState>(b).unwrap(), 1.0, 1.0);
    let head = root.transform_point(rig[Segment::Head.index()].position.into());
    app.world_mut().spawn((
        Projectile {
            id: 991,
            weapon: WeaponId::M4A1,
            velocity: -Vec3::X * 200.0,
            damage: 100.0,
            penetration: 100.0,
            lifetime: 3.0,
            owner_entity: a,
            penetrated: Vec::new(),
        },
        Transform::from_translation(head + Vec3::X),
    ));
    app.world_mut()
        .resource_mut::<WorldState>()
        .snapshot_elapsed = 1.0 / 30.0;
    steps(&mut app, 1);
    assert!(!app.world().get::<BodyHealth>(b).unwrap().alive());
    assert!(
        app.world().get::<physical::Corpsed>(b).is_some(),
        "fatal snapshot must include replacement, not a missing-body frame"
    );
    assert_eq!(
        app.world_mut()
            .query::<&physical::PhysicsPiece>()
            .iter(app.world())
            .filter(|p| p.owner == 8)
            .count(),
        SEGMENT_COUNT
    );
    assert_eq!(
        app.world().get::<PoseState>(b).unwrap().mode,
        PhysicalMode::Dead
    );
}

#[test]
fn live_articulated_fall_retains_ids_hit_volumes_and_pose_until_supported_getup() {
    use common::humanoid::*;
    let (mut app, _, b) = physical_fixture();
    app.world_mut().get_mut::<PoseState>(b).unwrap().impulse = [3.0, 0.0, 0.0];
    steps(&mut app, 1);
    assert!(app.world().get::<live_body::Body>(b).is_some());
    let mut ids: Vec<_> = app
        .world_mut()
        .query::<&physical::PhysicsPiece>()
        .iter(app.world())
        .filter(|p| p.owner == 8)
        .map(|p| p.id)
        .collect();
    ids.sort_unstable();
    assert_eq!(ids.len(), SEGMENT_COUNT);
    let mut recovering_frames = 0;
    let mut previous: Option<Vec<(u64, Transform)>> = None;
    for _ in 0..450 {
        let pose = app.world().get::<PoseState>(b).unwrap();
        let root = *app.world().get::<Transform>(b).unwrap();
        if pose.mode == PhysicalMode::Standing {
            break;
        }
        assert!(
            pose.physical.is_some(),
            "fall and get-up must use the actual articulated rig"
        );
        let rig = evaluate_pose(pose, 1.0, 1.0);
        let pieces: Vec<_> = app
            .world_mut()
            .query::<(&physical::PhysicsPiece, &Transform)>()
            .iter(app.world())
            .filter(|(p, _)| p.owner == 8 && !p.detached)
            .map(|(p, t)| (p.id, p.segment, *t))
            .collect();
        let mut current: Vec<_> = pieces.iter().map(|(id, _, _)| *id).collect();
        current.sort_unstable();
        assert_eq!(current, ids, "get-up cannot replace physical segment IDs");
        for (_, segment, t) in &pieces {
            let expected = root.mul_transform(rig[segment.index()].transform());
            assert!(
                t.translation.distance(expected.translation) < 0.0011,
                "hit volume detached from physical segment {segment:?}"
            );
            assert!(t.rotation.angle_between(expected.rotation) < 0.02);
        }
        if app.world().get::<PoseState>(b).unwrap().mode == PhysicalMode::Recovering {
            recovering_frames += 1;
            if let Some(previous) = &previous {
                for (id, _, t) in &pieces {
                    let old = previous.iter().find(|(old_id, _)| old_id == id).unwrap().1;
                    assert!(
                        t.translation.distance(old.translation) < 0.065,
                        "get-up pose teleport"
                    );
                }
            }
            for (_, segment, t) in &pieces {
                let half = segment.half_size() * 0.88;
                let extent = (t.rotation * Vec3::X).y.abs() * half.x
                    + (t.rotation * Vec3::Y).y.abs() * half.y
                    + (t.rotation * Vec3::Z).y.abs() * half.z;
                assert!(
                    t.translation.y - extent > 0.065,
                    "get-up crossed floor: {segment:?}"
                );
            }
        }
        previous = Some(pieces.iter().map(|(id, _, t)| (*id, *t)).collect());
        steps(&mut app, 1);
    }
    assert!(
        recovering_frames > 60,
        "must observe the actual get-up transition"
    );
    assert_eq!(
        app.world().get::<PoseState>(b).unwrap().mode,
        PhysicalMode::Standing
    );
    assert!(app.world().get::<PoseState>(b).unwrap().physical.is_none());
    assert!(app.world().get::<live_body::Body>(b).is_none());
    assert_eq!(
        app.world_mut()
            .query::<&physical::PhysicsPiece>()
            .iter(app.world())
            .filter(|p| p.owner == 8)
            .count(),
        0
    );
    assert!(app.world().get::<Transform>(b).unwrap().translation.y > 0.98);
}

#[test]
fn death_during_live_getup_preserves_the_existing_segments_and_returns_them_to_dynamics() {
    use common::humanoid::*;
    let (mut app, _, b) = physical_fixture();
    app.world_mut().get_mut::<PoseState>(b).unwrap().impulse = [3.0, 0.0, 0.0];
    for _ in 0..450 {
        steps(&mut app, 1);
        let pose = app.world().get::<PoseState>(b).unwrap();
        if pose.mode == PhysicalMode::Recovering && pose.mode_time > 0.3 {
            break;
        }
    }
    assert_eq!(
        app.world().get::<PoseState>(b).unwrap().mode,
        PhysicalMode::Recovering
    );
    let mut before: Vec<_> = app
        .world_mut()
        .query::<&physical::PhysicsPiece>()
        .iter(app.world())
        .filter(|p| p.owner == 8)
        .map(|p| p.id)
        .collect();
    before.sort_unstable();
    app.world_mut().get_mut::<BodyHealth>(b).unwrap().parts[0] = 0.0;
    steps(&mut app, 2);
    assert_eq!(
        app.world().get::<PoseState>(b).unwrap().mode,
        PhysicalMode::Dead
    );
    let mut after = Vec::new();
    for (p, rigid) in app
        .world_mut()
        .query::<(&physical::PhysicsPiece, &RigidBody)>()
        .iter(app.world())
        .filter(|(p, _)| p.owner == 8)
    {
        after.push(p.id);
        assert_eq!(*rigid, RigidBody::Dynamic);
    }
    after.sort_unstable();
    assert_eq!(before, after);
    assert_eq!(after.len(), SEGMENT_COUNT);
}

#[test]
fn normal_separation_during_a_live_ragdoll_detaches_existing_ids_without_duplicates() {
    use common::humanoid::*;
    let (mut app, a, b) = physical_fixture();
    app.world_mut().get_mut::<PoseState>(b).unwrap().impulse = [3.0, 0.0, 0.0];
    steps(&mut app, 35);
    assert!(app.world().get::<live_body::Body>(b).is_some());
    let original: Vec<_> = app
        .world_mut()
        .query::<&physical::PhysicsPiece>()
        .iter(app.world())
        .filter(|p| p.owner == 8)
        .map(|p| p.id)
        .collect();
    for _ in 0..3 {
        let root = *app.world().get::<Transform>(b).unwrap();
        let rig = evaluate_pose(app.world().get::<PoseState>(b).unwrap(), 1.0, 1.0);
        let target = root.transform_point(rig[Segment::LeftShin.index()].position.into());
        // Arrange an exposed view of the articulated shin, not a ray through a
        // different limb. The actual projectile still uses normal aim/damage.
        let eye = [
            Vec3::X,
            -Vec3::X,
            Vec3::Z,
            -Vec3::Z,
            Vec3::new(-1.0, 0.0, 1.0),
            Vec3::new(-1.0, 0.0, -1.0),
        ]
        .into_iter()
        .map(|v| {
            let mut eye = target + v.normalize() * 3.0;
            // Keep the shooter grounded; an elevated teleport makes it fall
            // and correctly blocks the second normal trigger.
            eye.y = 1.018 + PLAYER_VIEW_HEIGHT;
            eye
        })
        .find(|eye| {
            let mut rotation = Transform::from_translation(*eye)
                .looking_at(target, Vec3::Y)
                .rotation;
            for _ in 0..8 {
                let muzzle = *eye + rotation * Vec3::from(WeaponKind::Rifle.spec().barrel_offset);
                rotation = Transform::from_translation(muzzle)
                    .looking_at(target, Vec3::Y)
                    .rotation;
            }
            let muzzle = *eye + rotation * Vec3::from(WeaponKind::Rifle.spec().barrel_offset);
            ray_segments(
                root.rotation.inverse() * (muzzle - root.translation),
                root.rotation.inverse() * (rotation * -Vec3::Z),
                5.0,
                &rig,
                0,
            )
            .first()
            .is_some_and(|hit| hit.0 == Segment::LeftShin)
        })
        .expect("fixture must expose the intended shin rather than an occluding limb");
        app.world_mut().get_mut::<Transform>(a).unwrap().translation =
            eye - Vec3::Y * PLAYER_VIEW_HEIGHT;
        normal_shot(&mut app, a, target);
    }
    assert_ne!(
        app.world().get::<BodyHealth>(b).unwrap().anatomy.missing
            & (1 << Segment::LeftShin.index()),
        0
    );
    let pieces: Vec<_> = app
        .world_mut()
        .query::<&physical::PhysicsPiece>()
        .iter(app.world())
        .filter(|p| p.owner == 8)
        .map(|p| (p.id, p.detached))
        .collect();
    assert_eq!(pieces.len(), SEGMENT_COUNT);
    assert!(pieces.iter().all(|(id, _)| original.contains(id)));
    assert_eq!(pieces.iter().filter(|(_, detached)| *detached).count(), 2);
    let body = app.world().get::<live_body::Body>(b).unwrap();
    assert!(
        body.parts[Segment::LeftShin.index()].is_none()
            && body.parts[Segment::LeftFoot.index()].is_none()
    );
}

#[test]
fn unsupported_articulated_body_cannot_begin_getup_when_the_timer_expires() {
    use common::humanoid::*;
    let (mut app, _, b) = physical_fixture();
    app.world_mut().get_mut::<PoseState>(b).unwrap().impulse = [3.0, 0.0, 0.0];
    steps(&mut app, 35);
    let environment: Vec<_> = app
        .world_mut()
        .query_filtered::<Entity, With<SurfaceKind>>()
        .iter(app.world())
        .collect();
    for entity in environment {
        app.world_mut().entity_mut(entity).despawn();
    }
    steps(&mut app, 256);
    let pose = app.world().get::<PoseState>(b).unwrap();
    assert!(
        pose.mode_time > 1.8,
        "must cross the recovery eligibility timer"
    );
    assert_eq!(pose.mode, PhysicalMode::Fallen);
    assert!(pose.physical.is_some());
    assert!(!app.world().get::<MovementState>(b).unwrap().grounded);
    assert!(app.world().get::<BodyHealth>(b).unwrap().alive());
    assert_eq!(
        app.world()
            .get::<live_body::Body>(b)
            .unwrap()
            .parts
            .iter()
            .flatten()
            .count(),
        SEGMENT_COUNT
    );
}

#[test]
fn surviving_getup_by_a_wall_keeps_the_articulated_body_on_the_clear_side() {
    use common::humanoid::*;
    let (mut app, _, b) = physical_fixture();
    let start = app.world().get::<Transform>(b).unwrap().translation;
    let wall_face = start.x + 0.63;
    app.world_mut().spawn((
        SurfaceKind::Concrete,
        Collider::cuboid(0.12, 1.5, 2.0),
        Transform::from_xyz(wall_face + 0.12, 1.6, start.z),
    ));
    steps(&mut app, 2);
    app.world_mut().get_mut::<PoseState>(b).unwrap().impulse = [3.0, 0.0, 0.0];
    let mut recovery_frames = 0;
    for _ in 0..650 {
        steps(&mut app, 1);
        let mode = app.world().get::<PoseState>(b).unwrap().mode;
        if mode == PhysicalMode::Recovering {
            recovery_frames += 1;
            for (piece, t) in app
                .world_mut()
                .query::<(&physical::PhysicsPiece, &Transform)>()
                .iter(app.world())
            {
                if piece.owner != 8 || piece.detached {
                    continue;
                }
                let half = piece.segment.half_size() * 0.88;
                let extent = (t.rotation * Vec3::X).x.abs() * half.x
                    + (t.rotation * Vec3::Y).x.abs() * half.y
                    + (t.rotation * Vec3::Z).x.abs() * half.z;
                assert!(
                    t.translation.x + extent < wall_face + 0.02,
                    "get-up crossed wall: {:?}",
                    piece.segment
                );
            }
        }
        if recovery_frames > 0 && mode == PhysicalMode::Standing {
            break;
        }
    }
    assert!(recovery_frames > 60);
    assert_eq!(
        app.world().get::<PoseState>(b).unwrap().mode,
        PhysicalMode::Standing
    );
    assert!(
        app.world().get::<Transform>(b).unwrap().translation.x < wall_face - PLAYER_COLLIDER_RADIUS
    );
}

#[test]
fn crouched_fall_begins_at_the_captured_bent_pose_without_a_solver_launch() {
    use common::humanoid::*;
    let (mut app, _, b) = physical_fixture();
    app.world_mut().get_mut::<ClientInput>(b).unwrap().crouch = true;
    held_steps(&mut app, b, 30);
    let captured = app.world().get::<ragdoll::Motion>(b).unwrap().world;
    assert!(app.world().get::<MovementState>(b).unwrap().crouched);
    app.world_mut().get_mut::<PoseState>(b).unwrap().impulse = [3.0, 0.0, 0.0];
    steps(&mut app, 1);
    assert!(app.world().get::<live_body::Body>(b).is_some());
    let mut count = 0;
    for (piece, t) in app
        .world_mut()
        .query::<(&physical::PhysicsPiece, &Transform)>()
        .iter(app.world())
    {
        if piece.owner != 8 {
            continue;
        }
        assert!(
            t.translation
                .distance(captured[piece.segment.index()].translation)
                < 0.065,
            "crouched handoff jumped: {:?}",
            piece.segment
        );
        count += 1;
    }
    assert_eq!(count, SEGMENT_COUNT);
}

#[test]
fn walking_character_ignores_fragment_support_and_keeps_its_ground_height() {
    let (mut app, _, b) = physical_fixture();
    let root = app.world().get::<Transform>(b).unwrap().translation;
    // A loose boot under the next step must not be a KCC autostep/ground hit.
    app.world_mut().spawn((
        RigidBody::Dynamic,
        Collider::cuboid(0.16, 0.12, 0.18),
        CollisionGroups::new(Group::GROUP_2, Group::GROUP_1 | Group::GROUP_2),
        Transform::from_translation(Vec3::new(root.x, 0.22, root.z - 0.45)),
    ));
    app.world_mut().get_mut::<ClientInput>(b).unwrap().forward = true;
    for _ in 0..40 {
        held_steps(&mut app, b, 1);
        let current = app.world().get::<Transform>(b).unwrap().translation;
        assert!(
            (current.y - root.y).abs() < 0.02,
            "debris changed character ground height: {current:?}"
        );
        assert!(
            app.world().get::<ragdoll::Motion>(b).unwrap().linear[0]
                .y
                .abs()
                < 2.0,
            "debris injected an upward ragdoll handoff velocity"
        );
    }
    assert!(root.z - app.world().get::<Transform>(b).unwrap().translation.z > 0.5);
}

#[test]
fn stationary_half_turn_replants_both_feet_without_leaving_crossed_shins() {
    use common::humanoid::*;
    let (mut app, _, b) = physical_fixture();
    app.world_mut()
        .get_mut::<ClientInput>(b)
        .unwrap()
        .camera
        .yaw = std::f32::consts::PI;
    held_steps(&mut app, b, 64);
    let pose = app.world().get::<PoseState>(b).unwrap();
    assert!(pose.foot_weight.iter().all(|w| *w > 0.995));
    let rig = evaluate_pose(pose, 1.0, 1.0);
    assert!(rig[Segment::LeftFoot.index()].position[0] < -0.05);
    assert!(rig[Segment::RightFoot.index()].position[0] > 0.05);
    for segment in [Segment::LeftShin, Segment::RightShin] {
        let target = Vec3::from(rig[segment.index()].position);
        let eye = Vec3::new(0.0, 0.55, -4.0);
        let hits = ray_segments(eye, (target - eye).normalize(), 5.0, &rig, 0);
        assert_eq!(
            hits.first().unwrap().0,
            segment,
            "pivot occluded the intended shin"
        );
    }
}

#[test]
fn authoritative_ads_preserves_ammo_and_cadence_centres_barrel_and_cancels_for_reload_and_fall() {
    use common::humanoid::*;
    let (mut app, a, _) = physical_fixture();
    app.world_mut().get_mut::<ClientInput>(a).unwrap().aim = true;
    held_steps(&mut app, a, 64);
    assert!(app.world().get::<PoseState>(a).unwrap().ads > 0.99);
    assert_eq!(
        app.world().get::<Arsenal>(a).unwrap().current().rounds(),
        30
    );
    app.world_mut().get_mut::<ClientInput>(a).unwrap().fire = true;
    held_steps(&mut app, a, 1);
    app.world_mut().get_mut::<ClientInput>(a).unwrap().fire = false;
    let pose = app.world().get::<PoseState>(a).unwrap();
    let root = app.world().get::<Transform>(a).unwrap();
    let expected = root.translation
        + Vec3::Y * PLAYER_VIEW_HEIGHT
        + Quat::from(&app.world().get::<ClientInput>(a).unwrap().camera)
            * common::weapon_animation::barrel_offset(WeaponKind::Rifle, pose.ads);
    let shot = app
        .world()
        .resource::<WorldState>()
        .fired_projectiles
        .last()
        .unwrap();
    assert!(Vec3::from(shot.muzzle).distance(expected) < 0.001);
    assert_eq!(
        app.world().get::<Arsenal>(a).unwrap().current().rounds(),
        29
    );
    app.world_mut()
        .get_mut::<ClientInput>(a)
        .unwrap()
        .reload_sequence += 1;
    held_steps(&mut app, a, 64);
    assert!(app.world().get::<PoseState>(a).unwrap().ads < 0.01);
    assert!(app.world().get::<Arsenal>(a).unwrap().reload_remaining > 0.0);
    assert_eq!(
        app.world().get::<Arsenal>(a).unwrap().current().rounds(),
        29
    );
    held_steps(&mut app, a, 330);
    assert!(app.world().get::<PoseState>(a).unwrap().ads > 0.99);
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(a).unwrap();
        input.run = true;
        input.forward = true;
    }
    held_steps(&mut app, a, 64);
    assert!(
        app.world().get::<PoseState>(a).unwrap().ads < 0.01,
        "sprint must lower the sights"
    );
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(a).unwrap();
        input.run = false;
        input.forward = false;
    }
    held_steps(&mut app, a, 90);
    assert!(app.world().get::<PoseState>(a).unwrap().ads > 0.99);
    app.world_mut().get_mut::<PoseState>(a).unwrap().impulse = [3.0, 0.0, 0.0];
    held_steps(&mut app, a, 80);
    assert_eq!(
        app.world().get::<PoseState>(a).unwrap().mode,
        PhysicalMode::Fallen
    );
    assert!(app.world().get::<PoseState>(a).unwrap().ads < 0.01);
}

#[test]
fn lean_moves_the_authoritative_eye_and_muzzle_sideways() {
    let (mut app, shooter, _) = physical_fixture();
    let root = *app.world().get::<Transform>(shooter).unwrap();
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(shooter).unwrap();
        input.camera.roll = 0.3;
        input.fire = true;
        input.fire_pressed_sequence += 1;
    }

    held_steps(&mut app, shooter, 1);

    let input = app.world().get::<ClientInput>(shooter).unwrap();
    let pose = app
        .world()
        .get::<common::humanoid::PoseState>(shooter)
        .unwrap();
    let expected_eye = root.translation
        + root.rotation * common::humanoid::aim_lean_offset(input.camera.roll)
        + Vec3::Y * PLAYER_VIEW_HEIGHT;
    let expected_muzzle = expected_eye
        + Quat::from(&input.camera)
            * common::weapon_animation::barrel_offset(WeaponKind::Rifle, pose.ads);
    let shot = app
        .world()
        .resource::<WorldState>()
        .fired_projectiles
        .last()
        .unwrap();

    assert!((pose.aim_roll - 0.3).abs() < 0.001);
    assert!(Vec3::from(shot.muzzle).distance(expected_muzzle) < 0.001);
}

#[test]
fn walking_and_running_pose_cadence_stays_in_a_human_step_range() {
    let walking_steps = common::humanoid::stride_phase_rate(3.25, 0.0) / std::f32::consts::PI;
    let running_steps = common::humanoid::stride_phase_rate(5.5, 0.0) / std::f32::consts::PI;
    assert!((2.0..3.2).contains(&walking_steps));
    assert!((3.0..4.3).contains(&running_steps));
    assert_eq!(common::humanoid::stride_phase_rate(0.0, 0.0), 0.0);
}

#[test]
fn ads_barrel_beyond_a_concrete_panel_cannot_fire_through_the_panel() {
    let (mut app, a, b) = physical_fixture();
    let root = app.world().get::<Transform>(a).unwrap().translation;
    app.world_mut().spawn((
        SurfaceKind::Concrete,
        Collider::cuboid(1.0, 1.5, 0.1),
        Transform::from_xyz(root.x, 1.6, root.z - 0.60),
    ));
    app.world_mut().get_mut::<ClientInput>(a).unwrap().aim = true;
    held_steps(&mut app, a, 64);
    app.world_mut().get_mut::<ClientInput>(a).unwrap().fire = true;
    held_steps(&mut app, a, 1);
    app.world_mut().get_mut::<ClientInput>(a).unwrap().fire = false;
    held_steps(&mut app, a, 20);
    assert_eq!(
        app.world().get::<BodyHealth>(b).unwrap().anatomy.regions,
        REGION_MAX
    );
    assert!(
        app.world()
            .resource::<WorldState>()
            .impact_marks
            .iter()
            .any(|m| m.pos[2] > root.z - 0.8)
    );
    assert_eq!(
        app.world().get::<Arsenal>(a).unwrap().current().rounds(),
        29
    );
}

// ANIM-15: solve the specified muzzle ray, not an eye-origin approximation.
fn aim_offset_ray(eye: Vec3, target: Vec3, barrel: Vec3) -> CameraInput {
    let d = target - eye;
    let h = (d.length_squared() - barrel.x * barrel.x - barrel.y * barrel.y).sqrt();
    let pitch = (d.y / (h * h + barrel.y * barrel.y).sqrt())
        .clamp(-1.0, 1.0)
        .asin()
        - barrel.y.atan2(h);
    let local_z = barrel.y * pitch.sin() - h * pitch.cos();
    CameraInput {
        pitch,
        yaw: (-d.x).atan2(-d.z) + barrel.x.atan2(-local_z),
        roll: 0.0,
    }
}

#[test]
fn close_hip_and_ads_muzzle_rays_hit_actual_heads_without_capsule_redirection() {
    use common::humanoid::*;
    for weapon in [WeaponKind::Rifle, WeaponKind::Smg, WeaponKind::Pistol] {
        for aiming in [false, true] {
            for distance in [1.05, 1.25, 1.45, 2.0] {
                let (mut app, a, b) = physical_fixture();
                if weapon == WeaponKind::Smg {
                    let loadout = Loadout {
                        primary: weapon.definition(),
                        armor: ArmorKind::None,
                        helmet: false,
                        ..default()
                    };
                    *app.world_mut().get_mut::<Arsenal>(a).unwrap() = Arsenal::new(loadout);
                }
                let origin = app.world().get::<Transform>(a).unwrap().translation;
                app.world_mut().get_mut::<Transform>(b).unwrap().translation =
                    origin - Vec3::Z * distance;
                {
                    let mut input = app.world_mut().get_mut::<ClientInput>(a).unwrap();
                    input.weapon = if weapon == WeaponKind::Pistol {
                        EquipmentSlot::Sidearm
                    } else {
                        EquipmentSlot::Primary
                    };
                    input.aim = aiming;
                }
                for _ in 0..120 {
                    held_steps(&mut app, a, 1);
                    let arsenal = app.world().get::<Arsenal>(a).unwrap();
                    if arsenal.current().spec().class == weapon
                        && arsenal.current().mechanics().equipped()
                        && arsenal.current().mechanics().action().is_none()
                    {
                        break;
                    }
                }
                let arsenal = app.world().get::<Arsenal>(a).unwrap();
                assert_eq!(arsenal.current().spec().class, weapon);
                assert!(arsenal.current().mechanics().equipped());
                held_steps(&mut app, a, 64);
                let eye = app.world().get::<Transform>(a).unwrap().translation
                    + Vec3::Y * PLAYER_VIEW_HEIGHT;
                let root = *app.world().get::<Transform>(b).unwrap();
                let rig = evaluate_pose(app.world().get::<PoseState>(b).unwrap(), 1.0, 1.0);
                let target = root.transform_point(rig[Segment::Head.index()].position.into());
                let ads = app.world().get::<PoseState>(a).unwrap().ads;
                let barrel = common::weapon_animation::barrel_offset(weapon, ads);
                let camera = aim_offset_ray(eye, target, barrel);
                let expected = eye + Quat::from(&camera) * barrel;
                {
                    let mut input = app.world_mut().get_mut::<ClientInput>(a).unwrap();
                    input.camera = camera;
                    input.fire = true;
                    input.fire_pressed_sequence += 1;
                }
                held_steps(&mut app, a, 1);
                app.world_mut().get_mut::<ClientInput>(a).unwrap().fire = false;
                held_steps(&mut app, a, 12);
                let body = app.world().get::<BodyHealth>(b).unwrap();
                assert!(
                    body.anatomy.regions[Region::Head as usize] < REGION_MAX[Region::Head as usize],
                    "clear posed head missed: {weapon:?} ADS={aiming} distance={distance} body={:?}",
                    body.anatomy
                );
                assert!(
                    (body.anatomy.regions[Region::Head as usize]
                        - (REGION_MAX[Region::Head as usize] - weapon.spec().damage).max(0.0))
                    .abs()
                        < 0.01,
                    "one close shot must retain the specified damage: {weapon:?} ADS={aiming} distance={distance}"
                );
                assert_eq!(
                    app.world().get::<Arsenal>(a).unwrap().current().rounds(),
                    weapon.spec().magazine_size - 1
                );
                let shot = app
                    .world()
                    .resource::<WorldState>()
                    .fired_projectiles
                    .iter()
                    .find(|shot| shot.shooter == 7)
                    .unwrap();
                assert!(
                    Vec3::from(shot.muzzle).distance(expected) < 0.001,
                    "free muzzle anchor changed"
                );
            }
        }
    }
}

#[test]
fn muzzle_beyond_a_close_actor_cannot_skip_its_actual_anatomy_in_hip_or_ads() {
    use common::humanoid::*;
    for aiming in [false, true] {
        let (mut app, a, b) = physical_fixture();
        let origin = app.world().get::<Transform>(a).unwrap().translation;
        app.world_mut().get_mut::<Transform>(b).unwrap().translation = origin - Vec3::Z * 0.72;
        app.world_mut().get_mut::<ClientInput>(a).unwrap().aim = aiming;
        held_steps(&mut app, a, 64);
        let eye =
            app.world().get::<Transform>(a).unwrap().translation + Vec3::Y * PLAYER_VIEW_HEIGHT;
        let barrel = common::weapon_animation::barrel_offset(
            WeaponKind::Rifle,
            app.world().get::<PoseState>(a).unwrap().ads,
        );
        let root = *app.world().get::<Transform>(b).unwrap();
        let rig = evaluate_pose(app.world().get::<PoseState>(b).unwrap(), 1.0, 1.0);
        assert!(
            !ray_segments(
                root.rotation.inverse() * (eye - root.translation),
                root.rotation.inverse() * barrel.normalize(),
                barrel.length(),
                &rig,
                0
            )
            .is_empty(),
            "fixture must put actual anatomy, not merely a capsule, before the muzzle"
        );
        app.world_mut().get_mut::<ClientInput>(a).unwrap().fire = true;
        held_steps(&mut app, a, 1);
        app.world_mut().get_mut::<ClientInput>(a).unwrap().fire = false;
        held_steps(&mut app, a, 12);
        assert_ne!(
            app.world().get::<BodyHealth>(b).unwrap().anatomy.regions,
            REGION_MAX,
            "shot spawned beyond the close body: ADS={aiming}"
        );
        assert_eq!(
            app.world().get::<Arsenal>(a).unwrap().current().rounds(),
            29
        );
    }
}

#[test]
fn narrow_concrete_on_eye_to_muzzle_path_stops_hip_and_ads_without_parallel_eye_bypass() {
    for aiming in [false, true] {
        let (mut app, a, b) = physical_fixture();
        app.world_mut().get_mut::<ClientInput>(a).unwrap().aim = aiming;
        held_steps(&mut app, a, 64);
        let eye =
            app.world().get::<Transform>(a).unwrap().translation + Vec3::Y * PLAYER_VIEW_HEIGHT;
        let barrel = common::weapon_animation::barrel_offset(
            WeaponKind::Rifle,
            app.world()
                .get::<common::humanoid::PoseState>(a)
                .unwrap()
                .ads,
        );
        let centre = eye + barrel * 0.5;
        app.world_mut().spawn((
            SurfaceKind::Concrete,
            Collider::cuboid(0.04, 0.015, 0.04),
            Transform::from_translation(centre),
        ));
        app.world_mut()
            .get_mut::<Transform>(b)
            .unwrap()
            .translation
            .x = eye.x + barrel.x;
        held_steps(&mut app, a, 3);
        app.world_mut().get_mut::<ClientInput>(a).unwrap().fire = true;
        held_steps(&mut app, a, 1);
        app.world_mut().get_mut::<ClientInput>(a).unwrap().fire = false;
        held_steps(&mut app, a, 12);
        assert_eq!(
            app.world().get::<BodyHealth>(b).unwrap().anatomy.regions,
            REGION_MAX,
            "narrow cover was bypassed: ADS={aiming}"
        );
        assert!(
            app.world()
                .resource::<WorldState>()
                .impact_marks
                .iter()
                .any(|mark| Vec3::from(mark.pos).distance(centre) < 0.10),
            "missing actual clearance impact"
        );
        assert_eq!(
            app.world().get::<Arsenal>(a).unwrap().current().rounds(),
            29
        );
    }
}

fn clearance_fixture() -> (App, Entity, Entity) {
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
    let mut participant = Participant::default();
    participant.begin();
    let target = app
        .world_mut()
        .spawn((
            BodyHealth::default(),
            ArmorState::default(),
            MovementState::default(),
            participant,
            Collider::capsule_y(PLAYER_COLLIDER_HALF_HEIGHT, PLAYER_COLLIDER_RADIUS),
            CollisionGroups::new(Group::GROUP_3, Group::ALL),
            Transform::from_xyz(0.0, 1.0, 4.0),
        ))
        .id();
    (app, owner, target)
}

#[test]
fn pre_muzzle_wood_attenuates_once_across_clearance_and_normal_flight() {
    let (mut app, owner, target) = clearance_fixture();
    app.world_mut().spawn((
        SurfaceKind::Wood,
        Collider::cuboid(1.0, 1.0, 0.12),
        Transform::from_xyz(0.0, 1.0, 2.0),
    ));
    steps(&mut app, 3);
    app.world_mut().spawn((
        Projectile {
            id: 991,
            weapon: WeaponId::M4A1,
            velocity: Vec3::Z * 100.0,
            damage: 50.0,
            penetration: 30.0,
            lifetime: 3.0,
            owner_entity: owner,
            penetrated: Vec::new(),
        },
        MuzzleClearance(Vec3::new(0.0, 1.0, 3.0)),
        Transform::from_xyz(0.0, 1.0, 0.0),
    ));
    steps(&mut app, 12);
    let health = app
        .world()
        .get::<BodyHealth>(target)
        .unwrap()
        .anatomy
        .regions[Region::Thorax as usize];
    // Same original 0.24m wood / 30 energy / 50damage contract: exactly42 damage.
    assert!(
        (health - 43.0).abs() < 0.1,
        "clearance charged wood twice or bypassed it: {health}"
    );
    assert_eq!(app.world().resource::<WorldState>().impact_marks.len(), 2);
    assert_eq!(
        app.world_mut()
            .query::<&Projectile>()
            .iter(app.world())
            .count(),
        0
    );
}

#[test]
fn pre_muzzle_ray_through_a_posed_leg_gap_does_not_hit_the_movement_capsule() {
    use common::humanoid::*;
    let (mut app, owner, target) = clearance_fixture();
    app.world_mut()
        .get_mut::<Transform>(target)
        .unwrap()
        .translation
        .z = 2.0;
    let start = Vec3::new(0.0, 0.45, 0.0);
    let rig = evaluate_pose(&PoseState::default(), 1.0, 1.0);
    assert!(ray_segments(start - Vec3::new(0.0, 1.0, 2.0), Vec3::Z, 4.0, &rig, 0).is_empty());
    steps(&mut app, 3);
    let shot = app
        .world_mut()
        .spawn((
            Projectile {
                id: 992,
                weapon: WeaponId::M4A1,
                velocity: Vec3::Z * 100.0,
                damage: 50.0,
                penetration: 30.0,
                lifetime: 3.0,
                owner_entity: owner,
                penetrated: Vec::new(),
            },
            MuzzleClearance(Vec3::new(0.0, 0.45, 3.0)),
            Transform::from_translation(start),
        ))
        .id();
    steps(&mut app, 1);
    assert!(
        app.world().get::<Transform>(shot).unwrap().translation.z > 3.5,
        "a clear gap must continue from its original muzzle, not the eye"
    );
    assert!(
        app.world().get::<MuzzleClearance>(shot).is_none(),
        "clearance must be consumed once"
    );
    steps(&mut app, 12);
    assert_eq!(
        app.world()
            .get::<BodyHealth>(target)
            .unwrap()
            .anatomy
            .regions,
        REGION_MAX
    );
    assert!(app.world().resource::<WorldState>().impact_marks.is_empty());
}

#[test]
fn muzzle_inside_the_same_wood_keeps_penetration_ids_across_both_paths() {
    let (mut app, owner, target) = clearance_fixture();
    app.world_mut().spawn((
        SurfaceKind::Wood,
        Collider::cuboid(1.0, 1.0, 0.12),
        Transform::from_xyz(0.0, 1.0, 2.0),
    ));
    steps(&mut app, 3);
    app.world_mut().spawn((
        Projectile {
            id: 993,
            weapon: WeaponId::M4A1,
            velocity: Vec3::Z * 100.0,
            damage: 50.0,
            penetration: 30.0,
            lifetime: 3.0,
            owner_entity: owner,
            penetrated: Vec::new(),
        },
        MuzzleClearance(Vec3::new(0.0, 1.0, 2.0)),
        Transform::from_xyz(0.0, 1.0, 0.0),
    ));
    steps(&mut app, 12);
    let health = app
        .world()
        .get::<BodyHealth>(target)
        .unwrap()
        .anatomy
        .regions[Region::Thorax as usize];
    assert!(
        (health - 43.0).abs() < 0.1,
        "same wood charged twice at path boundary: {health}"
    );
    assert_eq!(
        app.world().resource::<WorldState>().impact_marks.len(),
        2,
        "flight must not emit another entry/exit while starting inside penetrated wood"
    );
    assert_eq!(
        app.world_mut()
            .query::<&Projectile>()
            .iter(app.world())
            .count(),
        0
    );
}

fn voxel_shot(app: &mut App, owner: Entity, cell: &common::map::BarrierCell, damage: f32) {
    app.world_mut().spawn((
        Projectile {
            id: 9000,
            weapon: WeaponId::M4A1,
            velocity: -Vec3::Z * 100.0,
            damage,
            penetration: 30.0,
            lifetime: PROJECTILE_LIFETIME,
            owner_entity: owner,
            penetrated: Vec::new(),
        },
        Transform::from_translation(Vec3::from(cell.center) + Vec3::Z * 0.6),
    ));
}

fn voxel_entity(app: &mut App, id: u16) -> Entity {
    app.world_mut()
        .query::<(Entity, &BarrierVoxel)>()
        .iter(app.world())
        .find(|(_, voxel)| voxel.id == id)
        .unwrap()
        .0
}

#[test]
fn voxel_barriers_damage_neighbors_concrete_stopping_and_raid_reset() {
    let (mut app, owner, other) = physical_fixture();
    let cell = common::map::barrier_cells()
        .into_iter()
        .find(|c| c.id == 2)
        .unwrap();
    let entity = voxel_entity(&mut app, cell.id);
    let neighbor = voxel_entity(&mut app, cell.id - 1);
    for expected in [60.0, 20.0, 0.0] {
        voxel_shot(&mut app, owner, &cell, 40.0);
        steps(&mut app, 2);
        assert_eq!(
            app.world().get::<BarrierVoxel>(entity).unwrap().health,
            expected
        );
        assert_eq!(
            app.world().get::<BarrierVoxel>(neighbor).unwrap().health,
            100.0
        );
        assert_eq!(
            app.world_mut()
                .query::<&Projectile>()
                .iter(app.world())
                .count(),
            0,
            "even the shot destroying concrete must stop"
        );
    }
    assert!(app.world().get::<ColliderDisabled>(entity).is_some());
    assert_eq!(
        app.world().resource::<WorldState>().destroyed_cells,
        vec![cell.id]
    );
    steps(&mut app, 2);
    // A fresh shot crosses the removed cell and hits the next depth layer.
    voxel_shot(&mut app, owner, &cell, 40.0);
    steps(&mut app, 2);
    assert_eq!(
        app.world().get::<BarrierVoxel>(neighbor).unwrap().health,
        60.0
    );
    app.world_mut().resource_mut::<RaidState>().phase = RaidPhase::Debrief;
    ready(&mut app, owner);
    steps(&mut app, 3);
    assert_eq!(app.world().resource::<RaidState>().phase, RaidPhase::Lobby);
    assert!(
        app.world()
            .resource::<WorldState>()
            .destroyed_cells
            .is_empty()
    );
    assert_eq!(
        app.world().get::<BarrierVoxel>(entity).unwrap().health,
        100.0
    );
    assert_eq!(
        app.world().get::<BarrierVoxel>(neighbor).unwrap().health,
        100.0
    );
    assert!(app.world().get::<ColliderDisabled>(entity).is_none());
    // Start also repairs a damaged world independently of the lobby transition.
    app.world_mut()
        .get_mut::<BarrierVoxel>(entity)
        .unwrap()
        .health = 0.0;
    app.world_mut().entity_mut(entity).insert(ColliderDisabled);
    app.world_mut()
        .resource_mut::<WorldState>()
        .destroyed_cells
        .push(cell.id);
    ready(&mut app, owner);
    ready(&mut app, other);
    steps(&mut app, 390);
    assert_eq!(app.world().resource::<RaidState>().phase, RaidPhase::Active);
    assert_eq!(
        app.world().get::<BarrierVoxel>(entity).unwrap().health,
        100.0
    );
    assert!(app.world().get::<ColliderDisabled>(entity).is_none());
    assert!(
        app.world()
            .resource::<WorldState>()
            .destroyed_cells
            .is_empty()
    );
}

#[test]
fn voxel_barriers_wood_penetration_same_tick_holes_and_movement() {
    let (mut app, owner, _) = physical_fixture();
    let cells: Vec<_> = common::map::barrier_cells()
        .into_iter()
        .filter(|c| matches!(c.surface, SurfaceKind::Wood) && c.center[0] < 0.0)
        .collect();
    let cell = &cells[0];
    let entity = voxel_entity(&mut app, cell.id);
    voxel_shot(&mut app, owner, cell, 30.0);
    voxel_shot(&mut app, owner, cell, 30.0);
    steps(&mut app, 1);
    let mut energies: Vec<f32> = app
        .world_mut()
        .query::<&Projectile>()
        .iter(app.world())
        .map(|p| p.penetration)
        .collect();
    energies.sort_by(f32::total_cmp);
    assert_eq!(energies.len(), 2);
    assert!(
        (energies[0] - 25.2).abs() < 0.001,
        "wood must cost 4.8 penetration"
    );
    assert_eq!(
        energies[1], 30.0,
        "second shot in same tick must cross the hole freely"
    );
    assert_eq!(app.world().get::<BarrierVoxel>(entity).unwrap().health, 0.0);
    assert_eq!(
        app.world().resource::<WorldState>().destroyed_cells,
        vec![cell.id]
    );
    // Existing solid panel blocks the production character controller.
    app.world_mut()
        .get_mut::<Transform>(owner)
        .unwrap()
        .translation = Vec3::new(-7.0, 1.05, -3.5);
    app.world_mut()
        .get_mut::<ClientInput>(owner)
        .unwrap()
        .forward = true;
    held_steps(&mut app, owner, 70);
    assert!(app.world().get::<Transform>(owner).unwrap().translation.z > -4.6);
    for cell in &cells[1..] {
        voxel_shot(&mut app, owner, cell, 30.0);
    }
    steps(&mut app, 3);
    assert_eq!(
        app.world().resource::<WorldState>().destroyed_cells.len(),
        32
    );
    held_steps(&mut app, owner, 100);
    assert!(
        app.world().get::<Transform>(owner).unwrap().translation.z < -5.5,
        "character must pass through the cleared panel"
    );
}

#[test]
fn voxel_barriers_normal_weapon_input_breaks_wood_with_one_round() {
    let (mut app, owner, _) = physical_fixture();
    app.world_mut()
        .get_mut::<Transform>(owner)
        .unwrap()
        .translation = Vec3::new(-7.0, 1.05, -3.0);
    steps(&mut app, 3);
    let before = app
        .world()
        .get::<Arsenal>(owner)
        .unwrap()
        .current()
        .rounds();
    normal_shot(&mut app, owner, Vec3::new(-7.25, 1.35, -5.0));
    assert_eq!(
        app.world()
            .get::<Arsenal>(owner)
            .unwrap()
            .current()
            .rounds(),
        before - 1
    );
    let removed = &app.world().resource::<WorldState>().destroyed_cells;
    assert_eq!(removed.len(), 1);
    let id = removed[0];
    let entity = voxel_entity(&mut app, id);
    assert_eq!(app.world().get::<BarrierVoxel>(entity).unwrap().health, 0.0);
    assert!(app.world().get::<ColliderDisabled>(entity).is_some());
}

#[test]
fn voxel_barriers_production_snapshots_recover_loss_and_late_join() {
    let (mut app, owner, _) = physical_fixture();
    let cell = common::map::barrier_cells()
        .into_iter()
        .find(|c| c.id == 72)
        .unwrap();
    voxel_shot(&mut app, owner, &cell, 30.0);
    steps(&mut app, 3);
    let mut peer = bevy_renet2::prelude::RenetClient::new(data::renet_config(), false);
    peer.set_connected();
    app.world_mut()
        .resource_mut::<RenetServer>()
        .add_connection(999, false);
    // Join after destruction and discard a whole outgoing snapshot.
    steps(&mut app, 5);
    let _ = app
        .world_mut()
        .resource_mut::<RenetServer>()
        .get_packets_to_send(999)
        .unwrap();
    steps(&mut app, 5);
    {
        let mut server = app.world_mut().resource_mut::<RenetServer>();
        for packet in server.get_packets_to_send(999).unwrap() {
            peer.process_packet(packet);
        }
    }
    let bytes = peer
        .receive_message(DefaultChannel::Unreliable)
        .expect("production snapshot");
    let snapshot = data::decode::<WorldSnapshot>(&bytes);
    assert_eq!(
        common::map::destroyed_ids(&snapshot.destroyed_cells),
        vec![cell.id]
    );
    assert_eq!(PROTOCOL_ID, 11);
}
