#![allow(missing_docs)]
// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

//! A `Compilation` contains the compiled function bodies for a WebAssembly
//! module (`CompiledFunction`).

use super::{
    address_map::FunctionAddressMap,
    relocation::Relocation,
    section::{CustomSection, SectionIndex},
    unwind::{CompiledFunctionUnwindInfo, CompiledFunctionUnwindInfoLike},
};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use wasmer_types::{
    FunctionIndex, LocalFunctionIndex, SignatureIndex, TrapInformation, entity::PrimaryMap,
};

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

/// Any struct that acts like a `FunctionBody`.
pub trait FunctionBodyLike<'a> {
    type UnwindInfo: CompiledFunctionUnwindInfoLike<'a>;

    fn body(&'a self) -> &'a [u8];
    fn unwind_info(&'a self) -> Option<&'a Self::UnwindInfo>;
}

impl<'a> FunctionBodyLike<'a> for FunctionBody {
    type UnwindInfo = CompiledFunctionUnwindInfo;

    fn body(&'a self) -> &'a [u8] {
        &self.body
    }

    fn unwind_info(&'a self) -> Option<&'a Self::UnwindInfo> {
        self.unwind_info.as_ref()
    }
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

pub type Functions = PrimaryMap<LocalFunctionIndex, CompiledFunction>;
pub type CustomSections = PrimaryMap<SectionIndex, CustomSection>;

/// Section indices for native unwind information.
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[derive(Debug, PartialEq, Eq, Clone, Default)]
pub struct UnwindInfo {
    pub eh_frame: Option<SectionIndex>,
    pub compact_unwind: Option<SectionIndex>,
}

impl UnwindInfo {
    pub fn new(eh_frame: SectionIndex) -> Self {
        Self { eh_frame: Some(eh_frame), compact_unwind: None }
    }

    pub fn new_cu(compact_unwind: SectionIndex) -> Self {
        Self { eh_frame: None, compact_unwind: Some(compact_unwind) }
    }
}

/// Section index for the native global offset table.
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[derive(Debug, PartialEq, Eq, Clone, Default)]
pub struct GOT {
    pub index: Option<SectionIndex>,
}

impl GOT {
    pub fn empty() -> Self {
        Self { index: None }
    }
}

/// In-memory inputs to native object emission, not a serialized artifact.
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[derive(Debug, PartialEq, Eq)]
pub struct ObjectCompilation {
    pub functions: Functions,
    pub custom_sections: CustomSections,
    pub function_call_trampolines: PrimaryMap<SignatureIndex, FunctionBody>,
    pub dynamic_function_trampolines: PrimaryMap<FunctionIndex, FunctionBody>,
    pub unwind_info: UnwindInfo,
    pub got: GOT,
}

pub type CompiledFunctionBody = Vec<u8>;

/// The result of compiling a WebAssembly module into a native ELF image.
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[derive(Debug, PartialEq, Eq)]
pub struct Compilation {
    pub data: Vec<u8>,
    pub function_max_stack_usage: PrimaryMap<LocalFunctionIndex, Option<usize>>,
}
