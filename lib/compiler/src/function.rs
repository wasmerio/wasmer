//! A `Compilation` contains the compiled function bodies for a WebAssembly
//! module (`CompiledFunction`).
//!
//! The `CompiledFunction` will be used mainly by different frontends:
//! * `jit`: to generate a JIT
//! * `obj`: to generate a native object

use crate::std::ops::Range;
use crate::std::vec::Vec;
use crate::trap::TrapInformation;
use crate::{CompiledFunctionUnwindInfo, FunctionAddressMap, JumpTableOffsets, Relocation};
use serde::{Deserialize, Serialize};

use wasm_common::entity::PrimaryMap;
use wasm_common::LocalFuncIndex;

type FunctionBody = Vec<u8>;

/// The result of compiling a WebAssembly function.
///
/// This structure only have the compiled information data
/// (function bytecode body, relocations, traps, jump tables
/// and unwind information).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct CompiledFunction {
    /// The function body.
    #[serde(with = "serde_bytes")]
    pub body: FunctionBody,

    /// The relocations (in the body)
    pub relocations: Vec<Relocation>,

    /// The traps (in the body)
    pub traps: Vec<TrapInformation>,

    /// The jump tables offsets (in the body).
    pub jt_offsets: JumpTableOffsets,

    /// The unwind information.
    pub unwind_info: CompiledFunctionUnwindInfo,

    /// The address map.
    ///
    /// TODO: Make it optional as it's not required for trampoline generation.
    pub address_map: FunctionAddressMap,
}

/// The compiled functions map (index in the Wasm -> function)
pub type Functions = PrimaryMap<LocalFuncIndex, CompiledFunction>;

/// The result of compiling a WebAssembly module's functions.
#[derive(Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct Compilation {
    /// Compiled code for the function bodies.
    functions: Functions,
}

impl Compilation {
    /// Creates a compilation artifact from a contiguous function buffer and a set of ranges
    pub fn new(functions: Functions) -> Self {
        Self { functions }
    }

    /// Allocates the compilation result with the given function bodies.
    pub fn from_buffer(
        buffer: Vec<u8>,
        functions: impl IntoIterator<
            Item = (
                Range<usize>,
                JumpTableOffsets,
                Range<usize>,
                Vec<Relocation>,
                Vec<TrapInformation>,
                FunctionAddressMap,
            ),
        >,
    ) -> Self {
        Self::new(
            functions
                .into_iter()
                .map(
                    |(body_range, jt_offsets, unwind_range, relocations, traps, address_map)| {
                        CompiledFunction {
                            body: buffer[body_range].to_vec(),
                            jt_offsets,
                            unwind_info: CompiledFunctionUnwindInfo::Windows(
                                buffer[unwind_range].to_vec(),
                            ),
                            address_map,
                            relocations,
                            traps,
                        }
                    },
                )
                .collect(),
        )
    }

    /// Gets the bytes of a single function
    pub fn get(&self, func: LocalFuncIndex) -> &CompiledFunction {
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

    /// Gets functions jump table offsets.
    pub fn get_jt_offsets(&self) -> PrimaryMap<LocalFuncIndex, JumpTableOffsets> {
        self.functions
            .iter()
            .map(|(_, func)| func.jt_offsets.clone())
            .collect::<PrimaryMap<LocalFuncIndex, _>>()
    }

    /// Gets functions jump table offsets.
    pub fn get_relocations(&self) -> PrimaryMap<LocalFuncIndex, Vec<Relocation>> {
        self.functions
            .iter()
            .map(|(_, func)| func.relocations.clone())
            .collect::<PrimaryMap<LocalFuncIndex, _>>()
    }

    /// Gets functions address maps.
    pub fn get_address_maps(&self) -> PrimaryMap<LocalFuncIndex, FunctionAddressMap> {
        self.functions
            .iter()
            .map(|(_, func)| func.address_map.clone())
            .collect::<PrimaryMap<LocalFuncIndex, _>>()
    }

    /// Gets functions jump table offsets.
    pub fn get_traps(&self) -> PrimaryMap<LocalFuncIndex, Vec<TrapInformation>> {
        self.functions
            .iter()
            .map(|(_, func)| func.traps.clone())
            .collect::<PrimaryMap<LocalFuncIndex, _>>()
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
