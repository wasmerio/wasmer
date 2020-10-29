// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

use crate::vmcontext::{VMFunctionImport, VMGlobalImport, VMMemoryImport, VMTableImport};
use wasmer_types::entity::{BoxedSlice, PrimaryMap};
use wasmer_types::{FunctionIndex, GlobalIndex, MemoryIndex, TableIndex};

/// Resolved import pointers.
#[derive(Clone)]
pub struct Imports {
    /// Resolved addresses for imported functions.
    pub functions: BoxedSlice<FunctionIndex, VMFunctionImport>,

    /// Initializers for host function environments. This is split out from `functions`
    /// because the generated code never needs to touch this and the extra wasted
    /// space may affect Wasm runtime performance due to increased cache pressure.
    pub host_function_env_initializers: BoxedSlice<
        FunctionIndex,
        Option<
            fn(*mut std::ffi::c_void, *const std::ffi::c_void) -> Result<(), *mut std::ffi::c_void>,
        >,
    >,

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
        function_imports: PrimaryMap<FunctionIndex, VMFunctionImport>,
        host_function_env_initializers: PrimaryMap<
            FunctionIndex,
            Option<
                fn(
                    *mut std::ffi::c_void,
                    *const std::ffi::c_void,
                ) -> Result<(), *mut std::ffi::c_void>,
            >,
        >,
        table_imports: PrimaryMap<TableIndex, VMTableImport>,
        memory_imports: PrimaryMap<MemoryIndex, VMMemoryImport>,
        global_imports: PrimaryMap<GlobalIndex, VMGlobalImport>,
    ) -> Self {
        Self {
            functions: function_imports.into_boxed_slice(),
            host_function_env_initializers: host_function_env_initializers.into_boxed_slice(),
            tables: table_imports.into_boxed_slice(),
            memories: memory_imports.into_boxed_slice(),
            globals: global_imports.into_boxed_slice(),
        }
    }

    /// Construct a new `Imports` instance with no imports.
    pub fn none() -> Self {
        Self {
            functions: PrimaryMap::new().into_boxed_slice(),
            host_function_env_initializers: PrimaryMap::new().into_boxed_slice(),
            tables: PrimaryMap::new().into_boxed_slice(),
            memories: PrimaryMap::new().into_boxed_slice(),
            globals: PrimaryMap::new().into_boxed_slice(),
        }
    }
}
