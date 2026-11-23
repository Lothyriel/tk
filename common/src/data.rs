use bevy_renet2::prelude::{ChannelConfig, ConnectionConfig, SendType};
use bytes::Bytes;
use rkyv::{
    api::high::{to_bytes_in, HighSerializer, HighValidator},
    from_bytes,
    rancor::{Error, Strategy},
    ser::allocator::ArenaHandle,
    util::AlignedVec,
    Archive, Deserialize, Serialize,
};

pub fn renet_config() -> ConnectionConfig {
    let mut config = ConnectionConfig::test();
    // Separate queues/order domains: configuration must not head-of-line block
    // ordinary input or connectivity. The shared ID is unidirectional.
    let channel = ChannelConfig {
        channel_id: crate::replication::CONFIGURATION_CHANNEL,
        max_memory_usage_bytes: 2 * 1024 * 1024,
        send_type: SendType::ReliableOrdered {
            resend_time: std::time::Duration::from_millis(100),
        },
    };
    config.server_channels_config.push(channel.clone());
    config.client_channels_config.push(channel);
    let inventory = ChannelConfig {
        channel_id: crate::replication::inventory::CHANNEL,
        max_memory_usage_bytes: 64 * 1024,
        send_type: SendType::ReliableOrdered {
            resend_time: std::time::Duration::from_millis(100),
        },
    };
    config.server_channels_config.push(ChannelConfig {
        channel_id: crate::replication::shots::SHOT_CHANNEL,
        max_memory_usage_bytes: 2 * 1024 * 1024,
        send_type: SendType::ReliableOrdered {
            resend_time: std::time::Duration::from_millis(100),
        },
    });
    let mut destruction = config.server_channels_config[0].clone();
    destruction.channel_id = crate::map::destruction::CHANNEL;
    destruction.max_memory_usage_bytes = 512 * 1024;
    // Frozen fragment shapes make full cover state span several datagrams.
    // Repeating unreliable snapshots cannot recover periodic fragment loss.
    // Retry independently; receivers discard superseded snapshot sequences.
    destruction.send_type = SendType::ReliableUnordered {
        resend_time: std::time::Duration::from_millis(100),
    };
    config.server_channels_config.insert(0, destruction);
    // Independent bounded cover state must fit beside the existing <60 kB actor
    // snapshot without starving either channel in the worst physical scene.
    config.available_bytes_per_tick = 80_000;
    // Send control and shot commits ahead of replaceable presentation state.
    // The additional cover queue and 60 Hz snapshots must not delay reliable
    // configuration replies under packet loss.
    config
        .server_channels_config
        .sort_by_key(|channel| match channel.channel_id {
            2 => 0,
            crate::replication::CONFIGURATION_CHANNEL => 1,
            crate::replication::shots::SHOT_CHANNEL => 2,
            crate::map::destruction::CHANNEL => 3,
            0 => 4,
            _ => 5,
        });
    config
        .client_channels_config
        .sort_by_key(|channel| match channel.channel_id {
            crate::replication::CONFIGURATION_CHANNEL => 0,
            2 => 1,
            _ => 2,
        });
    // Append after sorting so private resync never queues ahead of existing
    // input/control or the global snapshot in either direction.
    config.server_channels_config.push(inventory.clone());
    config.client_channels_config.push(inventory);
    config
}

pub fn encode(
    input: &impl for<'a> Serialize<HighSerializer<Vec<u8>, ArenaHandle<'a>, Error>>,
) -> Bytes {
    Bytes::from(to_bytes_in::<_, Error>(input, Vec::new()).unwrap())
}

pub fn decode<D>(input: &[u8]) -> D
where
    D: Archive,
    D::Archived: for<'a> rkyv::bytecheck::CheckBytes<HighValidator<'a, Error>>
        + Deserialize<D, Strategy<rkyv::de::Pool, Error>>,
{
    try_decode(input).expect("Valid archived network payload")
}

