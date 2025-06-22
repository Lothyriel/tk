use std::{
    net::{Ipv4Addr, UdpSocket},
    time::SystemTime,
};

use bevy::{log::LogPlugin, prelude::*};

use bevy_renet2::{
    netcode::{
        NativeSocket, NetcodeServerPlugin, NetcodeServerTransport, ServerAuthentication,
        ServerSetupConfig,
    },
    prelude::{RenetServer, RenetServerPlugin},
};

use common::*;

mod tick;

fn main() {
    App::new()
        .add_plugins(LogPlugin::default())
        .add_plugins(MinimalPlugins)
        .add_plugins(ServerPlugin)
        .run();
}

struct ServerPlugin;

impl bevy::prelude::Plugin for ServerPlugin {
    fn build(&self, app: &mut App) {
        let (server, transport) = renet_init();

        app.add_plugins(common::Plugin)
            .add_plugins(RenetServerPlugin)
            .add_plugins(NetcodeServerPlugin)
            .add_plugins(tick::Plugin)
            .insert_resource(server)
            .insert_resource(transport);
    }
}

fn renet_init() -> (RenetServer, NetcodeServerTransport) {
    let public_addr = (Ipv4Addr::UNSPECIFIED, DEFAULT_PORT).into();

    info!("Starting server on: {:?}", public_addr);

    let socket = UdpSocket::bind(public_addr).unwrap();

    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();

    let server_config = ServerSetupConfig {
        current_time,
        max_clients: 32,
        protocol_id: PROTOCOL_ID,
        socket_addresses: vec![vec![public_addr]],
        authentication: ServerAuthentication::Unsecure,
    };

    let socket = NativeSocket::new(socket).unwrap();

    let transport = NetcodeServerTransport::new(server_config, socket).unwrap();

    let server = RenetServer::new(common::data::renet_config());

    (server, transport)
}
