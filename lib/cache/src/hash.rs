use std::string::ToString;

/// The hash of a wasm module.
///
/// Used as a key when loading and storing modules in a [`Cache`].
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
// WasmHash is made up of a 32 byte array
pub struct WasmHash([u8; 32]);

impl WasmHash {
    /// Creates a new hash. Has to be encodable as a Hex format.
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Hash a wasm module.
    ///
    /// # Note:
    /// This does no verification that the supplied data
    /// is, in fact, a wasm module.
    pub fn generate(wasm: &[u8]) -> Self {
        let hash = blake3::hash(wasm);
        WasmHash::new(hash.into())
    }

    // /// Create hash from hexadecimal representation
    // pub fn decode(hex_str: &str) -> Result<Self, Error> {
    //     let bytes = hex::decode(hex_str).map_err(|e| {
    //         Error::DeserializeError(format!(
    //             "Could not decode prehashed key as hexadecimal: {}",
    //             e
    //         ))
    //     })?;
    //     if bytes.len() != 32 {
    //         return Err(Error::DeserializeError(
    //             "Prehashed keys must deserialze into exactly 32 bytes".to_string(),
    //         ));
    //     }
    //     use std::convert::TryInto;
    //     Ok(WasmHash(bytes[0..32].try_into().map_err(|e| {
    //         Error::DeserializeError(format!("Could not get first 32 bytes: {}", e))
    //     })?))
    // }

    pub(crate) fn into_array(self) -> [u8; 32] {
        let mut total = [0u8; 32];
        total[0..32].copy_from_slice(&self.0);
        total
    }
}

impl ToString for WasmHash {
    /// Create the hexadecimal representation of the
    /// stored hash.
    fn to_string(&self) -> String {
        hex::encode(&self.into_array() as &[u8])
    }
}

// impl FromStr for Point {
// }