pub fn try_decode<D>(input: &[u8]) -> Result<D, Error>
where
    D: Archive,
    D::Archived: for<'a> rkyv::bytecheck::CheckBytes<HighValidator<'a, Error>>
        + Deserialize<D, Strategy<rkyv::de::Pool, Error>>,
{
    // Transport frames can start at any byte offset. rkyv's checked API still
    // requires aligned storage even when the archived contents are valid.
    let mut aligned = AlignedVec::<16>::with_capacity(input.len());
    aligned.extend_from_slice(input);
    from_bytes::<D, Error>(&aligned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_replies_arrive_under_snapshot_cover_and_input_queue_pressure() {
        use bevy_renet2::prelude::{DefaultChannel, RenetClient, RenetServer};
        let mut config = renet_config();
        config.available_bytes_per_tick = 1024;
        let mut server = RenetServer::new(config.clone());
        let mut client = RenetClient::new(config, false);
        server.add_connection(7, false);
        client.set_connected();
        server.send_message(7, DefaultChannel::Unreliable, vec![0; 4096]);
        server.send_message(7, crate::map::destruction::CHANNEL, vec![0; 4096]);
        server.send_message(
            7,
            crate::replication::CONFIGURATION_CHANNEL,
            encode(&77_u64),
        );
        for packet in server.get_packets_to_send(7).unwrap() {
            client.process_packet(packet);
        }
        let reply = client
            .receive_message(crate::replication::CONFIGURATION_CHANNEL)
            .expect("reliable control reply must arrive in the first budgeted update");
        assert_eq!(decode::<u64>(&reply), 77);

        client.send_message(DefaultChannel::ReliableOrdered, vec![0; 4096]);
        client.send_message(crate::replication::CONFIGURATION_CHANNEL, encode(&88_u64));
        for packet in client.get_packets_to_send() {
            server.process_packet_from(packet, 7).unwrap();
        }
        let request = server
            .receive_message(7, crate::replication::CONFIGURATION_CHANNEL)
            .expect("configuration request must not wait behind the input backlog");
        assert_eq!(decode::<u64>(&request), 88);
    }

    #[test]
    fn network_archive_decodes_from_an_unaligned_transport_slice() {
        let payload = encode(&vec![1_u64, 2, 3]);
        let mut frame = vec![0_u8];
        frame.extend_from_slice(&payload);
        assert_eq!(try_decode::<Vec<u64>>(&frame[1..]).unwrap(), vec![1, 2, 3]);
        assert!(try_decode::<Vec<u64>>(&[1, 2, 3]).is_err());
    }

    #[test]
    fn persistent_anatomy_pose_and_piece_state_roundtrip_with_bounded_snapshot() {
        use crate::{combat::*, humanoid::*, *};
        let mut player = ClientData {
            id: 7,
            configuration: 1,
            weapon_instance: WeaponInstanceId(73),
            life: 4,
            respawn_ticks: 301,
            ..default()
        };
        player.body.anatomy.separate(Segment::LeftForearm);
        player.body.anatomy.organs[Organ::Heart as usize] = 25.0;
        player.pose.mode = PhysicalMode::Fallen;
        player.pose.mode_time = 1.2;
        player.pose.set_vault(Some(0.5));
        player.pose.sprinting = true;
        println!(
            "TK ACTOR archive_fixed_bytes={} normal_bytes={}",
            std::mem::size_of::<crate::ArchivedClientData>(),
            encode(&player).len()
        );
        let piece = PhysicsPieceData {
            id: 123,
            owner: 7,
            segment: Segment::LeftForearm,
            pose: SegmentPose {
                position: [1.0, 0.2, 3.0],
                rotation: Quat::from_rotation_x(0.5).to_array(),
            },
            detached: true,
            bot: false,
        };
        let shot = FiredProjectileData {
            id: 44,
            weapon: WeaponId::MP5,
            shooter: 7,
            time: 3.8,
            muzzle: [0.2, 1.2, 0.0],
            direction: [0.0, 0.0, -1.0],
            ejection: [0.25, 1.5, -0.15],
            ejection_present: true,
            audio_signature: 55,
        };
        let snapshot = WorldSnapshot {
            configuration_epoch: 1,
            destroyed_cells: crate::map::destroyed_mask(&[0, 17, 175]),
            sequence: 200,
            raid: raid::RaidState {
                mode: raid::MatchMode::ArenaSolo,
                loot_available: [true, false, true],
                ..default()
            },
            players: vec![player.clone()],
            projectiles: vec![],
            impact_marks: vec![ImpactMarkData {
                id: 9,
                pos: [1.0, 2.0, 3.0],
                normal: [0.0, 0.0, 1.0],
                size_mm: 25,
            }],
            fired_projectiles: vec![shot],
            physics_pieces: vec![piece],
            simulation_time: 4.0,
        };
        let mut decoded = try_decode::<WorldSnapshot>(&encode(&snapshot)).unwrap();
        assert!(decoded.catalog_valid());
        assert_eq!(decoded.fired_projectiles[0].weapon, WeaponId::MP5);
        assert_eq!(decoded.players[0].weapon_instance, WeaponInstanceId(73));
        assert_eq!(decoded.players[0].life, 4);
        assert_eq!(decoded.players[0].respawn_ticks, 301);
        assert_eq!(decoded.raid.mode, raid::MatchMode::ArenaSolo);
        assert_eq!(decoded.raid.loot_available, [true, false, true]);
        decoded.players[0].weapon_id = WeaponId(9999);
        assert!(!decoded.catalog_valid());
        decoded.players[0].weapon_id = WeaponId::M1911;
        assert!(
            !decoded.catalog_valid(),
            "known definition with a false class must be rejected"
        );
        decoded.players[0].weapon_id = WeaponId::M4A1;
        decoded.fired_projectiles[0].weapon = WeaponId(9999);
        assert!(!decoded.catalog_valid());
        decoded.fired_projectiles[0].weapon = WeaponId::MP5;
        assert!(decoded.catalog_valid());
        assert_eq!(
            crate::map::destroyed_ids(&decoded.destroyed_cells),
            vec![0, 17, 175]
        );
        assert_eq!(decoded.players[0].body.anatomy.missing, (1 << 6) | (1 << 7));
        assert_eq!(decoded.players[0].body.anatomy.organs[1], 25.0);
        assert_eq!(decoded.players[0].pose.mode, PhysicalMode::Fallen);
        assert_eq!(decoded.players[0].pose.mode_time, 1.2);
        assert_eq!(decoded.players[0].pose.vault, 128);
        assert!(decoded.players[0].pose.sprinting);
        assert_eq!(decoded.physics_pieces[0].id, 123);
        assert_eq!(decoded.physics_pieces[0].owner, 7);
        assert_eq!(decoded.physics_pieces[0].pose.position, [1.0, 0.2, 3.0]);
        assert_eq!(decoded.fired_projectiles[0].shooter, 7);
        assert_eq!(decoded.fired_projectiles[0].id, 44);
        assert_eq!(decoded.impact_marks[0].id, 9);
        assert_eq!(decoded.impact_marks[0].size_mm, 25);
        let mut worst = snapshot;
        worst.destroyed_cells = [u8::MAX; 22];
        worst.players = (0..32)
            .map(|id| ClientData {
                id,
                ..player.clone()
            })
            .collect();
        worst.physics_pieces = (0..320)
            .map(|id| PhysicsPieceData {
                id,
                ..worst.physics_pieces[0].clone()
            })
            .collect();
        worst.impact_marks = (0..256)
            .map(|id| ImpactMarkData {
                id,
                pos: [0.0; 3],
                normal: [0.0, 1.0, 0.0],
                size_mm: 50,
            })
            .collect();
        worst.fired_projectiles = (0..224)
            .map(|id| FiredProjectileData {
                id,
                ..worst.fired_projectiles[0].clone()
            })
            .collect();
        let bytes = encode(&worst);
        println!(
            "TK MAX SNAPSHOT actors=32 pieces=320 impacts=256 recent_shots=224 bytes={}",
            bytes.len()
        );
        assert!(bytes.len() < 60_000);
        for player in &mut worst.players {
            let rig = evaluate_pose(&PoseState::default(), 1.0, 1.0);
            player.pose.physical = Some(Box::new(PackedRig::pack(&rig)));
        }
        use crate::replication::shots::*;
        let shot_template = worst.fired_projectiles[0].clone();
        let physical = pack_snapshot(worst).unwrap();
        let packed = physical.bytes;
        let received: WorldSnapshot = decode(&packed);
        println!(
            "TK MAX PHYSICAL SNAPSHOT actors=32 physical_rigs=32 pieces=320 bytes={} reliable_only={}",
            packed.len(),
            physical.overflow.len()
        );
        assert!(packed.len() < MAX_SNAPSHOT_BYTES);
        assert_eq!(
            received
                .players
                .iter()
                .filter(|p| p.pose.physical.is_some())
                .count(),
            32
        );
        assert_eq!(received.physics_pieces.len(), 320);
        assert_eq!(received.impact_marks.len(), 256);
        let mut shots: Vec<_> = received
            .fired_projectiles
            .iter()
            .chain(&physical.overflow)
            .map(|shot| {
                assert_eq!(shot.ejection(), Some([0.25, 1.5, -0.15]));
                assert_eq!(shot.audio_signature, 55);
                shot.id
            })
            .collect();
        shots.sort_unstable();
        assert!(shots.into_iter().eq(0..224));
        // Project the same physical stress at higher rates, up to the 128 Hz
        // simulation ceiling (one shot per actor/tick). No mandatory fields
        // disappear: the explicit overflow plus snapshot retain every shot ID.
        for rpm in [900_u32, 1200, 2400, 7680] {
            let mut projected: WorldSnapshot = decode(&packed);
            let count = (32 * rpm).div_ceil(120); // half a second
            projected.fired_projectiles = (0..u64::from(count))
                .map(|id| FiredProjectileData {
                    id,
                    ..shot_template.clone()
                })
                .collect();
            let raw = encode(&projected).len();
            let result = pack_snapshot(projected).unwrap();
            let received: WorldSnapshot = decode(&result.bytes);
            assert!(result.bytes.len() < MAX_SNAPSHOT_BYTES);
            assert_eq!(received.players.len(), 32);
            assert_eq!(received.physics_pieces.len(), 320);
            assert_eq!(received.impact_marks.len(), 256);
            let ids: std::collections::BTreeSet<_> = received
                .fired_projectiles
                .iter()
                .chain(&result.overflow)
                .map(|shot| shot.id)
                .collect();
            assert_eq!(ids, (0..u64::from(count)).collect());
            println!(
                "TK SHOT PROJECTION rpm={rpm} shots={count} raw={raw} snapshot={} reliable_only={}",
                result.bytes.len(),
                result.overflow.len()
            );
        }
        let mut impossible: WorldSnapshot = decode(&packed);
        impossible.physics_pieces = vec![impossible.physics_pieces[0].clone(); 2000];
        assert!(pack_snapshot(impossible).unwrap_err().mandatory_bytes >= MAX_SNAPSHOT_BYTES);
        let decoded = try_decode::<WorldSnapshot>(&packed).unwrap();
        assert!(decoded.players.iter().all(|p| p.pose.physical.is_some()));
    }
}
