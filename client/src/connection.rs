//! A transport lifetime is also a replica and input lifetime.
use std::{
    io,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    time::SystemTime,
};

use bevy::prelude::*;
use bevy_renet2::{
    netcode::{ClientAuthentication, NativeSocket, NetcodeClientPlugin, NetcodeClientTransport},
    prelude::{RenetClient, RenetClientPlugin},
};
use common::{
    Client, DEFAULT_PORT, Lobby, LocalInput, LocalPlayer, PROTOCOL_ID, PlayerId, PlayerVisualState,
};

use crate::{ClientSystems, input::inventory_panel::Panel, loadout::Editor, menu, sync};

pub(crate) struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        dotenvy::dotenv().ok();
        app.add_plugins((RenetClientPlugin, NetcodeClientPlugin))
            .insert_resource(disconnected_client())
            .insert_resource(PlayerId(0))
            .insert_resource(Connection {
                request: Some(Request::Connect),
                notice: None,
                address: endpoint(),
            })
            .add_systems(
                Update,
                apply
                    .after(ClientSystems::Input)
                    .before(ClientSystems::Send),
            );
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Request {
    Connect,
    Disconnect,
}

#[derive(Resource)]
pub(crate) struct Connection {
    pub request: Option<Request>,
    pub notice: Option<String>,
    address: Result<SocketAddr, &'static str>,
}

fn disconnected_client() -> RenetClient {
    let mut client = RenetClient::new(common::data::renet_config(), false);
    client.disconnect();
    client
}

fn endpoint() -> Result<SocketAddr, &'static str> {
    let address: IpAddr = std::env::var("SERVER_ADDR")
        .unwrap_or_else(|_| "127.0.0.1".into())
        .parse()
        .map_err(|_| "Invalid SERVER_ADDR")?;
    let port = match std::env::var("TK_PORT") {
        Ok(port) => port.parse().map_err(|_| "Invalid TK_PORT")?,
        Err(_) => DEFAULT_PORT,
    };
    Ok(SocketAddr::new(address, port))
}

fn transport(server_addr: SocketAddr, client_id: u64) -> io::Result<NetcodeClientTransport> {
    let bind = if server_addr.is_ipv4() {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    } else {
        IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED)
    };
    let socket = NativeSocket::new(UdpSocket::bind((bind, 0))?).map_err(io::Error::other)?;
    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(io::Error::other)?;
    NetcodeClientTransport::new(
        current_time,
        ClientAuthentication::Unsecure {
            client_id,
            protocol_id: PROTOCOL_ID,
            socket_id: 0,
            server_addr,
            user_data: None,
        },
        socket,
    )
    .map_err(io::Error::other)
}

fn apply(world: &mut World) {
    let request = world.resource_mut::<Connection>().request.take();
    let connected_transport = world.contains_resource::<NetcodeClientTransport>();
    let Some(request) = request else {
        if connected_transport && world.resource::<RenetClient>().is_disconnected() {
            let reason = format!(
                "Disconnected: {:?}",
                world.resource::<RenetClient>().disconnect_reason()
            );
            reset(world);
            world.resource_mut::<Connection>().notice = Some(reason);
        }
        return;
    };
    reset(world);
    let notice = match request {
        Request::Disconnect => Some("Disconnected. Reconnect to start a fresh session.".into()),
        Request::Connect => {
            let id = (rand::random::<u64>() >> 1).max(1);
            match world.resource::<Connection>().address.and_then(|address| {
                transport(address, id).map_err(|error| {
                    warn!("Could not open server connection: {error}");
                    "Could not open UDP connection"
                })
            }) {
                Ok(transport) => {
                    world.insert_resource(RenetClient::new(
                        common::data::renet_config(),
                        transport.is_reliable(),
                    ));
                    world.insert_resource(transport);
                    world.resource_mut::<PlayerId>().0 = id;
                    rekey_root(world, id);
                    None
                }
                Err(reason) => Some(format!("{reason}. Fix the configuration and retry.")),
            }
        }
    };
    world.resource_mut::<Connection>().notice = notice;
}

fn reset(world: &mut World) {
    if let Some(mut transport) = world.get_resource_mut::<NetcodeClientTransport>() {
        transport.disconnect();
    }
    world.remove_resource::<NetcodeClientTransport>();
    world.insert_resource(disconnected_client());
    world.insert_resource(sync::ClientSession::default());
    world.insert_resource(sync::cover::State::default());
    world.insert_resource(sync::inventory::State::default());
    world.insert_resource(LocalInput::default());
    world.insert_resource(Editor::default());
    world.insert_resource(Panel::default());
    world.insert_resource(menu::Menu::default());
    world
        .resource_mut::<Messages<sync::inventory::Operate>>()
        .clear();
    world
        .resource_mut::<Messages<sync::inventory::OperationFinished>>()
        .clear();
    world.resource_mut::<Messages<menu::Activation>>().clear();
    world.resource_mut::<ButtonInput<KeyCode>>().reset_all();
    world.resource_mut::<ButtonInput<MouseButton>>().reset_all();

    let mut actors = world.query_filtered::<Entity, (With<Client>, Without<LocalPlayer>)>();
    let actors: Vec<_> = actors.iter(world).collect();
    for actor in actors {
        world.despawn(actor);
    }
    crate::render::reset_connection_visuals(world);
    world.resource_mut::<Lobby>().players.clear();
    world.resource_mut::<PlayerId>().0 = 0;
    rekey_root(world, 0);
}

