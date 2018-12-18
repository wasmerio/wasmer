use crate::webassembly::vmoffsets::VMOffsets;
use cranelift_wasm::{
    DefinedFuncIndex, DefinedGlobalIndex, DefinedMemoryIndex, DefinedTableIndex, FuncIndex,
    GlobalIndex, MemoryIndex, SignatureIndex, TableIndex,
};

#[repr(C)]
pub struct VMContext {
    /// A pointer to an array of imported functions, indexed by `FuncIndex`.
    imported_functions: *const *const VMFunctionBody,

    /// A pointer to an array of imported tables, indexed by `TableIndex`.
    imported_tables: *mut VMTableImport,

    /// A pointer to an array of imported memories, indexed by `MemoryIndex,
    imported_memories: *mut VMMemoryImport,

    /// A pointer to an array of imported globals, indexed by `GlobalIndex`.
    imported_globals: *mut VMGlobalImport,

    /// A pointer to an array of locally-defined tables, indexed by `DefinedTableIndex`.
    tables: *mut VMTableDefinition,

    /// A pointer to an array of locally-defined memories, indexed by `DefinedMemoryIndex`.
    memories: *mut VMMemoryDefinition,

    /// A pointer to an array of locally-defined globals, indexed by ``DefinedGlobalIndex`.
    globals: *mut VMGlobalDefinition,

    /// Signature identifiers for signature-checked indirect calls.
    signature_ids: *mut VMSharedSigIndex,
}

/// Used to provide type safety for passing around function pointers.
/// The typesystem ensures this cannot be dereferenced.
pub enum VMFunctionBody {}

/// Definition of a table used by the VM. (obviously)
#[repr(C)]
pub struct VMTableDefinition {
    /// pointer to the elements in the table.
    pub base: *mut u8,
    /// Number of elements in the table (NOT necessarily the size of the table in bytes!).
    pub current_elements: usize,
}

impl VMTableDefinition {
    pub fn offset_base(offsets: &VMOffsets) -> u8 {
        0 * offsets.ptr_size
    }

    pub fn offset_current_elements(offsets: &VMOffsets) -> u8 {
        1 * offsets.ptr_size
    }
}

#[repr(C)]
pub struct VMTableImport {
    /// A pointer to the table definition.
    pub table: *mut VMTableDefinition,
    /// A pointer to the vmcontext that owns this table definition.
    pub vmctx: *mut VMContext,
}

impl VMTableImport {
    pub fn offset_table(offsets: &VMOffsets) -> u8 {
        0 * offsets.ptr_size
    }

    pub fn offset_vmctx(offsets: &VMOffsets) -> u8 {
        1 * offsets.ptr_size
    }
}

/// Definition of a memory used by the VM.
#[repr(C)]
pub struct VMMemoryDefinition {
    /// Pointer to the bottom of linear memory.
    pub base: *mut u8,
    /// Current logical size of this linear memory in bytes.
    pub size: usize,
}

impl VMMemoryDefinition {
    pub fn offset_base(offsets: &VMOffsets) -> u8 {
        0 * offsets.ptr_size
    }

    pub fn offset_size(offsets: &VMOffsets) -> u8 {
        1 * offsets.ptr_size
    }
}

#[repr(C)]
pub struct VMMemoryImport {
    /// A pointer to the memory definition.
    pub memory: *mut VMMemoryDefinition,
    /// A pointer to the vmcontext that owns this memory definition.
    pub vmctx: *mut VMContext,
}

impl VMMemoryImport {
    pub fn offset_memory(offsets: &VMOffsets) -> u8 {
        0 * offsets.ptr_size
    }

    pub fn offset_vmctx(offsets: &VMOffsets) -> u8 {
        1 * offsets.ptr_size
    }
}

/// Definition of a global used by the VM.
#[repr(C, align(8))]
pub struct VMGlobalDefinition {
    pub data: [u8; 8],
}

#[repr(C)]
pub struct VMGlobalImport {
    pub globals: *mut VMGlobalDefinition,
}

impl VMGlobalImport {
    pub fn offset_globals(offsets: &VMOffsets) -> u8 {
        0 * offsets.ptr_size
    }
}

#[repr(C)]
pub struct VMSharedSigIndex(u32);

#[repr(C)]
pub struct VMCallerCheckedAnyfunc {
    pub func: *const VMFunctionBody,
    pub type_index: VMSharedSigIndex,
    pub vmctx: *mut VMContext,
}

impl VMCallerCheckedAnyfunc {
    pub fn offset_func(offsets: &VMOffsets) -> u8 {
        0 * offsets.ptr_size
    }

    pub fn offset_type_index(offsets: &VMOffsets) -> u8 {
        1 * offsets.ptr_size
    }

    pub fn offset_vmctx(offsets: &VMOffsets) -> u8 {
        2 * offsets.ptr_size
    }
}
