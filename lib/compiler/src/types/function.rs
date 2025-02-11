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
    relocation::Relocation,
    section::{CustomSection, SectionIndex},
    unwind::{
        ArchivedCompiledFunctionUnwindInfo, CompiledFunctionUnwindInfo,
        CompiledFunctionUnwindInfoLike,
    },
};
use rkyv::{
    option::ArchivedOption, Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize,
};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use wasmer_types::{
    entity::PrimaryMap, FunctionIndex, LocalFunctionIndex, SignatureIndex, TrapInformation,
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
    fn unwind_info(&'a self) -> Option<&Self::UnwindInfo>;
}

impl<'a> FunctionBodyLike<'a> for FunctionBody {
    type UnwindInfo = CompiledFunctionUnwindInfo;

    fn body(&'a self) -> &'a [u8] {
        self.body.as_ref()
    }

    fn unwind_info(&'a self) -> Option<&Self::UnwindInfo> {
        self.unwind_info.as_ref()
    }
}

impl<'a> FunctionBodyLike<'a> for ArchivedFunctionBody {
    type UnwindInfo = ArchivedCompiledFunctionUnwindInfo;

    fn body(&'a self) -> &'a [u8] {
        self.body.as_ref()
    }

    fn unwind_info(&'a self) -> Option<&Self::UnwindInfo> {
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

    /// The relocations (in the body)
    pub relocations: Vec<Relocation>,

    /// The frame information.
    pub frame_info: CompiledFunctionFrameInfo,
}

/// The compiled functions map (index in the Wasm -> function)
pub type Functions = PrimaryMap<LocalFunctionIndex, CompiledFunction>;

/// The custom sections for a Compilation.
pub type CustomSections = PrimaryMap<SectionIndex, CustomSection>;

/// The unwinding information for this Compilation.
///
/// It is used for retrieving the unwind information once an exception
/// happens.
/// In the future this structure may also hold other information useful
/// for debugging.
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[derive(RkyvSerialize, RkyvDeserialize, Archive, Debug, PartialEq, Eq, Clone, Default)]
#[rkyv(derive(Debug), compare(PartialEq))]
pub struct UnwindInfo {
    /// The section index in the [`Compilation`] that corresponds to the exception frames.
    /// [Learn
    /// more](https://refspecs.linuxfoundation.org/LSB_3.0.0/LSB-PDA/LSB-PDA/ehframechpt.html).
    pub eh_frame: Option<SectionIndex>,
    pub compact_unwind: Option<SectionIndex>,
}

impl UnwindInfo {
    /// Creates a `Dwarf` struct with the corresponding indices for its sections
    pub fn new(eh_frame: SectionIndex) -> Self {
        Self {
            eh_frame: Some(eh_frame),
            compact_unwind: None,
        }
    }

    pub fn new_cu(compact_unwind: SectionIndex) -> Self {
        Self {
            eh_frame: None,
            compact_unwind: Some(compact_unwind),
        }
    }
}

/// The GOT - Global Offset Table - for this Compilation.
///
/// The GOT is but a list of pointers to objects (functions, data, sections..); in our context the
/// GOT is represented simply as a custom section.
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[derive(RkyvSerialize, RkyvDeserialize, Archive, Debug, PartialEq, Eq, Clone, Default)]
#[rkyv(derive(Debug))]
pub struct GOT {
    /// The section index in the [`Compilation`] that corresponds to the GOT.
    pub index: Option<SectionIndex>,
}

impl GOT {
    pub fn empty() -> Self {
        Self { index: None }
    }
}
/// The result of compiling a WebAssembly module's functions.
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[derive(Debug, PartialEq, Eq)]
pub struct Compilation {
    /// Compiled code for the function bodies.
    pub functions: Functions,

    /// Custom sections for the module.
    /// It will hold the data, for example, for constants used in a
    /// function, global variables, rodata_64, hot/cold function partitioning, ...
    pub custom_sections: CustomSections,

    /// Trampolines to call a function defined locally in the wasm via a
    /// provided `Vec` of values.
    ///
    /// This allows us to call easily Wasm functions, such as:
    ///
    /// ```ignore
    /// let func = instance.exports.get_function("my_func");
    /// func.call(&[Value::I32(1)]);
    /// ```
    pub function_call_trampolines: PrimaryMap<SignatureIndex, FunctionBody>,

    /// Trampolines to call a dynamic function defined in
    /// a host, from a Wasm module.
    ///
    /// This allows us to create dynamic Wasm functions, such as:
    ///
    /// ```ignore
    /// fn my_func(values: &[Val]) -> Result<Vec<Val>, RuntimeError> {
    ///     // do something
    /// }
    ///
    /// let my_func_type = FunctionType::new(vec![Type::I32], vec![Type::I32]);
    /// let imports = imports!{
    ///     "namespace" => {
    ///         "my_func" => Function::new(&store, my_func_type, my_func),
    ///     }
    /// }
    /// ```
    ///
    /// Note: Dynamic function trampolines are only compiled for imported function types.
    pub dynamic_function_trampolines: PrimaryMap<FunctionIndex, FunctionBody>,

    /// Section ids corresponding to the unwind information.
    pub unwind_info: UnwindInfo,

    /// A reference to the [`GOT`] instance for the compilation.
    pub got: GOT,
}
