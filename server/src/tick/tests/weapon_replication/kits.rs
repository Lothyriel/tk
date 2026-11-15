use super::*;

fn request(app: &App, actor: Entity, peer: &Peer, sequence: u64) -> ConfigurationCommand {
    let weapon = app
        .world()
        .get::<Arsenal>(actor)
        .unwrap()
        .current()
        .instance();
    let mut config = catalog().configuration(WeaponId::M4A1).unwrap();
    config
        .components
        .iter_mut()
        .find(|p| p.node == 1)
        .unwrap()
        .definition = DefinitionId(120);
    ConfigurationCommand {
        epoch: peer.receiver.cache.epoch(),
        request: sequence,
        slot: EquipmentSlot::Primary,
        instance: weapon.id,
        expected_revision: weapon.revision,
        weapon: config,
        reserves: Some(vec![DefinitionId(101), DefinitionId(120)]),
    }
}

fn send(app: &mut App, peer: &mut Peer, command: &ConfigurationCommand) -> CommandOutcome {
    peer.network
        .send_message(CONFIGURATION_CHANNEL, data::encode(command));
    pump(app, &mut [peer], 40, true);
    peer.receiver.last_result.as_ref().unwrap().outcome
}

#[test]
fn lobby_kits_confirm_lock_restock_and_publish_to_late_join() {
    let mut app = app();
    let actor = add_player(&mut app, 7, Loadout::default());
    let mut peer = Peer::connect(&mut app, 7);
    pump(&mut app, &mut [&mut peer], 64, true);
    let command = request(&app, actor, &peer, 1);
    assert_eq!(send(&mut app, &mut peer, &command), CommandOutcome::Applied);
    let kit = app.world().get::<Arsenal>(actor).unwrap().kits().to_vec();
    let confirmed = data::encode(
        app.world()
            .get::<Arsenal>(actor)
            .unwrap()
            .current()
            .instance(),
    );
    assert_eq!(send(&mut app, &mut peer, &command), CommandOutcome::Applied);
    assert_eq!(
        data::encode(
            app.world()
                .get::<Arsenal>(actor)
                .unwrap()
                .current()
                .instance()
        ),
        confirmed
    );

    // A ready command is consumed before configuration commands in the same
    // production fixed tick; a delayed edit cannot bypass the countdown lock.
    app.world_mut()
        .get_mut::<ClientInput>(actor)
        .unwrap()
        .lobby_sequence = 1;
    let locked = request(&app, actor, &peer, 2);
    assert_eq!(send(&mut app, &mut peer, &locked), CommandOutcome::Locked);
    assert_eq!(
        app.world().resource::<RaidState>().phase,
        RaidPhase::Countdown
    );
    pump(&mut app, &mut [&mut peer], 400, true);
    assert_eq!(app.world().resource::<RaidState>().phase, RaidPhase::Active);
    let deployed = app.world().get::<Arsenal>(actor).unwrap();
    assert_eq!(deployed.kits(), kit);
    assert_eq!(
        (deployed.current().rounds(), deployed.current().reserve()),
        (20, 50)
    );
    let first_instance = deployed.current().instance().id;
    let first_magazines: Vec<_> = deployed
        .current()
        .mechanics()
        .magazines()
        .iter()
        .map(|m| m.id)
        .collect();
    let raid_edit = request(&app, actor, &peer, 1);
    assert_eq!(
        send(&mut app, &mut peer, &raid_edit),
        CommandOutcome::Locked
    );

    let mut late = Peer::connect(&mut app, 9);
    pump(&mut app, &mut [&mut peer, &mut late], 64, true);
    let snapshot = late.latest.as_ref().unwrap();
    let local = snapshot.players.iter().find(|p| p.id == 7).unwrap();
    let cached = late
        .receiver
        .cache
        .get(snapshot.configuration_epoch, local.configuration)
        .unwrap();
    assert_eq!(cached.packet.weapon, kit[0].1.weapon);
    assert_eq!(cached.resolved.magazine_size, 20);

    // Consume ammunition and change the inserted definition in the live raid.
    let mut arsenal = app.world_mut().get_mut::<Arsenal>(actor).unwrap();
    assert!(arsenal.equipped_mut(EquipmentSlot::Primary).unwrap().fire());
    let weapon = arsenal.equipped_mut(EquipmentSlot::Primary).unwrap();
    assert!(weapon.mechanics_mut().start_reload(2.0));
    weapon.advance(3.0);
    assert_eq!(weapon.spec().magazine_size, 30);
    app.world_mut().resource_mut::<RaidState>().phase = RaidPhase::Debrief;
    app.world_mut()
        .get_mut::<ClientInput>(actor)
        .unwrap()
        .lobby_sequence = 2;
    pump(&mut app, &mut [&mut peer, &mut late], 64, true);
    assert_eq!(app.world().resource::<RaidState>().phase, RaidPhase::Lobby);
    assert_eq!(
        app.world()
            .get::<Arsenal>(actor)
            .unwrap()
            .current()
            .rounds(),
        20
    );
    app.world_mut()
        .get_mut::<ClientInput>(actor)
        .unwrap()
        .lobby_sequence = 3;
    pump(&mut app, &mut [&mut peer, &mut late], 440, true);
    assert_eq!(app.world().resource::<RaidState>().phase, RaidPhase::Active);
    let second = app.world().get::<Arsenal>(actor).unwrap();
    assert_eq!(second.kits(), kit);
    assert_eq!(
        (second.current().rounds(), second.current().reserve()),
        (20, 50)
    );
    assert_ne!(second.current().instance().id, first_instance);
    assert!(
        second
            .current()
            .mechanics()
            .magazines()
            .iter()
            .all(|m| !first_magazines.contains(&m.id))
    );
    assert_eq!(
        second.equipped(EquipmentSlot::Sidearm).unwrap().rounds(),
        17
    );
}

