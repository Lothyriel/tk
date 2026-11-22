//! Directional envelopes for the point-to-point inventory channel.
use crate::{
    catalog::{
        item_inventory::{
            CompartmentDelta, InventoryOperation, InventorySnapshot, OperationResult, SessionKey,
        },
        CompartmentId, ItemInstanceId,
    },
    data,
};
use bytes::Bytes;
use rkyv::{Archive, Deserialize, Serialize};

pub const CHANNEL: u8 = 6;
pub const MAX_REQUEST_BYTES: usize = 256;
pub const MAX_UPDATE_BYTES: usize = 16 * 1024;
const VERSION: u8 = 4;

#[derive(Debug, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub enum InventoryRequest {
    Operate(InventoryOperation),
    Resync {
        session: SessionKey,
        compartment: CompartmentId,
    },
    Drop {
        session: SessionKey,
        item: ItemInstanceId,
        revision: u64,
    },
}

#[derive(Debug, PartialEq, Archive, Serialize, Deserialize)]
pub struct InventoryUpdate {
    pub session: SessionKey,
    pub body: UpdateBody,
}

#[derive(Debug, PartialEq, Archive, Serialize, Deserialize)]
pub enum UpdateBody {
    Begin,
    Closed(CompartmentId),
    Snapshot(InventorySnapshot),
    Delta(Vec<CompartmentDelta>),
    Result { id: u64, result: OperationResult },
    Ground(Vec<GroundItem>),
    GroundFocus(Option<ItemInstanceId>),
}

#[derive(Clone, Debug, PartialEq, Archive, Serialize, Deserialize)]
pub struct GroundItem {
    pub id: ItemInstanceId,
    /// Index into S1_SEEDS, covered by the connection protocol ID.
    pub seed: u8,
    /// World position of the asset's authored root, not the collider center.
    pub root: [f32; 3],
}

#[derive(Debug, PartialEq, Eq)]
pub enum WireError {
    Size,
    Version(u8),
    Archive,
}

fn payload(frame: &[u8], limit: usize) -> Result<&[u8], WireError> {
    if frame.len() > limit {
        return Err(WireError::Size);
    }
    match frame.split_first() {
        Some((&VERSION, payload)) => Ok(payload),
        Some((&other, _)) => Err(WireError::Version(other)),
        None => Err(WireError::Archive),
    }
}

fn frame(payload: Bytes, limit: usize) -> Result<Bytes, WireError> {
    if payload.len() >= limit {
        return Err(WireError::Size);
    }
    let mut frame = Vec::with_capacity(payload.len() + 1);
    frame.push(VERSION);
    frame.extend_from_slice(&payload);
    Ok(frame.into())
}

macro_rules! codec {
    ($ty:ty, $limit:ident) => {
        impl $ty {
            pub fn encode(&self) -> Result<Bytes, WireError> {
                frame(data::encode(self), $limit)
            }

            pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
                data::try_decode(payload(bytes, $limit)?).map_err(|_| WireError::Archive)
            }
        }
    };
}

codec!(InventoryRequest, MAX_REQUEST_BYTES);
codec!(InventoryUpdate, MAX_UPDATE_BYTES);

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_renet2::prelude::{DefaultChannel, RenetClient, RenetServer};

    #[test]
    fn inventory_wire_roundtrips_ground_and_cleanup() {
        for items in [
            vec![GroundItem {
                id: ItemInstanceId(7),
                seed: 3,
                root: [-4.25, 0.1, 3.0],
            }],
            vec![],
        ] {
            let update = InventoryUpdate {
                session: SessionKey {
                    raid_epoch: 3,
                    session_epoch: 5,
                },
                body: UpdateBody::Ground(items),
            };
            let frame = update.encode().unwrap();
            assert_eq!(InventoryUpdate::decode(&frame), Ok(update));
        }
        assert_eq!(InventoryUpdate::decode(&[1]), Err(WireError::Version(1)));
    }

    #[test]
    fn inventory_wire_checks_bounds_versions_and_archive() {
        let request = InventoryRequest::Resync {
            session: SessionKey {
                raid_epoch: 3,
                session_epoch: 5,
            },
            compartment: CompartmentId(1),
        };
        let encoded = request.encode().unwrap();
        assert_eq!(InventoryRequest::decode(&encoded), Ok(request));
        assert_eq!(
            InventoryRequest::decode(&[VERSION]),
            Err(WireError::Archive)
        );
        assert_eq!(InventoryUpdate::decode(&[99]), Err(WireError::Version(99)));
        assert_eq!(
            InventoryRequest::decode(&[VERSION; MAX_REQUEST_BYTES + 1]),
            Err(WireError::Size)
        );
        assert_eq!(
            InventoryUpdate::decode(&[VERSION; MAX_UPDATE_BYTES + 1]),
            Err(WireError::Size)
        );
    }

    #[test]
    fn inventory_wire_delivers_privately_without_blocking_input() {
        let mut config = data::renet_config();
        config.available_bytes_per_tick = 1024;
        let mut server = RenetServer::new(config.clone());
        let mut client = RenetClient::new(config.clone(), false);
        let mut other = RenetClient::new(config, false);
        for id in [7, 8] {
            server.add_connection(id, false);
        }
        client.set_connected();
        other.set_connected();
        let session = SessionKey {
            raid_epoch: 3,
            session_epoch: 5,
        };
        let request = InventoryRequest::Resync {
            session,
            compartment: CompartmentId(1),
        };
        for _ in 0..150 {
            client.send_message(CHANNEL, request.encode().unwrap());
        }
        client.send_message(DefaultChannel::ReliableOrdered, data::encode(&42_u64));
        for packet in client.get_packets_to_send() {
            server.process_packet_from(packet, 7).unwrap();
        }
        let input = server
            .receive_message(7, DefaultChannel::ReliableOrdered)
            .unwrap();
        assert_eq!(data::decode::<u64>(&input), 42);
        assert_eq!(
            InventoryRequest::decode(&server.receive_message(7, CHANNEL).unwrap()),
            Ok(request)
        );
        let update = InventoryUpdate {
            session,
            body: UpdateBody::Begin,
        };
        server.send_message(7, CHANNEL, update.encode().unwrap());
        for packet in server.get_packets_to_send(7).unwrap() {
            client.process_packet(packet);
        }
        for packet in server.get_packets_to_send(8).unwrap() {
            other.process_packet(packet);
        }
        assert_eq!(
            InventoryUpdate::decode(&client.receive_message(CHANNEL).unwrap()),
            Ok(update)
        );
        assert!(other.receive_message(CHANNEL).is_none());
    }
}
