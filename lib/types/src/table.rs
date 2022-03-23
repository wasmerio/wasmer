use loupe::MemoryUsage;
#[cfg(feature = "enable-rkyv")]
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

/// Implementation styles for WebAssembly tables.
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize, MemoryUsage)]
#[cfg_attr(
    feature = "enable-rkyv",
    derive(RkyvSerialize, RkyvDeserialize, Archive)
)]
pub enum TableStyle {
    /// Signatures are stored in the table and checked in the caller.
    CallerChecksSignature,
}
