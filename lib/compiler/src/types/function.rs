#![allow(missing_docs)]
// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

//! A `Compilation` contains the compiled function bodies for a WebAssembly
//! module (`CompiledFunction`).

use super::{
    address_map::FunctionAddressMap, relocation::Relocation, unwind::CompiledFunctionUnwindInfo,
};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use wasmer_types::{LocalFunctionIndex, TrapInformation, entity::PrimaryMap};

/// In-memory frame information used when emitting a native object.
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CompiledFunctionFrameInfo {
    /// Traps sorted by code offset.
    pub traps: Vec<TrapInformation>,
    pub address_map: FunctionAddressMap,
}

/// In-memory function body used when emitting a native object.
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionBody {
    #[cfg_attr(feature = "enable-serde", serde(with = "serde_bytes"))]
    pub body: Vec<u8>,
    pub unwind_info: Option<CompiledFunctionUnwindInfo>,
}

/// The result of compiling a function before native object emission.
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledFunction {
    pub body: FunctionBody,
    pub relocations: Vec<Relocation>,
    pub frame_info: CompiledFunctionFrameInfo,
    pub maximum_stack_usage: Option<usize>,
}

pub type CompiledFunctionBody = Vec<u8>;

/// The result of compiling a WebAssembly module into a native ELF image.
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[derive(Debug, PartialEq, Eq)]
pub struct Compilation {
    pub data: Vec<u8>,
    pub function_max_stack_usage: PrimaryMap<LocalFunctionIndex, Option<usize>>,
}
