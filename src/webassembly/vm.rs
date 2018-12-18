
use cranelift_wasm::{
    TableIndex, FuncIndex, MemoryIndex, GlobalIndex,
    DefinedTableIndex, DefinedFuncIndex, DefinedMemoryIndex, DefinedGlobalIndex,
    SignatureIndex,
};

#[repr(C)]
pub struct VmCtx {
    /// A pointer to an array of imported functions, indexed by `FuncIndex`.
    imported_funcs: *const *const Function,

    /// A pointer to an array of imported tables, indexed by `TableIndex`.
    imported_tables: *mut ImportedTable,

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

/// Used to provide type safety (ish) for passing around function pointers.
/// The typesystem ensures this cannot be dereferenced since an
/// empty enum cannot actually exist.
pub enum Function { }

/// Definition of a table used by the VM. (obviously)
#[repr(C)]
pub struct LocalTable {
    /// pointer to the elements in the table.
    pub base: *mut u8,
    /// Number of elements in the table (NOT necessarily the size of the table in bytes!).
    pub current_elements: usize,
}

impl LocalTable {
    pub fn offset_base(offsets: &Offsets) -> u8 {
        0 * offsets.ptr_size
    }

    pub fn offset_current_elements(offsets: &Offsets) -> u8 {
        1 * offsets.ptr_size
    }
}

#[repr(C)]
pub struct ImportedTable {
    /// A pointer to the table definition.
    pub table: *mut LocalTable,
    /// A pointer to the vmcontext that owns this table definition.
    pub vmctx: *mut VmCtx,
}

impl ImportedTable {
    pub fn offset_table(offsets: &Offsets) -> u8 {
        0 * offsets.ptr_size
    }

    pub fn offset_vmctx(offsets: &Offsets) -> u8 {
        1 * offsets.ptr_size
    }
}

/// Definition of a memory used by the VM.
#[repr(C)]
pub struct LocalMemory {
    /// Pointer to the bottom of linear memory.
    pub base: *mut u8,
    /// Current logical size of this linear memory in bytes.
    pub size: usize,
}

impl LocalMemory {
    pub fn offset_base(offsets: &Offsets) -> u8 {
        0 * offsets.ptr_size
    }

    pub fn offset_size(offsets: &Offsets) -> u8 {
        1 * offsets.ptr_size
    }
}

#[repr(C)]
pub struct ImportedMemory {
    /// A pointer to the memory definition.
    pub memory: *mut LocalMemory,
}

impl ImportedMemory {
    pub fn offset_memory(offsets: &Offsets) -> u8 {
        0 * offsets.ptr_size
    }
}

/// Definition of a global used by the VM.
#[repr(C, align(8))]
pub struct LocalGlobal {
    pub data: [u8; 8],
}

#[repr(C)]
pub struct ImportedGlobal {
    pub globals: *mut LocalGlobal,
}

impl ImportedGlobal {
    pub fn offset_globals(offsets: &Offsets) -> u8 {
        0 * offsets.ptr_size
    }
}

#[repr(C)]
pub struct SigId(u32);

#[repr(C)]
pub struct CallerCheckedAnyfunc {
    pub func: *const VMFunctionBody,
    pub sig: SigId,
    pub vmctx: *mut VmCtx,
}

impl VMCallerCheckedAnyfunc {
    pub fn offset_func(offsets: &Offsets) -> u8 {
        0 * offsets.ptr_size
    }

    pub fn offset_type_index(offsets: &Offsets) -> u8 {
        1 * offsets.ptr_size
    }

    pub fn offset_vmctx(offsets: &Offsets) -> u8 {
        2 * offsets.ptr_size
    }
}

#[derive(Copy, Clone, )]
pub struct Offsets {
    ptr_size: u8,
}

impl Offsets {
    pub fn new(ptr_size: u8) -> Self {
        Self {
            ptr_size,
        }
    }
}