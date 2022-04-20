use crate::DeserializeError;
use std::str::FromStr;
use std::string::ToString;

/// A hash used as a key when loading and storing modules in a
/// [`Cache`].
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
// Hash is made up of a 32 byte array
pub struct Hash([u8; 32]);

impl Hash {
    /// Creates a new instance from 32 raw bytes.
    /// Does not perform any hashing. In order to create a hash from data,
    /// use `Hash::generate`.
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Creates a new hash from a slice of bytes.
    pub fn generate(bytes: &[u8]) -> Self {
        let hash = blake3::hash(bytes);
        Self::new(hash.into())
    }

    pub(crate) fn to_array(self) -> [u8; 32] {
        self.0
    }
}

impl ToString for Hash {
    /// Create the hexadecimal representation of the
    /// stored hash.
    fn to_string(&self) -> String {
        hex::encode(&self.to_array())
    }
}

impl FromStr for Hash {
    type Err = DeserializeError;
    /// Create hash from hexadecimal representation
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = hex::decode(s).map_err(|e| {
            DeserializeError::Generic(format!(
                "Could not decode prehashed key as hexadecimal: {}",
                e
            ))
        })?;
        if bytes.len() != 32 {
            return Err(DeserializeError::Generic(
                "Prehashed keys must deserialze into exactly 32 bytes".to_string(),
            ));
        }
        use std::convert::TryInto;
        Ok(Self(bytes[0..32].try_into().map_err(|e| {
            DeserializeError::Generic(format!("Could not get first 32 bytes: {}", e))
        })?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_to_array_works() {
        let original = [
            0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x12, 0x65, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF,
            0x12, 0x65, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x12, 0x65, 0xAA, 0xBB, 0xCC, 0xDD,
            0xEE, 0xFF, 0x12, 0x65,
        ];
        let hash = Hash::new(original);
        assert_eq!(hash.to_array(), original);
    }
}
