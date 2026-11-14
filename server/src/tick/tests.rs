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
