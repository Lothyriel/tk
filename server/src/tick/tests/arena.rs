use super::*;

const PLAYER: u64 = 7;

fn human(app: &App) -> Entity {
    app.world().resource::<Lobby>().players[&PLAYER]
}

fn bot(app: &mut App) -> Entity {
    app.world_mut()
        .query_filtered::<Entity, With<Bot>>()
        .single(app.world())
        .expect("exactly one Arena bot")
}

fn select_arena(app: &mut App, player: Entity) {
    app.world_mut().get_mut::<ClientInput>(player).unwrap().mode = MatchMode::ArenaSolo;
}

fn kill(app: &mut App, entity: Entity) {
    app.world_mut().get_mut::<BodyHealth>(entity).unwrap().parts[1] = 0.0;
}

fn assert_active_solo(app: &mut App) {
    let raid = app.world().resource::<RaidState>();
    assert_eq!(raid.mode, MatchMode::ArenaSolo);
    assert_eq!(raid.phase, RaidPhase::Active);
    assert_eq!(raid.loot_available, [false; 3]);
    assert_eq!(
        app.world_mut().query::<&Client>().iter(app.world()).count(),
        2
    );
}

#[test]
fn arena_solo_replaces_both_bodies_restocking_confirmed_kit_and_reengaging() {
    let mut app = app();
    let loadout = Loadout {
        armor: ArmorKind::Heavy,
        ..default()
    };
    let player = add_player(&mut app, PLAYER, loadout);
    select_arena(&mut app, player);
    let catalog = common::catalog::catalog();
    let mut kit = app.world().get::<Arsenal>(player).unwrap().kits()[0]
        .1
        .clone();
    kit.weapon.components[0].node = 40;
    kit.weapon.components.sort_by_key(|part| part.node);
    app.world_mut()
        .get_mut::<Arsenal>(player)
        .unwrap()
        .select_kit(catalog, EquipmentSlot::Primary, 0, kit.clone())
        .unwrap();
    ready(&mut app, player);
    steps(&mut app, 390);
    assert_active_solo(&mut app);
    let raid_id = app.world().resource::<RaidState>().raid_id;

    // A delayed ready/cancel intent cannot accidentally leave an active Arena.
    ready(&mut app, player);
    steps(&mut app, 2);
    assert_active_solo(&mut app);

    for generation in 1..=2 {
        let (player, opponent) = (human(&app), bot(&mut app));
        // Consume ammo and alter the unconfirmed lobby intent before death.
        let before = app
            .world()
            .get::<Arsenal>(player)
            .unwrap()
            .current()
            .rounds();
        {
            let mut input = app.world_mut().get_mut::<ClientInput>(player).unwrap();
            input.loadout = Loadout {
                primary: WeaponId::MP5,
                ..default()
            };
            input.fire_pressed_sequence += 1;
            input.fire = true;
        }
        held_steps(&mut app, player, 4);
        assert!(
            app.world()
                .get::<Arsenal>(player)
                .unwrap()
                .current()
                .rounds()
                < before
        );

        app.world_mut().get_mut::<ClientInput>(player).unwrap().fire = false;
        let before = app.world().resource::<WorldState>().next_projectile_id;
        steps(&mut app, 340);
        assert!(
            app.world().resource::<WorldState>().next_projectile_id > before,
            "Arena bot sees and fires at the live player"
        );
        assert!(
            app.world().get::<BodyHealth>(player).unwrap().total() < BodyHealth::default().total(),
            "existing bot weapon and projectiles damage the player"
        );

        // Simultaneous deaths exercise both independent timers without allowing
        // the old corpse or weapon state to become the newly active actor.
        kill(&mut app, player);
        kill(&mut app, opponent);
        steps(&mut app, 1);
        assert_eq!(app.world().resource::<RaidState>().seconds_left, 3.0);
        steps(&mut app, 383);
        assert_eq!(human(&app), player);
        assert_eq!(bot(&mut app), opponent);
        for actor in [player, opponent] {
            assert_eq!(
                app.world().get::<Participant>(actor).unwrap().status,
                RaidStatus::Dead
            );
        }
        assert!(
            app.world_mut()
                .query::<&physical::PhysicsPiece>()
                .iter(app.world())
                .count()
                > 0
        );
        steps(&mut app, 1);
        assert_active_solo(&mut app);
        let (new_player, new_bot) = (human(&app), bot(&mut app));
        assert!(app.world().get_entity(player).is_err());
        assert!(app.world().get_entity(opponent).is_err());
        assert_eq!(app.world().resource::<RaidState>().seconds_left, 0.0);
        assert_eq!(app.world().resource::<RaidState>().raid_id, raid_id);
        assert_eq!(
            app.world_mut()
                .query::<&physical::PhysicsPiece>()
                .iter(app.world())
                .count(),
            0
        );
        for (actor, expected) in [
            (new_player, super::super::arena::PLAYER_SPAWNS[0]),
            (new_bot, super::super::arena::BOT_SPAWNS[0]),
        ] {
            assert_eq!(
                app.world()
                    .get::<super::super::arena::Life>(actor)
                    .unwrap()
                    .generation,
                generation
            );
            assert_eq!(
                app.world().get::<Transform>(actor).unwrap().translation,
                expected
            );
            assert_eq!(
                app.world().get::<BodyHealth>(actor).unwrap().total(),
                BodyHealth::default().total()
            );
            assert_eq!(
                app.world().get::<Participant>(actor).unwrap().status,
                RaidStatus::Active
            );
        }
        let arsenal = app.world().get::<Arsenal>(new_player).unwrap();
        assert_eq!(arsenal.loadout, loadout);
        assert_eq!(arsenal.kits()[0].1, kit);
        assert_eq!(
            arsenal.current().rounds(),
            arsenal.current().spec().magazine_size
        );
    }

    let player = human(&app);
    kill(&mut app, player);
    steps(&mut app, 2);
    app.world_mut()
        .get_mut::<ClientInput>(player)
        .unwrap()
        .arena_leave = true;
    ready(&mut app, player);
    steps(&mut app, 2);
    assert_eq!(app.world().resource::<RaidState>().phase, RaidPhase::Lobby);
    assert!(
        app.world_mut()
            .query::<&Bot>()
            .iter(app.world())
            .next()
            .is_none()
    );
    assert!(
        app.world_mut()
            .query::<&physical::PhysicsPiece>()
            .iter(app.world())
            .next()
            .is_none()
    );
    app.world_mut()
        .get_mut::<ClientInput>(player)
        .unwrap()
        .arena_leave = false;
    ready(&mut app, player);
    steps(&mut app, 390);
    assert_active_solo(&mut app);
    assert_eq!(app.world().resource::<RaidState>().raid_id, raid_id + 1);
}

