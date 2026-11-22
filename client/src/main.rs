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
    let item_review = std::env::args().any(|argument| argument == "--item-review");
    assert!(
        !grid_probe || !(playtest || item_review),
        "run the inventory grid probe without other playtest or review modes"
    );
    let viewport = playtest.then(playtest::requested_viewport).flatten();
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
    if grid_probe {
        app.add_plugins(playtest::inventory_grid::Plugin);
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
