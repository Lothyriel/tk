use bevy::{asset::AssetPlugin, prelude::*};

mod connection;
mod hud;
mod input;
mod loadout;
mod menu;
mod playtest;
mod preferences;
mod render;
mod sync;

fn main() {
    let playtest = std::env::args().any(|argument| argument == "--playtest");
    let grid_probe = std::env::args().any(|argument| argument == "--inventory-grid-probe");
    let arena_menu_probe = playtest::arena_menu::Probe::requested();
    let grid_pointer =
        std::env::args().any(|argument| argument == "--inventory-grid-pointer-checkpoint");
    let grid_inspect = std::env::args().any(|argument| argument == "--inventory-grid-inspect");
    let grid_review =
        grid_pointer || std::env::args().any(|argument| argument == "--inventory-grid-review");
    let voxel_grid = playtest && std::env::var_os("TK_PLAYTEST_INVENTORY_GRID").is_some();
    let close_ack = std::env::args().any(|arg| arg == "--inventory-grid-close-ack");
    assert!(
        !playtest::inventory_grid::partial_merge_enabled()
            || (voxel_grid
                && !grid_review
                && !close_ack
                && std::env::var("TK_PLAYTEST_CLIENTS").as_deref() == Ok("1")
                && [
                    "TK_PLAYTEST_GRID_SPLIT",
                    "TK_PLAYTEST_GRID_BENCH",
                    "TK_PLAYTEST_LOOT_INSPECT",
                    "TK_PLAYTEST_INVENTORY_TELEMETRY",
                    "TK_PLAYTEST_LOOT_CONTEST"
                ]
                .iter()
                .all(|flag| std::env::var_os(flag).is_none())),
        "partial merge requires the plain solo grid route"
    );
    assert!(
        !close_ack
            || (voxel_grid
                && !grid_review
                && std::env::var("TK_PLAYTEST_CLIENTS").as_deref() == Ok("1")
                && std::env::var_os("TK_PLAYTEST_INVENTORY_TELEMETRY").is_none()),
        "close/ACK diagnostic requires the plain solo grid route"
    );
    assert!(
        !grid_inspect || (grid_pointer && voxel_grid),
        "grid inspection requires the solo pointer review route"
    );
    assert!(
        !voxel_grid
            || (std::env::var_os("TK_PLAYTEST_VOXEL_LOOT").is_some()
                && std::env::var_os("TK_PLAYTEST_NO_CAPTURE").is_some()
                && std::env::var_os("TK_PLAYTEST_VIDEO").is_none()),
        "the combined grid route requires voxel loot with capture and video disabled"
    );
    assert!(
        !(playtest && arena_menu_probe.is_some()),
        "run the Arena menu probe without --playtest"
    );
    let item_review = std::env::args().any(|argument| argument == "--item-review");
    assert!(
        !grid_probe || !(playtest || item_review || arena_menu_probe.is_some()),
        "run the inventory grid probe without other playtest or review modes"
    );
    assert!(
        !grid_review
            || (!(grid_probe || item_review || arena_menu_probe.is_some())
                && (!playtest || voxel_grid)),
        "run grid review alone or with the no-video solo voxel grid route"
    );
    let viewport = (playtest
        || grid_review
        || (arena_menu_probe.is_some()
            && (std::env::var_os("TK_PREFERENCES_SCREENSHOT").is_some()
                || std::env::var_os("TK_LOBBY_REVIEW_SCALE").is_some())))
    .then(playtest::requested_viewport)
    .flatten();
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(AssetPlugin {
                file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../assets").into(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "TK | Extraction Alpha".into(),
                    resolution: viewport.map_or_else(
                        || (1280, 800).into(),
                        |(width, height)| {
                            bevy::window::WindowResolution::new(width, height)
                                .with_scale_factor_override(1.0)
                        },
                    ),
                    resizable: viewport.is_none(),
                    ..default()
                }),
                ..default()
            }),
    );
    if item_review {
        app.add_plugins(render::voxel::item_review::Plugin).run();
        return;
    }
    app.configure_sets(
        Update,
        (
            ClientSystems::Receive,
            ClientSystems::Input,
            ClientSystems::Send,
            ClientSystems::Present,
        )
            .chain(),
    )
    .add_plugins(ClientPlugin)
    .add_plugins(hud::Plugin);
    if playtest {
        app.add_plugins(playtest::Plugin);
    }
    if grid_probe || voxel_grid {
        app.add_plugins(playtest::inventory_grid::Plugin {
            integrated: voxel_grid,
        });
    }
    if let Some(probe) = arena_menu_probe {
        app.add_plugins(probe);
    }
    if close_ack {
        app.add_plugins(input::inventory_panel::CloseAckPlugin);
    }
    if grid_review {
        app.add_plugins(input::inventory_panel::GridReview {
            automated: voxel_grid,
            pointer_checkpoint: grid_pointer,
            inspect: grid_inspect,
        });
    }
    app.run();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
