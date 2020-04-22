use crate::indexes::{GlobalIndex, MemoryIndex};

#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};

/// A memory index and offset within that memory where a data initialization
/// should is to be performed.
#[derive(Clone)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct DataInitializerLocation {
    /// The index of the memory to initialize.
    pub memory_index: MemoryIndex,

    /// Optionally a Global variable base to initialize at.
    pub base: Option<GlobalIndex>,

    /// A constant offset to initialize at.
    pub offset: usize,
}

/// A data initializer for linear memory.
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct DataInitializer<'data> {
    /// The location where the initialization is to be performed.
    pub location: DataInitializerLocation,

    /// The initialization data.
    pub data: &'data [u8],
}
