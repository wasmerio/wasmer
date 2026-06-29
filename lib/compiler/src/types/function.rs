/*
 * ! Remove me once rkyv generates doc-comments for fields or generates an #[allow(missing_docs)]
 * on their own.
 */
#![allow(missing_docs)]
// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

//! A `Compilation` contains the compiled function bodies for a WebAssembly
//! module (`CompiledFunction`).

use super::{
    address_map::FunctionAddressMap,
    unwind::{
        ArchivedCompiledFunctionUnwindInfo, CompiledFunctionUnwindInfo,
        CompiledFunctionUnwindInfoLike,
    },
};
use rkyv::{
    Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize, option::ArchivedOption,
};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use wasmer_types::{
    FunctionIndex, LocalFunctionIndex, SignatureIndex, TrapInformation, entity::PrimaryMap,
};

/// The frame info for a Compiled function.
///
/// This structure is only used for reconstructing
/// the frame information after a `Trap`.
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[derive(RkyvSerialize, RkyvDeserialize, Archive, Debug, Clone, PartialEq, Eq, Default)]
#[rkyv(derive(Debug))]
pub struct CompiledFunctionFrameInfo {
    /// The traps (in the function body).
    ///
    /// Code offsets of the traps MUST be in ascending order.
    pub traps: Vec<TrapInformation>,

    /// The address map.
    pub address_map: FunctionAddressMap,
}

/// The function body.
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[derive(RkyvSerialize, RkyvDeserialize, Archive, Debug, Clone, PartialEq, Eq)]
#[rkyv(derive(Debug))]
pub struct FunctionBody {
    /// The function body bytes.
    #[cfg_attr(feature = "enable-serde", serde(with = "serde_bytes"))]
    pub body: Vec<u8>,

    /// The function unwind info
    pub unwind_info: Option<CompiledFunctionUnwindInfo>,
}

/// Any struct that acts like a `FunctionBody`.
#[allow(missing_docs)]
pub trait FunctionBodyLike<'a> {
    type UnwindInfo: CompiledFunctionUnwindInfoLike<'a>;

    fn body(&'a self) -> &'a [u8];
    fn unwind_info(&'a self) -> Option<&'a Self::UnwindInfo>;
}

impl<'a> FunctionBodyLike<'a> for FunctionBody {
    type UnwindInfo = CompiledFunctionUnwindInfo;

    fn body(&'a self) -> &'a [u8] {
        self.body.as_ref()
    }

    fn unwind_info(&'a self) -> Option<&'a Self::UnwindInfo> {
        self.unwind_info.as_ref()
    }
}

impl<'a> FunctionBodyLike<'a> for ArchivedFunctionBody {
    type UnwindInfo = ArchivedCompiledFunctionUnwindInfo;

    fn body(&'a self) -> &'a [u8] {
        self.body.as_ref()
    }

    fn unwind_info(&'a self) -> Option<&'a Self::UnwindInfo> {
        match self.unwind_info {
            ArchivedOption::Some(ref x) => Some(x),
            ArchivedOption::None => None,
        }
    }
}

/// The result of compiling a WebAssembly function.
///
/// This structure only have the compiled information data
/// (function bytecode body, relocations, traps, jump tables
/// and unwind information).
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[derive(RkyvSerialize, RkyvDeserialize, Archive, Debug, Clone, PartialEq, Eq)]
#[rkyv(derive(Debug))]
pub struct CompiledFunction {
    /// The function body.
    pub body: FunctionBody,

    /// The frame information.
    pub frame_info: CompiledFunctionFrameInfo,

    /// The maximum stack allocation directly connected to the function itself
    /// if tracked (does not include any potential function calls).
    pub maximum_stack_usage: Option<usize>,
}

/// The compiled functions map (index in the Wasm -> function)
pub type Functions = PrimaryMap<LocalFunctionIndex, CompiledFunction>;
