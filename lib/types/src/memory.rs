use crate::Pages;
use loupe::MemoryUsage;
#[cfg(feature = "enable-rkyv")]
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Error type describing things that can go wrong when operating on Wasm Memories.
#[derive(Error, Debug, Clone, PartialEq, Hash)]
pub enum MemoryError {
    /// Low level error with mmap.
    #[error("Error when allocating memory: {0}")]
    Region(String),
    /// The operation would cause the size of the memory to exceed the maximum or would cause
    /// an overflow leading to unindexable memory.
    #[error("The memory could not grow: current size {} pages, requested increase: {} pages", current.0, attempted_delta.0)]
    CouldNotGrow {
        /// The current size in pages.
        current: Pages,
        /// The attempted amount to grow by in pages.
        attempted_delta: Pages,
    },
    /// The operation would cause the size of the memory size exceed the maximum.
    #[error("The memory is invalid because {}", reason)]
    InvalidMemory {
        /// The reason why the provided memory is invalid.
        reason: String,
    },
    /// Caller asked for more minimum memory than we can give them.
    #[error("The minimum requested ({} pages) memory is greater than the maximum allowed memory ({} pages)", min_requested.0, max_allowed.0)]
    MinimumMemoryTooLarge {
        /// The number of pages requested as the minimum amount of memory.
        min_requested: Pages,
        /// The maximum amount of memory we can allocate.
        max_allowed: Pages,
    },
    /// Caller asked for a maximum memory greater than we can give them.
    #[error("The maximum requested memory ({} pages) is greater than the maximum allowed memory ({} pages)", max_requested.0, max_allowed.0)]
    MaximumMemoryTooLarge {
        /// The number of pages requested as the maximum amount of memory.
        max_requested: Pages,
        /// The number of pages requested as the maximum amount of memory.
        max_allowed: Pages,
    },
    /// A user defined error value, used for error cases not listed above.
    #[error("A user-defined error occurred: {0}")]
    Generic(String),
}

/// Implementation styles for WebAssembly linear memory.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, MemoryUsage)]
#[cfg_attr(
    feature = "enable-rkyv",
    derive(RkyvSerialize, RkyvDeserialize, Archive)
)]
pub enum MemoryStyle {
    /// The actual memory can be resized and moved.
    Dynamic {
        /// Our chosen offset-guard size.
        ///
        /// It represents the size in bytes of extra guard pages after the end
        /// to optimize loads and stores with constant offsets.
        offset_guard_size: u64,
    },
    /// Address space is allocated up front.
    Static {
        /// The number of mapped and unmapped pages.
        bound: Pages,
        /// Our chosen offset-guard size.
        ///
        /// It represents the size in bytes of extra guard pages after the end
        /// to optimize loads and stores with constant offsets.
        offset_guard_size: u64,
    },
}

impl MemoryStyle {
    /// Returns the offset-guard size
    pub fn offset_guard_size(&self) -> u64 {
        match self {
            Self::Dynamic { offset_guard_size } => *offset_guard_size,
            Self::Static {
                offset_guard_size, ..
            } => *offset_guard_size,
        }
    }
}
