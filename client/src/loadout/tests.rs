use super::*;
use bevy_renet2::prelude::RenetServer;
use common::{
    ClientData, EquipmentSlot,
    catalog::{DefinitionId, WeaponId, catalog},
    combat::{Arsenal, Loadout},
    replication::*,
};

fn fixture() -> (App, Arsenal, ConfigurationPublisher) {
    let arsenal = Arsenal::new(Loadout::default());
    let fingerprint = catalog_fingerprint(common::catalog::CATALOG_SOURCE);
    let mut publisher = ConfigurationPublisher::new(1, fingerprint).unwrap();
    let mut session = ClientSession::default();
    session.configurations.cache.begin_epoch(1).unwrap();
    for slot in [EquipmentSlot::Primary, EquipmentSlot::Sidearm] {
        publisher
            .register(7, slot, arsenal.equipped(slot).unwrap().instance())
            .unwrap();
    }
    for packet in publisher.entries() {
        session
            .configurations
            .cache
            .apply(catalog(), fingerprint, packet.clone())
            .unwrap();
    }
    session.players.push(ClientData { id: 7, ..default() });
    let mut client = RenetClient::new(data::renet_config(), false);
    client.set_connected();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(session)
        .insert_resource(PlayerId(7))
        .insert_resource(client)
        .insert_resource(preview::PreviewImage(Handle::default()))
        .init_resource::<Editor>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_systems(Startup, ui::setup)
        .add_systems(Update, (synchronize, interact).chain());
    app.update();
    app.world_mut().resource_mut::<Editor>().visible = true;
    (app, arsenal, publisher)
}

fn press(app: &mut App, action: ui::Action) {
    let button = app
        .world_mut()
        .query::<(Entity, &ui::Action)>()
        .iter(app.world())
        .find(|(_, current)| **current == action)
        .unwrap()
        .0;
    app.world_mut().entity_mut(button).insert(bevy::ui::Pressed);
    app.update();
    app.world_mut()
        .entity_mut(button)
        .remove::<bevy::ui::Pressed>();
    app.update();
}

#[test]
fn lobby_panel_clicks_submit_compact_kit_and_wait_for_confirmed_preview() {
    let (mut app, mut arsenal, mut publisher) = fixture();
    for row in [0, 1, 2, 3] {
        press(&mut app, ui::Action::Part(row));
    }
    press(&mut app, ui::Action::Position(1)); // CTR position5 ->0
    press(&mut app, ui::Action::Reserve(0)); // 30 ->20
    let editor = app.world().resource::<Editor>();
    let draft = editor.draft.as_ref().unwrap();
    let spec = draft.resolve(catalog()).unwrap();
    assert_eq!(spec.magazine_size, 20);
    assert_eq!(
        draft.reserves,
        [DefinitionId(120), DefinitionId(101), DefinitionId(101)]
    );
    assert_eq!(
        app.world()
            .resource::<ClientSession>()
            .configurations
            .cache
            .latest(7, EquipmentSlot::Primary)
            .unwrap()
            .resolved
            .magazine_size,
        30,
        "draft never replaces the confirmed preview"
    );
    press(&mut app, ui::Action::Apply);
    assert_eq!(app.world().resource::<Editor>().pending, Some(1));
    let mut server = RenetServer::new(data::renet_config());
    server.add_connection(7, false);
    for packet in app
        .world_mut()
        .resource_mut::<RenetClient>()
        .get_packets_to_send()
    {
        server.process_packet_from(packet, 7).unwrap();
    }
    let bytes = server.receive_message(7, CONFIGURATION_CHANNEL).unwrap();
    let command = ConfigurationCommand::decode(&bytes).unwrap();
    let result = CommandLedger::default().execute(1, command, true, &mut arsenal, catalog());
    assert_eq!(result.outcome, CommandOutcome::Applied);
    app.world_mut()
        .resource_mut::<ClientSession>()
        .configurations
        .last_result = Some(result);
    app.update();
    assert_eq!(
        app.world().resource::<Editor>().pending,
        Some(1),
        "result alone is not a published assembly"
    );
    publisher
        .register(7, EquipmentSlot::Primary, arsenal.current().instance())
        .unwrap();
    let packet = publisher.entries().last().unwrap().clone();
    app.world_mut()
        .resource_mut::<ClientSession>()
        .configurations
        .cache
        .apply(catalog(), packet.catalog, packet)
        .unwrap();
    app.update();
    assert_eq!(app.world().resource::<Editor>().pending, None);
    let session = app.world().resource::<ClientSession>();
    let accepted = session
        .configurations
        .cache
        .latest(7, EquipmentSlot::Primary)
        .unwrap();
    assert_eq!(accepted.resolved.magazine_size, 20);
    assert_eq!(
        accepted
            .packet
            .magazines
            .iter()
            .map(|m| m.definition)
            .collect::<Vec<_>>(),
        [
            DefinitionId(120),
            DefinitionId(120),
            DefinitionId(101),
            DefinitionId(101)
        ]
    );
}

#[test]
fn lobby_panel_reports_dependencies_removes_subtrees_and_enforces_ready_lock() {
    let (mut app, _, _) = fixture();
    press(&mut app, ui::Action::Part(7)); // RC2, missing mount
    let error = app
        .world()
        .resource::<Editor>()
        .draft
        .as_ref()
        .unwrap()
        .resolve(catalog())
        .unwrap_err();
    assert!(!error.message.is_empty());
    press(&mut app, ui::Action::Apply);
    assert!(app.world().resource::<Editor>().pending.is_none());
    assert!(
        app.world()
            .resource::<Editor>()
            .status
            .contains(&error.message)
    );
    press(&mut app, ui::Action::Part(4)); // SFMB
    press(&mut app, ui::Action::Part(8)); // QRP2
    press(&mut app, ui::Action::Part(9)); // PRO
    press(&mut app, ui::Action::Part(10)); // RVG
    let kit = app.world().resource::<Editor>().draft.as_ref().unwrap();
    kit.resolve(catalog()).unwrap();
    assert!(
        kit.weapon
            .components
            .iter()
            .any(|p| p.definition == DefinitionId(133))
    );
    press(&mut app, ui::Action::Part(8)); // remove mount AND optic
    let kit = app.world().resource::<Editor>().draft.as_ref().unwrap();
    assert!(
        kit.weapon
            .components
            .iter()
            .all(|p| ![9, 10].contains(&p.node))
    );
    kit.resolve(catalog()).unwrap();
    let before = kit.clone();
    app.world_mut().resource_mut::<ClientSession>().players[0]
        .participant
        .ready = true;
    press(&mut app, ui::Action::Part(0));
    press(&mut app, ui::Action::Apply);
    assert_eq!(
        app.world().resource::<Editor>().draft.as_ref(),
        Some(&before)
    );
    assert!(app.world().resource::<Editor>().pending.is_none());
    let session = app.world().resource::<ClientSession>();
    assert_eq!(
        session
            .configurations
            .cache
            .latest(7, EquipmentSlot::Primary)
            .unwrap()
            .packet
            .weapon,
        catalog().configuration(WeaponId::M4A1).unwrap()
    );
}
