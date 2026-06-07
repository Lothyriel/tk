use bevy_renet2::prelude::ConnectionConfig;
use bytes::Bytes;
use rkyv::{
    api::high::{to_bytes_in, HighSerializer, HighValidator},
    from_bytes,
    rancor::{Error, Strategy},
    ser::allocator::ArenaHandle,
    Archive, Deserialize, Serialize,
};

pub fn renet_config() -> ConnectionConfig {
    // change here if we need more reliable/unrealiable channels
    ConnectionConfig::test()
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
    from_bytes::<D, Error>(input).unwrap()
}
