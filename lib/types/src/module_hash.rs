use std::fmt::{self, Display, Formatter};

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use sha2::Digest;

/// Hashing algorithm to be used for the module info
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HashAlgorithm {
    /// Sha256
    Sha256,
    /// XXHash
    XXHash,
}

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
    /// xxhash
    XXHash([u8; 8]),

    /// sha256
    Sha256([u8; 32]),
}

#[cfg(feature = "artifact-size")]
impl loupe::MemoryUsage for ModuleHash {
    fn size_of_val(&self, _tracker: &mut dyn loupe::MemoryUsageTracker) -> usize {
        match self {
            ModuleHash::XXHash(_) => 8 * 8,
            ModuleHash::Sha256(_) => 8 * 32,
        }
    }
}

impl ModuleHash {
    /// Create a new [`ModuleHash`] from the raw xxhash hash.
    pub fn xxhash_from_bytes(key: [u8; 8]) -> Self {
        Self::XXHash(key)
    }

    /// Create a new [`ModuleHash`] from the raw sha256 hash.
    pub fn sha256_from_bytes(key: [u8; 32]) -> Self {
        Self::Sha256(key)
    }

    /// Parse a XXHash hash from a hex-encoded string.
    pub fn xxhash_parse_hex(hex_str: &str) -> Result<Self, hex::FromHexError> {
        let mut hash = [0_u8; 8];
        hex::decode_to_slice(hex_str, &mut hash)?;
        Ok(Self::xxhash_from_bytes(hash))
    }

    /// Parse a Sha256 hash from a hex-encoded string.
    pub fn sha256_parse_hex(hex_str: &str) -> Result<Self, hex::FromHexError> {
        let mut hash = [0_u8; 32];
        hex::decode_to_slice(hex_str, &mut hash)?;
        Ok(Self::sha256_from_bytes(hash))
    }

    /// Generate a new [`ModuleCache`] based on the XXHash hash of some bytes.
    pub fn xxhash(wasm: impl AsRef<[u8]>) -> Self {
        let wasm = wasm.as_ref();

        let hash = xxhash_rust::xxh64::xxh64(wasm, 0);

        Self::XXHash(hash.to_ne_bytes())
    }

    /// Generate a new [`ModuleCache`] based on the Sha256 hash of some bytes.
    pub fn sha256(wasm: impl AsRef<[u8]>) -> Self {
        let wasm = wasm.as_ref();

        let hash: [u8; 32] = sha2::Sha256::digest(wasm).into();

        Self::Sha256(hash)
    }

    /// Get the raw hash.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::XXHash(bytes) => bytes.as_slice(),
            Self::Sha256(bytes) => bytes.as_slice(),
        }
    }
}

impl Display for ModuleHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        fn format<const N: usize>(f: &mut Formatter<'_>, bytes: &[u8; N]) -> fmt::Result {
            for byte in bytes {
                write!(f, "{byte:02X}")?;
            }

            Ok(())
        }

        match self {
            Self::XXHash(bytes) => format(f, bytes)?,
            Self::Sha256(bytes) => format(f, bytes)?,
        }

        Ok(())
    }
}
