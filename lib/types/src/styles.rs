// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

//! Memory management for linear memories.
//!
//! `LinearMemory` is to WebAssembly linear memories what `Table` is to WebAssembly tables.

use loupe::MemoryUsage;
#[cfg(feature = "enable-rkyv")]
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use crate::Pages;


/// Implementation styles for WebAssembly linear memory.
#[derive(Debug, Clone, PartialEq, Eq, Hash, MemoryUsage)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
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

/// Implementation styles for WebAssembly tables.
#[derive(Debug, Clone, Hash, PartialEq, Eq, MemoryUsage)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(
    feature = "enable-rkyv",
    derive(RkyvSerialize, RkyvDeserialize, Archive)
)]
pub enum TableStyle {
    /// Signatures are stored in the table and checked in the caller.
    CallerChecksSignature,
}
