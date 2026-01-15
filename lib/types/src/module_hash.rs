use std::fmt::{self, Display, Formatter};

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
// SHA-256 Module hash type
#[derive(Default)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[rkyv(derive(Debug))]
pub struct ModuleHash([u8; 32]);
// SHA-256 hash

#[cfg(feature = "artifact-size")]
impl loupe::MemoryUsage for ModuleHash {
    fn size_of_val(&self, _tracker: &mut dyn loupe::MemoryUsageTracker) -> usize {
        // TODO
        8 * 32
    }
}

impl ModuleHash {
    /// Parse a Sha256 hash from a hex-encoded string.
    pub fn sha256_parse_hex(hex_str: &str) -> Result<Self, hex::FromHexError> {
        let mut hash = Self::default();
        hex::decode_to_slice(hex_str, &mut hash.0)?;
        Ok(hash)
    }

    /// Generate a new [`ModuleHash`] based on the Sha256 hash of some bytes.
    pub fn new(wasm: impl AsRef<[u8]>) -> Self {
        let wasm = wasm.as_ref();
        let hash = sha2::Sha256::digest(wasm).into();
        Self(hash)
    }

    /// Generate a random [`ModuleHash`]. For when you don't care about caches.
    pub fn random() -> Self {
        let mut hash = Self::default();
        getrandom::getrandom(&mut hash.0).unwrap();
        hash
    }

    /// Get the raw hash.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl Display for ModuleHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode_upper(self.0))
    }
}