#[test]
fn arena_solo_respawn_uses_clear_fallback_and_never_adds_raid_sentinels() {
    let mut app = app();
    let player = add_player(&mut app, PLAYER, Loadout::default());
    select_arena(&mut app, player);
    ready(&mut app, player);
    steps(&mut app, 390);
    assert_active_solo(&mut app);
    let opponent = bot(&mut app);
    let spawns = super::super::arena::BOT_SPAWNS;
    app.world_mut()
        .get_mut::<Transform>(player)
        .unwrap()
        .translation = spawns[0];
    kill(&mut app, opponent);
    steps(&mut app, 385);
    assert_active_solo(&mut app);
    let next = bot(&mut app);
    assert_ne!(next, opponent);
    assert_eq!(
        app.world().get::<Transform>(next).unwrap().translation,
        spawns[1]
    );
}

#[test]
fn arena_solo_bot_can_eliminate_a_player_and_target_the_replacement() {
    let mut app = app();
    let player = add_player(&mut app, PLAYER, Loadout::default());
    select_arena(&mut app, player);
    ready(&mut app, player);
    steps(&mut app, 390);
    let opponent = bot(&mut app);
    for _ in 0..(128 * 24) {
        if app.world().get::<Participant>(player).unwrap().status == RaidStatus::Dead {
            break;
        }
        steps(&mut app, 1);
    }
    assert_eq!(
        app.world().get::<Participant>(player).unwrap().status,
        RaidStatus::Dead,
        "production schedule and bot should cause a real elimination"
    );
    steps(&mut app, 384);
    let next = human(&app);
    assert_ne!(next, player);
    assert_eq!(bot(&mut app), opponent);
    for _ in 0..(128 * 8) {
        if app.world().get::<BodyHealth>(next).unwrap().total() < BodyHealth::default().total() {
            return;
        }
        steps(&mut app, 1);
    }
    panic!("surviving bot did not damage the replacement player");
}

#[test]
fn arena_pvp_ignores_flight_already_in_progress_when_the_target_respawns() {
    let mut app = app();
    let victim = add_player(&mut app, PLAYER, Loadout::default());
    let shooter = add_player(&mut app, 8, Loadout::default());
    for player in [victim, shooter] {
        app.world_mut().get_mut::<ClientInput>(player).unwrap().mode = MatchMode::ArenaPvp;
        ready(&mut app, player);
    }
    steps(&mut app, 390);
    assert_eq!(
        app.world().resource::<RaidState>().mode,
        MatchMode::ArenaPvp
    );
    kill(&mut app, victim);
    steps(&mut app, 384);
    assert_eq!(human(&app), victim);
    let next_id = app.world().resource::<WorldState>().next_projectile_id;
    // A projectile already in flight when the next body arrives. Keep it
    // outside the hit volume until the authoritative body exists.
    let spawn = super::super::arena::PLAYER_SPAWNS[0];
    let inject = |app: &mut App, id, position, velocity| {
        app.world_mut()
            .spawn((
                Projectile {
                    id,
                    weapon: WeaponId::M4A1,
                    velocity,
                    damage: 250.0,
                    penetration: 200.0,
                    lifetime: 1.0,
                    owner_entity: shooter,
                    penetrated: Vec::new(),
                },
                Transform::from_translation(position),
            ))
            .id()
    };
    let origin = spawn + Vec3::new(0.0, 0.3, 2.0);
    let stale = inject(&mut app, next_id, origin, Vec3::ZERO);
    app.world_mut()
        .resource_mut::<WorldState>()
        .next_projectile_id += 1;
    steps(&mut app, 1);
    let replacement = human(&app);
    assert_ne!(replacement, victim);
    app.world_mut()
        .get_mut::<Projectile>(stale)
        .unwrap()
        .velocity = -Vec3::Z * 600.0;
    steps(&mut app, 1);
    assert_eq!(
        app.world().get::<BodyHealth>(replacement).unwrap().parts,
        BodyHealth::default().parts,
    );

    // Same shot path, but this ID was authorized after the replacement.
    inject(&mut app, next_id + 1, origin, -Vec3::Z * 600.0);
    steps(&mut app, 1);
    assert!(
        app.world().get::<BodyHealth>(replacement).unwrap().total() < BodyHealth::default().total(),
        "the replacement must still take damage from fresh projectiles"
    );
}
