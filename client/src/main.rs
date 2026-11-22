use std::{
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    time::SystemTime,
};

use bevy::{asset::AssetPlugin, prelude::*};

use bevy_renet2::{
    netcode::{ClientAuthentication, NativeSocket, NetcodeClientPlugin, NetcodeClientTransport},
    prelude::{RenetClient, RenetClientPlugin},
};
use common::*;

mod hud;
mod input;
mod loadout;
mod menu;
mod playtest;
mod render;
mod sync;

fn main() {
    let playtest = std::env::args().any(|argument| argument == "--playtest");
    let item_review = std::env::args().any(|argument| argument == "--item-review");
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
        let (client, transport, client_id) = renet_init();

        app.add_plugins(common::Plugin)
            .add_plugins(RenetClientPlugin)
            .add_plugins(NetcodeClientPlugin)
            .add_plugins(render::Plugin)
            .add_plugins(input::Plugin)
            .add_plugins(sync::Plugin)
            .add_plugins(loadout::Plugin)
            .add_plugins(menu::Plugin)
            .insert_resource(PlayerId(client_id))
            .insert_resource(client)
            .insert_resource(transport);
    }
}

fn renet_init() -> (RenetClient, NetcodeClientTransport, u64) {
    dotenvy::dotenv().ok();

    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();

    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();

    let server_addr = std::env::var("SERVER_ADDR")
        .unwrap_or_else(|_| "127.0.0.1".into())
        .parse()
        .expect("Valid SERVER_ADDR");

    let port = std::env::var("TK_PORT")
        .map(|value| value.parse::<u16>().expect("Valid TK_PORT"))
        .unwrap_or(DEFAULT_PORT);
    let server_socket = SocketAddr::new(server_addr, port);

    info!("Connecting on {}", server_socket);

    let client_id = rand::random::<u64>() & ((1_u64 << 63) - 1);

    let authentication = ClientAuthentication::Unsecure {
        client_id,
        protocol_id: PROTOCOL_ID,
        socket_id: 0,
        server_addr: server_socket,
        user_data: None,
    };

    let socket = NativeSocket::new(socket).unwrap();

    let transport = NetcodeClientTransport::new(current_time, authentication, socket).unwrap();

    let client = RenetClient::new(common::data::renet_config(), transport.is_reliable());

    (client, transport, client_id)
}