fn rekey_root(world: &mut World, id: u64) {
    let mut roots = world.query_filtered::<Entity, With<LocalPlayer>>();
    if let Ok(root) = roots.single(world) {
        world.entity_mut(root).insert((
            Client { id },
            Transform::from_xyz(0.0, 1.0, 0.0),
            PlayerVisualState::default(),
        ));
        world.resource_mut::<Lobby>().players.insert(id, root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_renet2::{
        netcode::{NetcodeServerTransport, ServerAuthentication, ServerSetupConfig},
        prelude::{DefaultChannel, RenetServer},
    };
    use common::{
        catalog::CompartmentId,
        replication::inventory::{InventoryUpdate, UpdateBody},
    };
    use std::time::Duration;

    fn complete_handshake(
        world: &mut World,
        transport: &mut NetcodeServerTransport,
        server: &mut RenetServer,
    ) {
        let delta = Duration::from_millis(8);
        for _ in 0..96 {
            world.resource_scope(|world, mut net: Mut<NetcodeClientTransport>| {
                let mut client = world.resource_mut::<RenetClient>();
                net.update(delta, &mut client).unwrap();
                net.send_packets(&mut client).unwrap();
            });
            transport.update(delta, server).unwrap();
            transport.send_packets(server);
            if world.resource::<RenetClient>().is_connected() {
                return;
            }
        }
        panic!("loopback client did not connect");
    }

    #[test]
    fn reconnect_replaces_udp_session_and_discards_old_intents_and_epochs() {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = socket.local_addr().unwrap();
        let mut server_transport = NetcodeServerTransport::new(
            ServerSetupConfig {
                current_time: SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap(),
                max_clients: 4,
                max_token_lifetime: 30,
                protocol_id: PROTOCOL_ID,
                socket_addresses: vec![vec![address]],
                authentication: ServerAuthentication::Unsecure,
            },
            NativeSocket::new(socket).unwrap(),
        )
        .unwrap();
        let mut server = RenetServer::new(common::data::renet_config());
        let mut world = World::new();
        world.insert_resource(Connection {
            request: Some(Request::Connect),
            notice: None,
            address: Ok(address),
        });
        world.init_resource::<Lobby>();
        world.insert_resource(PlayerId(0));
        world.init_resource::<Messages<sync::inventory::Operate>>();
        world.init_resource::<Messages<sync::inventory::OperationFinished>>();
        world.init_resource::<Messages<menu::Activation>>();
        world.init_resource::<ButtonInput<KeyCode>>();
        world.init_resource::<ButtonInput<MouseButton>>();
        let root = world.spawn(LocalPlayer).id();
        apply(&mut world);
        complete_handshake(&mut world, &mut server_transport, &mut server);
        let old = world.resource::<PlayerId>().0;
        assert!(server.is_connected(old));
        let remote = world.spawn(Client { id: old + 1 }).id();
        world
            .resource_mut::<Lobby>()
            .players
            .insert(old + 1, remote);
        let mut state = world.resource_mut::<sync::ClientSession>();
        state.last_sequence = 999;
        state.configurations.cache.begin_epoch(9).unwrap();
        let session = common::catalog::item_inventory::SessionKey {
            raid_epoch: 9,
            session_epoch: 99,
        };
        world
            .resource_mut::<sync::inventory::State>()
            .apply(InventoryUpdate {
                session,
                body: UpdateBody::Begin,
            });
        world.resource_mut::<LocalInput>().lobby_sequence = 8;
        world.resource_mut::<LocalInput>().interact = true;
        world.resource_mut::<Editor>().pending = Some(55);
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        world
            .resource_mut::<Messages<sync::inventory::Operate>>()
            .write(sync::inventory::Operate {
                action: common::catalog::item_inventory::InventoryAction::Merge {
                    source: common::catalog::ItemInstanceId(1),
                    target: common::catalog::ItemInstanceId(2),
                },
                scope: [CompartmentId(old); 2],
            });
        world
            .resource_mut::<RenetClient>()
            .send_message(DefaultChannel::ReliableOrdered, vec![12]);
        world.resource_mut::<Connection>().request = Some(Request::Disconnect);
        apply(&mut world);
        assert!(world.resource::<RenetClient>().is_disconnected());
        assert!(!world.contains_resource::<NetcodeClientTransport>());
        assert!(world.get_entity(remote).is_err());
        server_transport
            .update(Duration::from_millis(8), &mut server)
            .unwrap();
        assert!(!server.is_connected(old));
        world.resource_mut::<Connection>().request = Some(Request::Connect);
        apply(&mut world);
        complete_handshake(&mut world, &mut server_transport, &mut server);
        let fresh = world.resource::<PlayerId>().0;
        assert_ne!(fresh, old);
        assert_eq!(world.resource::<Lobby>().players[&fresh], root);
        assert_eq!(world.resource::<sync::ClientSession>().last_sequence, 0);
        assert_eq!(
            world
                .resource::<sync::ClientSession>()
                .configurations
                .cache
                .epoch(),
            0
        );
        assert!(
            world
                .resource::<sync::inventory::State>()
                .grid_session()
                .is_none()
        );
        assert_eq!(world.resource::<LocalInput>().lobby_sequence, 0);
        assert!(!world.resource::<LocalInput>().interact);
        assert!(world.resource::<Editor>().pending.is_none());
        assert!(
            !world
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left)
        );
        assert!(
            world
                .resource::<Messages<sync::inventory::Operate>>()
                .is_empty()
        );
        assert!(
            server
                .receive_message(fresh, DefaultChannel::ReliableOrdered)
                .is_none()
        );

        // Cancel while the new handshake is still in progress.
        world.resource_mut::<Connection>().request = Some(Request::Connect);
        apply(&mut world);
        assert!(world.resource::<NetcodeClientTransport>().is_connecting());
        world.resource_mut::<Connection>().request = Some(Request::Disconnect);
        apply(&mut world);
        assert!(!world.contains_resource::<NetcodeClientTransport>());
    }
}
