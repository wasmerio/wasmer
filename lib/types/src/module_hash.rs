use std::{
    fmt::{self, Display, Formatter},
    hash::Hash,
};

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use sha2::Digest;

/// The hash of a WebAssembly module.
#[derive(
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[rkyv(derive(Debug))]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum ModuleHash {
    /// Deprecated.
    XXHash([u8; 8]),

    /// sha256
    Sha256([u8; 32]),
}

#[cfg(feature = "artifact-size")]
impl loupe::MemoryUsage for ModuleHash {
    fn size_of_val(&self, _tracker: &mut dyn loupe::MemoryUsageTracker) -> usize {
        match self {
            Self::XXHash(_) => 8 * 8,
            Self::Sha256(_) => 8 * 32,
        }
    }
}

impl ModuleHash {
    /// Parse a Sha256 hash from a hex-encoded string.
    pub fn sha256_parse_hex(hex_str: &str) -> Result<Self, hex::FromHexError> {
        let mut hash = [0_u8; _];
        hex::decode_to_slice(hex_str, &mut hash)?;
        Ok(Self::Sha256(hash))
    }

    /// Generate a new [`ModuleHash`] based on the Sha256 hash of some bytes.
    pub fn new(wasm: impl AsRef<[u8]>) -> Self {
        let wasm = wasm.as_ref();
        let hash = sha2::Sha256::digest(wasm).into();
        Self::Sha256(hash)
    }

    /// Generate a random [`ModuleHash`]. For when you don't care about caches.
    pub fn random() -> Self {
        let mut bytes = [0_u8; _];
        getrandom::getrandom(&mut bytes).unwrap();
        Self::Sha256(bytes)
    }

    /// Get the raw hash.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::XXHash(bytes) => bytes.as_slice(),
            Self::Sha256(bytes) => bytes.as_slice(),
        }
    }

    /// Build a short hex representation of the hash (first 4 bytes).
    pub fn short_hash(&self) -> String {
        hex::encode_upper(&self.as_bytes()[..4])
    }
}

impl Display for ModuleHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode_upper(self.as_bytes()))
    }
}
