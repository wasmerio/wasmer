// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer-reborn/blob/master/ATTRIBUTIONS.md

//! Memory management for linear memories.
//!
//! `LinearMemory` is to WebAssembly linear memories what `Table` is to WebAssembly tables.

use crate::vmcontext::VMMemoryDefinition;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ptr::NonNull;
use thiserror::Error;
use wasm_common::{MemoryType, Pages};

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
    #[error("The memory plan is invalid because {}", reason)]
    InvalidMemoryPlan {
        /// The reason why the memory plan is invalid.
        reason: String,
    },
    /// A user defined error value, used for error cases not listed above.
    #[error("A user-defined error occurred: {0}")]
    Generic(String),
}

/// Implementation styles for WebAssembly linear memory.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryStyle {
    /// The actual memory can be resized and moved.
    Dynamic,
    /// Address space is allocated up front.
    Static {
        /// The number of mapped and unmapped pages.
        bound: Pages,
    },
}

/// A WebAssembly linear memory description along with our chosen style for
/// implementing it.
#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct MemoryPlan {
    /// The WebAssembly linear memory description.
    pub memory: MemoryType,
    /// Our chosen implementation style.
    pub style: MemoryStyle,
    /// Our chosen offset-guard size.
    pub offset_guard_size: u64,
}

/// Trait for implementing Wasm Memory used by Wasmer.
pub trait Memory: fmt::Debug + Send + Sync {
    /// Returns the memory plan for this memory.
    fn plan(&self) -> &MemoryPlan;

    /// Returns the number of allocated wasm pages.
    fn size(&self) -> Pages;

    /// Grow memory by the specified amount of wasm pages.
    fn grow(&self, delta: Pages) -> Result<Pages, MemoryError>;

    /// Return a [`VMMemoryDefinition`] for exposing the memory to compiled wasm code.
    ///
    /// The pointer returned in [`VMMemoryDefinition`] must be valid for the lifetime of this memory.
    fn vmmemory(&self) -> NonNull<VMMemoryDefinition>;
}
