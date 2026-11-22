use super::*;

/// A layout fixture, not a device test. Inject coordinates into Bevy's
/// pointer stream; the UI backend, not the test, chooses the click entity.
#[test]
fn bench_marker_coordinate_picking_respects_target_scale_and_overlay() {
    use bevy::{
        app::HierarchyPropagatePlugin,
        camera::{ComputedCameraValues, RenderTarget, RenderTargetInfo},
        picking::{
            InteractionPlugin, PickingPlugin,
            events::PointerClick,
            pointer::{Location, PointerAction, PointerButton, PointerId, PointerInput},
        },
        ui::{
            ComputedUiRenderTargetInfo, ComputedUiTargetCamera, UiGlobalTransform, UiStack,
            picking_backend::UiPickingPlugin, update::propagate_ui_target_cameras,
        },
        window::{PrimaryWindow, WindowRef},
    };

    #[derive(Resource, Default)]
    struct Clicks(Vec<Entity>);

    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        PickingPlugin,
        InteractionPlugin,
        UiPickingPlugin,
        HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate),
        HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate),
    ))
    .init_resource::<UiScale>()
    .init_resource::<UiStack>()
    .init_resource::<Clicks>()
    .add_systems(Update, propagate_ui_target_cameras)
    .add_observer(
        |event: On<PointerClick>, nodes: Query<(), With<Node>>, mut clicks: ResMut<Clicks>| {
            // Bevy can also deliver the click to the window backend; this
            // assertion is about the UI target, distinct from the window.
            if nodes.contains(event.entity) {
                clicks.0.push(event.entity);
            }
        },
    );

    let window = app
        .world_mut()
        .spawn((Window::default(), PrimaryWindow))
        .id();
    let target = RenderTarget::Window(WindowRef::Entity(window));
    let camera = app
        .world_mut()
        .spawn((
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::new(800, 600),
                        scale_factor: 2.0,
                    }),
                    ..default()
                },
                ..default()
            },
            target.clone(),
        ))
        .id();
    app.world_mut().spawn(PointerId::Mouse);

    let mut node = |size, pickable| {
        app.world_mut()
            .spawn((
                Node::default(),
                UiTargetCamera(camera),
                ComputedNode { size, ..default() },
                UiGlobalTransform::from_xy(320.0, 240.0),
                InheritedVisibility::VISIBLE,
                pickable,
            ))
            .id()
    };
    let preview = node(Vec2::splat(200.0), Pickable::default());
    let marker = node(Vec2::splat(42.0), Pickable::default());
    let label = node(Vec2::splat(20.0), Pickable::IGNORE);
    app.update();
    *app.world_mut().resource_mut::<UiStack>() = UiStack {
        uinodes: vec![preview, marker, label],
        partition: std::iter::once(0..3).collect(),
    };

    let location = |position| Location {
        target: target.normalize(Some(window)).unwrap(),
        position,
    };
    let click_at = |app: &mut App, position| {
        for action in [
            PointerAction::Move { delta: Vec2::ZERO },
            PointerAction::Press(PointerButton::Primary),
            PointerAction::Release(PointerButton::Primary),
        ] {
            app.world_mut().write_message(PointerInput::new(
                PointerId::Mouse,
                location(position),
                action,
            ));
            app.update();
        }
    };
    click_at(&mut app, Vec2::new(160.0, 120.0));
    click_at(&mut app, Vec2::new(180.0, 120.0));
    assert_eq!(app.world().resource::<Clicks>().0, [marker, preview]);
}

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
        .init_resource::<Editor>()
        .init_resource::<Assets<Image>>()
        .init_resource::<preview::Preview>()
        .init_resource::<crate::render::voxel::configured::VisualBindings>()
        .add_message::<crate::menu::Activation>()
        .add_systems(Startup, (preview::setup, ui::setup).chain())
        .add_systems(
            Update,
            (synchronize, interact, preview::update, ui::refresh).chain(),
        );
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
    app.world_mut()
        .write_message(crate::menu::Activation(button));
    app.update();
}