enum ClientSystems {
    Receive,
    Input,
    Send,
    Present,
}

struct ClientPlugin;

impl bevy::prelude::Plugin for ClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(common::Plugin)
            .add_plugins(connection::Plugin)
            .add_plugins(render::Plugin)
            .add_plugins(input::Plugin)
            .add_plugins(sync::Plugin)
            .add_plugins(loadout::Plugin)
            .add_plugins(menu::Plugin);
    }
}

#[cfg(test)]
mod startup_tests {
    use super::*;
    use bevy_renet2::{
        netcode::NetcodeClientTransport,
        prelude::{DefaultChannel, RenetClient, RenetServer},
    };
    use common::{catalog::*, data, replication::*, *};

    #[test]
    fn first_lobby_snapshot_runs_client_presentation_without_a_renderer() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            WindowPlugin::default(),
            TransformPlugin,
            bevy::input::InputPlugin,
            bevy::input_focus::InputFocusPlugin,
            bevy::camera::CameraPlugin,
            bevy::mesh::MeshPlugin,
            bevy::gizmos::GizmoPlugin,
            bevy::audio::AudioPlugin::default(),
            bevy::light::LightPlugin,
            bevy::picking::DefaultPickingPlugins,
        ))
        .init_asset::<StandardMaterial>()
        .init_asset::<Image>()
        .init_asset::<Font>()
        .configure_sets(
            Update,
            (
                ClientSystems::Receive,
                ClientSystems::Input,
                ClientSystems::Send,
                ClientSystems::Present,
            )
                .chain(),
        )
        .add_plugins((ClientPlugin, hud::Plugin));
        app.finish();
        app.cleanup();
        app.update();
        let id = app.world().resource::<PlayerId>().0;
        assert_ne!(id, 0);
        let root = app.world().resource::<Lobby>().players[&id];
        app.world_mut().remove_resource::<NetcodeClientTransport>();
        app.world_mut()
            .resource_mut::<RenetClient>()
            .set_connected();
        let mut server = RenetServer::new(data::renet_config());
        server.add_connection(id, false);
        let fingerprint = catalog_fingerprint(CATALOG_SOURCE);
        let mut publisher = ConfigurationPublisher::new(1, fingerprint).unwrap();
        let weapon = EquippedWeapon::new(
            catalog(),
            WeaponInstanceId(1),
            catalog().configuration(WeaponId::M4A1).unwrap(),
        )
        .unwrap();
        let handle = publisher
            .register(id, EquipmentSlot::Primary, weapon.instance())
            .unwrap();
        server.send_message(
            id,
            DefaultChannel::ReliableOrdered,
            data::encode(&ServerMessage::ClientConnected { id }),
        );
        for message in [
            ConfigurationMessage::Epoch {
                epoch: 1,
                catalog: fingerprint,
            },
            ConfigurationMessage::Configuration(publisher.entries()[0].clone()),
        ] {
            server.send_message(id, CONFIGURATION_CHANNEL, data::encode(&message));
        }
        server.send_message(
            id,
            DefaultChannel::Unreliable,
            data::encode(&WorldSnapshot {
                configuration_epoch: 1,
                sequence: 1,
                simulation_time: 0.1,
                destroyed_cells: Default::default(),
                raid: Default::default(),
                players: vec![ClientData {
                    id,
                    alive: true,
                    configuration: handle,
                    weapon_instance: weapon.instance().id,
                    mechanics: weapon.mechanics().snapshot(),
                    ..default()
                }],
                projectiles: vec![],
                impact_marks: vec![],
                fired_projectiles: vec![],
                physics_pieces: vec![],
            }),
        );
        for packet in server.get_packets_to_send(id).unwrap() {
            app.world_mut()
                .resource_mut::<RenetClient>()
                .process_packet(packet);
        }
        app.update();
        app.update();
        assert_eq!(
            app.world().resource::<sync::ClientSession>().last_sequence,
            1
        );
        let players = &app.world().resource::<Lobby>().players;
        assert_eq!(players.len(), 1);
        assert_eq!(players[&id], root);
        assert!(app.world().get::<LocalPlayer>(root).is_some());
        assert!(app.world().get::<PlayerVisualState>(root).unwrap().alive);

        focused_ground_prompt_uses_inventory_font_and_clears(&mut app, id);
    }

    fn focused_ground_prompt_uses_inventory_font_and_clears(app: &mut App, id: u64) {
        use inventory::{GroundItem, InventoryUpdate, UpdateBody};
        {
            let mut session = app.world_mut().resource_mut::<sync::ClientSession>();
            session.raid.phase = common::raid::RaidPhase::Active;
            session.raid.raid_id = 1;
            session.players[0].participant.begin();
        }
        let key = item_inventory::SessionKey {
            raid_epoch: 1,
            session_epoch: 1,
        };
        let item = ItemInstanceId(7);
        let seed = item_manifest::S1_SEEDS
            .iter()
            .position(|definition| *definition == "food.water500")
            .unwrap()
            .try_into()
            .unwrap();
        for body in [
            UpdateBody::Begin,
            UpdateBody::Ground(vec![GroundItem {
                id: item,
                seed,
                root: [0.0; 3],
            }]),
            UpdateBody::GroundFocus(Some(item)),
        ] {
            app.world_mut()
                .resource_mut::<sync::inventory::State>()
                .apply(InventoryUpdate { session: key, body });
        }
        app.update();
        let world = app.world_mut();
        assert!(world.resource::<sync::ClientSession>().playing(id));
        let mut spans = world.query::<(&TextSpan, &TextFont, &ChildOf)>();
        let (label, font, parent) = spans
            .iter(world)
            .find(|(_, _, parent)| world.get::<hud::RaidHud>(parent.parent()).is_some())
            .unwrap();
        assert_eq!(label.as_str(), "\nGround item: Água potável 500 ml [F]");
        assert!(
            !world
                .get::<Text>(parent.parent())
                .unwrap()
                .contains("Ground item")
        );
        let bevy::text::FontSource::Handle(handle) = &font.font else {
            panic!("ground item names must not use the ASCII subset");
        };
        assert_eq!(
            world
                .resource::<Assets<Font>>()
                .get(handle)
                .unwrap()
                .data
                .as_ref(),
            include_bytes!("../../assets/fonts/FiraMono-Medium.ttf")
        );
        world
            .resource_mut::<sync::inventory::State>()
            .apply(InventoryUpdate {
                session: key,
                body: UpdateBody::GroundFocus(None),
            });
        app.update();
        let world = app.world();
        assert!(
            !spans
                .iter(world)
                .any(|(span, _, _)| span.contains("Ground item"))
        );
    }
}
