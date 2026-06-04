use bevy_renet2::prelude::ConnectionConfig;
use rkyv::{
    Archive, Deserialize, Serialize,
    api::high::{HighSerializer, HighValidator},
    from_bytes,
    rancor::{Error, Strategy},
    ser::allocator::ArenaHandle,
    to_bytes,
    util::AlignedVec,
};

pub fn renet_config() -> ConnectionConfig {
    // change here if we need more reliable/unrealiable channels
    ConnectionConfig::test()
}

pub fn encode(
    input: &impl for<'a> Serialize<HighSerializer<AlignedVec, ArenaHandle<'a>, Error>>,
) -> Vec<u8> {
    to_bytes::<Error>(input).unwrap().into_vec()
}

pub fn decode<D>(input: &[u8]) -> D
where
    D: Archive,
    D::Archived: for<'a> rkyv::bytecheck::CheckBytes<HighValidator<'a, Error>>
        + Deserialize<D, Strategy<rkyv::de::Pool, Error>>,
{
    from_bytes::<D, Error>(input).unwrap()
}
