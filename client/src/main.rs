use std::{
    net::{Ipv4Addr, UdpSocket},
    time::SystemTime,
};

use bevy::prelude::*;

use bevy_renet2::{
    netcode::{ClientAuthentication, NativeSocket, NetcodeClientPlugin, NetcodeClientTransport},
    prelude::{ConnectionConfig, RenetClient, RenetClientPlugin},
};
use common::*;

mod input;
mod sync;
mod ui;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(ClientPlugin)
        .run();
}

struct ClientPlugin;

impl bevy::prelude::Plugin for ClientPlugin {
    fn build(&self, app: &mut App) {
        let (client, transport) = renet_init();

        app.add_plugins(common::Plugin)
            .add_plugins(RenetClientPlugin)
            .add_plugins(NetcodeClientPlugin)
            .add_plugins(ui::Plugin)
            .add_plugins(input::Plugin)
            .add_plugins(sync::Plugin)
            .init_resource::<PlayerInput>()
            .insert_resource(client)
            .insert_resource(transport);
    }
}

fn renet_init() -> (RenetClient, NetcodeClientTransport) {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();

    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();

    let client_id = current_time.as_millis() as u64;

    let server_addr = dotenvy_macro::dotenv!("SERVER_ADDR")
        .parse()
        .expect("Valid SERVER_ADDR");

    info!("Connecting on {}", server_addr);

    let authentication = ClientAuthentication::Unsecure {
        client_id,
        protocol_id: PROTOCOL_ID,
        socket_id: 0,
        server_addr,
        user_data: None,
    };

    let socket = NativeSocket::new(socket).unwrap();

    let transport = NetcodeClientTransport::new(current_time, authentication, socket).unwrap();

    let client = RenetClient::new(ConnectionConfig::test(), transport.is_reliable());

    (client, transport)
}
