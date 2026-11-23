//! Focused real-UDP protocol/channel check. No window, game or recording.
use bevy::prelude::*;
use bevy_renet2::{
    netcode::{
        ClientAuthentication, NativeSocket, NetcodeClientTransport, NetcodeServerTransport,
        ServerAuthentication, ServerSetupConfig,
    },
    prelude::{DefaultChannel, RenetClient, RenetServer},
};
use common::{
    data,
    map::{
        destroyed_mask,
        destruction::{self, CoverSnapshot, Cut, FragmentBox, RubblePose},
    },
    raid::RaidPhase,
};
use std::{
    net::UdpSocket,
    time::{Duration, SystemTime},
};

fn main() {
    let socket = UdpSocket::bind("127.0.0.1:0").expect("bind isolated loopback server");
    let address = socket.local_addr().unwrap();
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let mut server_transport = NetcodeServerTransport::new(
        ServerSetupConfig {
            current_time: now,
            max_clients: 1,
            max_token_lifetime: 30,
            protocol_id: common::PROTOCOL_ID,
            socket_addresses: vec![vec![address]],
            authentication: ServerAuthentication::Unsecure,
        },
        NativeSocket::new(socket).unwrap(),
    )
    .unwrap();
    let mut server = RenetServer::new(data::renet_config());
    let socket = NativeSocket::new(UdpSocket::bind("127.0.0.1:0").unwrap()).unwrap();
    let mut client_transport = NetcodeClientTransport::new(
        now,
        ClientAuthentication::Unsecure {
            client_id: 7,
            protocol_id: common::PROTOCOL_ID,
            socket_id: 0,
            server_addr: address,
            user_data: None,
        },
        socket,
    )
    .unwrap();
    let mut client = RenetClient::new(data::renet_config(), false);
    let snapshot = CoverSnapshot {
        sequence: 77,
        raid_id: 3,
        phase: RaidPhase::Active,
        removed: destroyed_mask(&[72]),
        cuts: (0..destruction::MAX_CUTS)
            .map(|n| Cut {
                cell: n as u16,
                center: [0.0; 3],
                half_size: [0.003, 0.003, 0.02],
            })
            .collect(),
        rubble: (0..destruction::MAX_RUBBLE)
            .map(|n| RubblePose {
                id: n as u32 + 1,
                cell: n as u16,
                position: [1.0, 0.4, 2.0],
                rotation: Quat::IDENTITY.to_array(),
                boxes: (0..destruction::MAX_RUBBLE_SOLIDS / destruction::MAX_RUBBLE)
                    .map(|part| FragmentBox {
                        center: [part as f32 * 0.02, 0.0, 0.0],
                        half_size: [0.01; 3],
                    })
                    .collect(),
            })
            .collect(),
    };
    assert!(snapshot.valid());
    let payload = data::encode(&snapshot);
    assert!(payload.len() <= destruction::MAX_SNAPSHOT_BYTES);
    let mut input_received = false;
    for _ in 0..256 {
        let dt = Duration::from_secs_f64(1.0 / 128.0);
        client.update(dt);
        client_transport.update(dt, &mut client).unwrap();
        if client.is_connected() {
            client.send_message(
                DefaultChannel::ReliableOrdered,
                data::encode(&common::ClientInput {
                    forward: true,
                    ..default()
                }),
            );
        }
        client_transport.send_packets(&mut client).unwrap();
        server.update(dt);
        server_transport.update(dt, &mut server).unwrap();
        while let Some(bytes) = server.receive_message(7, DefaultChannel::ReliableOrdered) {
            let input = data::decode::<common::ClientInput>(&bytes);
            assert!(input.forward);
            assert!(!input.fire);
            input_received = true;
        }
        if input_received {
            server.send_message(7, destruction::CHANNEL, payload.clone());
        }
        server_transport.send_packets(&mut server);
        client_transport
            .update(Duration::ZERO, &mut client)
            .unwrap();
        if let Some(bytes) = client.receive_message(destruction::CHANNEL) {
            let received = data::decode::<CoverSnapshot>(&bytes);
            assert_eq!(received.sequence, 77);
            assert_eq!(received.raid_id, 3);
            assert_eq!(received.phase, RaidPhase::Active);
            assert_eq!(received.removed, snapshot.removed);
            assert_eq!(received.cuts, snapshot.cuts);
            assert_eq!(received.rubble, snapshot.rubble);
            assert!(input_received);
            println!(
                "PASS: loopback UDP protocol {}, authoritative input and {}-byte cover snapshot",
                common::PROTOCOL_ID,
                bytes.len()
            );
            return;
        }
        std::thread::yield_now();
    }
    panic!("loopback transport did not deliver the cover snapshot within 256 bounded updates");
}
