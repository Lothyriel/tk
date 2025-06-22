use bevy_renet2::prelude::ConnectionConfig;
use serde::de::DeserializeOwned;

const BINCODE_CONFIG: bincode::config::Configuration = bincode::config::standard();

pub fn renet_config() -> ConnectionConfig {
    // change here if we need more reliable/unrealiable channels
    ConnectionConfig::test()
}

pub fn encode<S: serde::Serialize>(input: S) -> Vec<u8> {
    bincode::serde::encode_to_vec(&input, BINCODE_CONFIG).unwrap()
}

pub fn decode<D: DeserializeOwned>(input: &[u8]) -> D {
    let (output, _) = bincode::serde::borrow_decode_from_slice(input, BINCODE_CONFIG).unwrap();

    output
}