#[test]
fn lobby_kits_primary_changes_preserve_sidearm_and_reject_forged_or_stale_edits() {
    let mut app = app();
    let actor = add_player(&mut app, 7, Loadout::default());
    let other = add_player(&mut app, 8, Loadout::default());
    let mut peer = Peer::connect(&mut app, 7);
    pump(&mut app, &mut [&mut peer], 64, true);
    let sidearm = app
        .world()
        .get::<Arsenal>(actor)
        .unwrap()
        .equipped(EquipmentSlot::Sidearm)
        .unwrap()
        .instance();
    let mut command = ConfigurationCommand {
        epoch: peer.receiver.cache.epoch(),
        request: 1,
        slot: EquipmentSlot::Sidearm,
        instance: sidearm.id,
        expected_revision: sidearm.revision,
        weapon: sidearm.configuration.clone(),
        reserves: Some(Vec::new()),
    };
    assert_eq!(send(&mut app, &mut peer, &command), CommandOutcome::Applied);
    assert_eq!(
        app.world()
            .get::<Arsenal>(actor)
            .unwrap()
            .equipped(EquipmentSlot::Sidearm)
            .unwrap()
            .reserve(),
        0
    );
    let selected = app.world().get::<Arsenal>(actor).unwrap().kits()[1].clone();
    command.request = 2;
    assert_eq!(
        send(&mut app, &mut peer, &command),
        CommandOutcome::StaleRevision
    );
    command.request = 3;
    command.instance = app
        .world()
        .get::<Arsenal>(other)
        .unwrap()
        .equipped(EquipmentSlot::Sidearm)
        .unwrap()
        .instance()
        .id;
    assert_eq!(
        send(&mut app, &mut peer, &command),
        CommandOutcome::NotOwned
    );
    for primary in [WeaponId::MP5, WeaponId::M4A1] {
        app.world_mut()
            .get_mut::<ClientInput>(actor)
            .unwrap()
            .loadout
            .primary = primary;
        pump(&mut app, &mut [&mut peer], 40, true);
        let arsenal = app.world().get::<Arsenal>(actor).unwrap();
        assert_eq!(arsenal.current().spec().definition, primary);
        assert_eq!(arsenal.kits()[1], selected);
        assert_eq!(
            arsenal.equipped(EquipmentSlot::Sidearm).unwrap().reserve(),
            0
        );
        assert!(
            !arsenal
                .equipped(EquipmentSlot::Sidearm)
                .unwrap()
                .mechanics()
                .equipped()
        );
    }
    assert_eq!(
        app.world()
            .get::<Arsenal>(other)
            .unwrap()
            .equipped(EquipmentSlot::Sidearm)
            .unwrap()
            .reserve(),
        51
    );
}
