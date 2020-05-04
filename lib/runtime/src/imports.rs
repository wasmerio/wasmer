use crate::vmcontext::{VMFunctionImport, VMGlobalImport, VMMemoryImport, VMTableImport};
use std::collections::HashSet;
use wasm_common::entity::{BoxedSlice, PrimaryMap};
use wasm_common::{FuncIndex, GlobalIndex, MemoryIndex, TableIndex};

/// Resolved import pointers.
#[derive(Clone)]
pub struct Imports {
    /// Resolved addresses for imported functions.
    pub functions: BoxedSlice<FuncIndex, VMFunctionImport>,

    /// Resolved addresses for imported tables.
    pub tables: BoxedSlice<TableIndex, VMTableImport>,

    /// Resolved addresses for imported memories.
    pub memories: BoxedSlice<MemoryIndex, VMMemoryImport>,

    /// Resolved addresses for imported globals.
    pub globals: BoxedSlice<GlobalIndex, VMGlobalImport>,
}

impl Imports {
    /// Construct a new `Imports` instance.
    pub fn new(
        function_imports: PrimaryMap<FuncIndex, VMFunctionImport>,
        table_imports: PrimaryMap<TableIndex, VMTableImport>,
        memory_imports: PrimaryMap<MemoryIndex, VMMemoryImport>,
        global_imports: PrimaryMap<GlobalIndex, VMGlobalImport>,
    ) -> Self {
        Self {
            functions: function_imports.into_boxed_slice(),
            tables: table_imports.into_boxed_slice(),
            memories: memory_imports.into_boxed_slice(),
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
        }
    }
}
