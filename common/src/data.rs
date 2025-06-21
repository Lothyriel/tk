use serde::de::DeserializeOwned;

const BINCODE_CONFIG: bincode::config::Configuration = bincode::config::standard();
pub fn encode<S: serde::Serialize>(input: S) -> Vec<u8> {
    bincode::serde::encode_to_vec(&input, BINCODE_CONFIG).unwrap()
}

pub fn decode<D: DeserializeOwned>(input: &[u8]) -> D {
    let (output, _) = bincode::serde::borrow_decode_from_slice(input, BINCODE_CONFIG).unwrap();

    output
}
