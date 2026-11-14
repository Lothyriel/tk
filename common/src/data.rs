use bevy_renet2::prelude::ConnectionConfig;
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
    fn network_archive_decodes_from_an_unaligned_transport_slice() {
        let payload = encode(&vec![1_u64, 2, 3]);
        let mut frame = vec![0_u8];
        frame.extend_from_slice(&payload);
        assert_eq!(try_decode::<Vec<u64>>(&frame[1..]).unwrap(), vec![1, 2, 3]);
        assert!(try_decode::<Vec<u64>>(&[1, 2, 3]).is_err());
    }
}
