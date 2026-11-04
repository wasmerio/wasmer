#![allow(missing_docs)]
// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

//! A `Compilation` contains the compiled function bodies for a WebAssembly
//! module (`CompiledFunction`).

use super::address_map::FunctionAddressMap;
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use wasmer_types::TrapInformation;

/// In-memory frame information used when emitting a native object.
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CompiledFunctionFrameInfo {
    /// Traps sorted by code offset.
    pub traps: Vec<TrapInformation>,
    pub address_map: FunctionAddressMap,
}

/// The compiled native container for a WebAssembly module.
pub type Compilation = Vec<u8>;
