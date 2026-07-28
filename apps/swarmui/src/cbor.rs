// Author: Lukas Bower
// Purpose: Provide maintained CBOR encoding and decoding helpers for SwarmUI.
// Copyright 2026 Lukas Bower

use serde::de::DeserializeOwned;
use serde::Serialize;

pub(crate) fn to_vec<T: Serialize + ?Sized>(
    value: &T,
) -> Result<Vec<u8>, ciborium::ser::Error<std::io::Error>> {
    let mut bytes = Vec::new();
    ciborium::ser::into_writer(value, &mut bytes)?;
    Ok(bytes)
}

pub(crate) fn from_slice<T: DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, ciborium::de::Error<std::io::Error>> {
    ciborium::de::from_reader(bytes)
}
