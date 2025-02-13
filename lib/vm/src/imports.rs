// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

use crate::vmcontext::{VMFunctionImport, VMGlobalImport, VMMemoryImport, VMTableImport};
use crate::VMTagImport;
use wasmer_types::entity::{BoxedSlice, PrimaryMap};
use wasmer_types::{FunctionIndex, GlobalIndex, MemoryIndex, TableIndex, TagIndex};

/// Resolved import pointers.
#[derive(Clone)]
pub struct Imports {
    /// Resolved addresses for imported functions.
    pub functions: BoxedSlice<FunctionIndex, VMFunctionImport>,

    /// Resolved addresses for imported tables.
    pub tables: BoxedSlice<TableIndex, VMTableImport>,

    /// Resolved addresses for imported memories.
    pub memories: BoxedSlice<MemoryIndex, VMMemoryImport>,

    /// Resolved addresses for imported memories.
    pub tags: BoxedSlice<TagIndex, VMTagImport>,

    /// Resolved addresses for imported globals.
    pub globals: BoxedSlice<GlobalIndex, VMGlobalImport>,
}

impl Imports {
    /// Construct a new `Imports` instance.
    pub fn new(
        function_imports: PrimaryMap<FunctionIndex, VMFunctionImport>,
        table_imports: PrimaryMap<TableIndex, VMTableImport>,
        memory_imports: PrimaryMap<MemoryIndex, VMMemoryImport>,
        tag_imports: PrimaryMap<TagIndex, VMTagImport>,
        global_imports: PrimaryMap<GlobalIndex, VMGlobalImport>,
    ) -> Self {
        Self {
            functions: function_imports.into_boxed_slice(),
            tables: table_imports.into_boxed_slice(),
            memories: memory_imports.into_boxed_slice(),
            tags: tag_imports.into_boxed_slice(),
            globals: global_imports.into_boxed_slice(),
        }
    }

    /// Construct a new `Imports` instance with no imports.
    pub fn none() -> Self {
        Self {
            functions: PrimaryMap::new().into_boxed_slice(),
            tables: PrimaryMap::new().into_boxed_slice(),
            memories: PrimaryMap::new().into_boxed_slice(),
            globals: PrimaryMap::new().into_boxed_slice(),
            tags: PrimaryMap::new().into_boxed_slice(),
        }
    }
}
