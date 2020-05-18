//! A `Compilation` contains the compiled function bodies for a WebAssembly
//! module (`CompiledFunction`).
//!
//! The `CompiledFunction` will be used mainly by different frontends:
//! * `jit`: to generate a JIT
//! * `obj`: to generate a native object

use crate::section::{CustomSection, SectionBody, SectionIndex};
use crate::std::vec::Vec;
use crate::trap::TrapInformation;
use crate::{CompiledFunctionUnwindInfo, FunctionAddressMap, JumpTableOffsets, Relocation};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};

use wasm_common::entity::PrimaryMap;
use wasm_common::LocalFunctionIndex;

/// The frame info for a Compiled function.
///
/// This structure is only used for reconstructing
/// the frame information after a `Trap`.
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CompiledFunctionFrameInfo {
    /// The traps (in the function body)
    pub traps: Vec<TrapInformation>,

    /// The address map.
    pub address_map: FunctionAddressMap,
}

/// The function body.
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionBody {
    /// The function body bytes.
    #[cfg_attr(feature = "enable-serde", serde(with = "serde_bytes"))]
    pub body: Vec<u8>,

    /// The function unwind info
    pub unwind_info: Option<CompiledFunctionUnwindInfo>,
}

/// The result of compiling a WebAssembly function.
///
/// This structure only have the compiled information data
/// (function bytecode body, relocations, traps, jump tables
/// and unwind information).
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledFunction {
    /// The function body.
    pub body: FunctionBody,

    /// The relocations (in the body)
    pub relocations: Vec<Relocation>,

    /// The jump tables offsets (in the body).
    pub jt_offsets: JumpTableOffsets,

    /// The frame information.
    pub frame_info: CompiledFunctionFrameInfo,
}

/// The compiled functions map (index in the Wasm -> function)
pub type Functions = PrimaryMap<LocalFunctionIndex, CompiledFunction>;

/// The custom sections for a Compilation.
pub type CustomSections = PrimaryMap<SectionIndex, CustomSection>;

/// The result of compiling a WebAssembly module's functions.
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[derive(Debug, PartialEq, Eq)]
pub struct Compilation {
    /// Compiled code for the function bodies.
    functions: Functions,
    /// Custom sections for the module.
    /// It will hold the data, for example, for constants used in a
    /// function, global variables, rodata_64, hot/cold function partitioning, ...
    custom_sections: CustomSections,
}

impl Compilation {
    /// Creates a compilation artifact from a contiguous function buffer and a set of ranges
    pub fn new(functions: Functions, custom_sections: CustomSections) -> Self {
        Self {
            functions,
            custom_sections,
        }
    }

    /// Gets the bytes of a single function
    pub fn get(&self, func: LocalFunctionIndex) -> &CompiledFunction {
        &self.functions[func]
    }

    /// Gets the number of functions defined.
    pub fn len(&self) -> usize {
        self.functions.len()
    }

    /// Returns whether there are no functions defined.
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }

    /// Gets functions relocations.
    pub fn get_relocations(&self) -> PrimaryMap<LocalFunctionIndex, Vec<Relocation>> {
        self.functions
            .iter()
            .map(|(_, func)| func.relocations.clone())
            .collect::<PrimaryMap<LocalFunctionIndex, _>>()
    }

    /// Gets functions bodies.
    pub fn get_function_bodies(&self) -> PrimaryMap<LocalFunctionIndex, FunctionBody> {
        self.functions
            .iter()
            .map(|(_, func)| func.body.clone())
            .collect::<PrimaryMap<LocalFunctionIndex, _>>()
    }

    /// Gets functions jump table offsets.
    pub fn get_jt_offsets(&self) -> PrimaryMap<LocalFunctionIndex, JumpTableOffsets> {
        self.functions
            .iter()
            .map(|(_, func)| func.jt_offsets.clone())
            .collect::<PrimaryMap<LocalFunctionIndex, _>>()
    }

    /// Gets functions frame info.
    pub fn get_frame_info(&self) -> PrimaryMap<LocalFunctionIndex, CompiledFunctionFrameInfo> {
        self.functions
            .iter()
            .map(|(_, func)| func.frame_info.clone())
            .collect::<PrimaryMap<LocalFunctionIndex, _>>()
    }

    /// Gets custom section data.
    pub fn get_custom_sections(&self) -> PrimaryMap<SectionIndex, SectionBody> {
        self.custom_sections
            .iter()
            .map(|(_, section)| section.bytes.clone())
            .collect::<PrimaryMap<SectionIndex, _>>()
    }

    /// Gets relocations that apply to custom sections.
    pub fn get_custom_section_relocations(&self) -> PrimaryMap<SectionIndex, Vec<Relocation>> {
        self.custom_sections
            .iter()
            .map(|(_, section)| section.relocations.clone())
            .collect::<PrimaryMap<SectionIndex, _>>()
    }
}

impl<'a> IntoIterator for &'a Compilation {
    type IntoIter = Iter<'a>;
    type Item = <Self::IntoIter as Iterator>::Item;

    fn into_iter(self) -> Self::IntoIter {
        Iter {
            iterator: self.functions.iter(),
        }
    }
}

pub struct Iter<'a> {
    iterator: <&'a Functions as IntoIterator>::IntoIter,
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a CompiledFunction;

    fn next(&mut self) -> Option<Self::Item> {
        self.iterator.next().map(|(_, b)| b)
    }
}