fn shown_part(app: &mut App, asset: &str) -> Option<Transform> {
    app.world_mut()
        .query::<(&crate::render::voxel::VoxelModel, &Transform)>()
        .iter(app.world())
        .find_map(|(part, pose)| (part.asset == asset).then_some(*pose))
}

fn preview_view(app: &App) -> Option<&preview::View> {
    app.world().resource::<preview::Preview>().view.as_ref()
}

#[test]
fn lobby_panel_clicks_preview_compact_kit_and_wait_for_confirmed_publication() {
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
        "draft preview never replaces the confirmed cache"
    );
    assert_eq!(preview_view(&app), Some(&preview::View::Draft));
    assert!(shown_part(&mut app, "m4a1_pmag_20").is_some());
    assert!(shown_part(&mut app, "m4a1_magazine_30").is_none());
    press(&mut app, ui::Action::Apply);
    press(&mut app, ui::Action::Apply);
    assert_eq!(app.world().resource::<Editor>().pending, Some(1));
    assert_eq!(preview_view(&app), Some(&preview::View::Pending));
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
    assert!(server.receive_message(7, CONFIGURATION_CHANNEL).is_none());
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
    assert_eq!(preview_view(&app), Some(&preview::View::Confirmed));
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
    assert!(
        matches!(preview_view(&app), Some(preview::View::Invalid(reason)) if reason.contains(&error.message))
    );
    assert!(shown_part(&mut app, "m4a1_rc2").is_none());
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
    assert!(shown_part(&mut app, "m4a1_pro").is_some());
    assert!(
        app.world()
            .resource::<preview::Preview>()
            .socket(9)
            .is_some()
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
    assert!(shown_part(&mut app, "m4a1_pro").is_none());
    assert!(
        app.world()
            .resource::<preview::Preview>()
            .socket(9)
            .is_none()
    );
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
    assert_eq!(preview_view(&app), Some(&preview::View::Locked));
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

#[test]
fn lobby_panel_rail_marker_tracks_preview_and_discard_restores_camera_and_kit() {
    let (mut app, _, _) = fixture();
    app.update();
    let socket = |app: &App| {
        app.world()
            .resource::<preview::Preview>()
            .socket(1)
            .unwrap()
    };
    let camera = |app: &mut App| {
        *app.world_mut()
            .query_filtered::<&Transform, With<preview::PreviewCamera>>()
            .single(app.world())
            .unwrap()
    };
    let baseline_camera = camera(&mut app);
    press(&mut app, ui::Action::Part(1));
    let before = socket(&app);
    let before_pose = shown_part(&mut app, "m4a1_ctr").unwrap();
    press(&mut app, ui::Action::Position(1));
    let after = socket(&app);
    let after_pose = shown_part(&mut app, "m4a1_ctr").unwrap();
    assert_ne!(before_pose.translation, after_pose.translation);
    assert_ne!(before, after);
    assert_eq!(camera(&mut app), baseline_camera);
    press(&mut app, ui::Action::Reset);
    app.update();
    assert_eq!(preview_view(&app), Some(&preview::View::Confirmed));
    assert!(shown_part(&mut app, "m4a1_ctr").is_none());
    assert!(shown_part(&mut app, "m4a1_stock").is_some());
    assert_eq!(camera(&mut app), baseline_camera);
}

#[test]
fn lobby_panel_server_rejection_restores_confirmed_assembly() {
    let (mut app, mut arsenal, _) = fixture();
    press(&mut app, ui::Action::Part(0));
    press(&mut app, ui::Action::Apply);
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
    let result = CommandLedger::default().execute(
        1,
        ConfigurationCommand::decode(&bytes).unwrap(),
        false,
        &mut arsenal,
        catalog(),
    );
    assert_eq!(result.outcome, CommandOutcome::Locked);
    app.world_mut()
        .resource_mut::<ClientSession>()
        .configurations
        .last_result = Some(result);
    app.update();
    assert_eq!(preview_view(&app), Some(&preview::View::Confirmed));
    assert_eq!(app.world().resource::<Editor>().status, "Locked");
    assert!(shown_part(&mut app, "m4a1_magazine_30").is_some());
    assert!(shown_part(&mut app, "m4a1_pmag_20").is_none());
}
