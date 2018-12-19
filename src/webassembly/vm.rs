use std::{ptr, mem};
use crate::common::slice::IndexedSlice;
use cranelift_wasm::{
    TableIndex, MemoryIndex, GlobalIndex, FuncIndex,
    DefinedTableIndex, DefinedMemoryIndex, DefinedGlobalIndex,
    SignatureIndex,
};

#[derive(Debug)]
#[repr(C)]
pub struct VmCtx {
    /// A pointer to an array of locally-defined memories, indexed by `DefinedMemoryIndex`.
    pub(in crate::webassembly) memories: IndexedSlice<LocalMemory, DefinedMemoryIndex>,

    /// A pointer to an array of locally-defined tables, indexed by `DefinedTableIndex`.
    pub(in crate::webassembly) tables: IndexedSlice<LocalTable, DefinedTableIndex>,

    /// A pointer to an array of locally-defined globals, indexed by `DefinedGlobalIndex`.
    pub(in crate::webassembly) globals: IndexedSlice<LocalGlobal, DefinedGlobalIndex>,

    /// A pointer to an array of imported memories, indexed by `MemoryIndex,
    pub(in crate::webassembly) imported_memories: IndexedSlice<ImportedMemory, MemoryIndex>,

    /// A pointer to an array of imported tables, indexed by `TableIndex`.
    pub(in crate::webassembly) imported_tables: IndexedSlice<ImportedTable, TableIndex>,

    /// A pointer to an array of imported globals, indexed by `GlobalIndex`.
    pub(in crate::webassembly) imported_globals: IndexedSlice<ImportedGlobal, GlobalIndex>,
    
    /// A pointer to an array of imported functions, indexed by `FuncIndex`.
    pub(in crate::webassembly) imported_funcs: IndexedSlice<ImportedFunc, FuncIndex>,

    /// Signature identifiers for signature-checked indirect calls.
    pub(in crate::webassembly) sig_ids: IndexedSlice<SigId, SignatureIndex>,
}

impl VmCtx {
    pub fn new(
        memories: *mut LocalMemory,
        tables: *mut LocalTable,
        globals: *mut LocalGlobal,
        imported_memories: *mut ImportedMemory,
        imported_tables: *mut ImportedTable,
        imported_globals: *mut ImportedGlobal,
        imported_funcs: *mut ImportedFunc,
        sig_ids: *mut SigId,
    ) -> Self {
        Self {
            memories: IndexedSlice::new(memories),
            tables: IndexedSlice::new(tables),
            globals: IndexedSlice::new(globals),
            imported_memories: IndexedSlice::new(imported_memories),
            imported_tables: IndexedSlice::new(imported_tables),
            imported_globals: IndexedSlice::new(imported_globals),
            imported_funcs: IndexedSlice::new(imported_funcs),
            sig_ids: IndexedSlice::new(sig_ids),
        }
    }

    pub fn offset_memories() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_tables() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_globals() -> u8 {
        2 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_imported_memories() -> u8 {
        3 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_imported_tables() -> u8 {
        4 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_imported_globals() -> u8 {
        5 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_imported_funcs() -> u8 {
        6 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_sig_ids() -> u8 {
        7 * (mem::size_of::<usize>() as u8)
    }
}

/// Used to provide type safety (ish) for passing around function pointers.
/// The typesystem ensures this cannot be dereferenced since an
/// empty enum cannot actually exist.
pub enum Func {}

/// An imported function, which contains the vmctx that owns this function.
#[derive(Debug, Clone)]
#[repr(C)]
pub struct ImportedFunc {
    pub func: *const Func,
    pub vmctx: *mut VmCtx,
}

impl ImportedFunc {
    pub fn offset_func() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_vmctx() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }
}

/// Definition of a table used by the VM. (obviously)
#[derive(Debug, Clone)]
#[repr(C)]
pub struct LocalTable {
    /// pointer to the elements in the table.
    pub base: *mut u8,
    /// Number of elements in the table (NOT necessarily the size of the table in bytes!).
    pub current_elements: usize,
}

impl LocalTable {
    pub fn offset_base() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_current_elements() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct ImportedTable {
    /// A pointer to the table definition.
    pub table: *mut LocalTable,
    /// A pointer to the vmcontext that owns this table definition.
    pub vmctx: *mut VmCtx,
}

impl ImportedTable {
    pub fn offset_table() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_vmctx() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }
}

/// Definition of a memory used by the VM.
#[derive(Debug, Clone)]
#[repr(C)]
pub struct LocalMemory {
    /// Pointer to the bottom of linear memory.
    pub base: *mut u8,
    /// Current logical size of this linear memory in bytes.
    pub size: usize,
}

impl LocalMemory {
    pub fn offset_base() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_size() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct ImportedMemory {
    /// A pointer to the memory definition.
    pub memory: *mut LocalMemory,
}

impl ImportedMemory {
    pub fn offset_memory() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }
}

/// Definition of a global used by the VM.
#[derive(Debug, Clone)]
#[repr(C)]
pub struct LocalGlobal {
    pub data: [u8; 8],
}

impl LocalGlobal {
    pub fn offset_data() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct ImportedGlobal {
    pub global: *mut LocalGlobal,
}

impl ImportedGlobal {
    pub fn offset_global() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }
}

#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct SigId(u32);

/// Caller-checked anyfunc
#[derive(Debug, Clone)]
#[repr(C)]
pub struct CCAnyfunc {
    pub func_data: ImportedFunc,
    pub sig_id: SigId,
}

impl CCAnyfunc {
    pub fn null() -> Self {
        Self {
            func_data: ImportedFunc {
                func: ptr::null(),
                vmctx: ptr::null_mut(),
            },
            sig_id: SigId(u32::max_value()),
        }
    }

    pub fn offset_func() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_vmctx() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_sig_id() -> u8 {
        2 * (mem::size_of::<usize>() as u8)
    }
}

#[cfg(test)]
mod vm_offset_tests {
    use super::{
        VmCtx,
        ImportedFunc,
        LocalTable,
        ImportedTable,
        LocalMemory,
        ImportedMemory,
        LocalGlobal,
        ImportedGlobal,
        CCAnyfunc,
    };

    #[test]
    fn vmctx() {
        assert_eq!(
            VmCtx::offset_memories() as usize,
            offset_of!(VmCtx => memories).get_byte_offset(),
        );

        assert_eq!(
            VmCtx::offset_tables() as usize,
            offset_of!(VmCtx => tables).get_byte_offset(),
        );

        assert_eq!(
            VmCtx::offset_globals() as usize,
            offset_of!(VmCtx => globals).get_byte_offset(),
        );

        assert_eq!(
            VmCtx::offset_imported_memories() as usize,
            offset_of!(VmCtx => imported_memories).get_byte_offset(),
        );

        assert_eq!(
            VmCtx::offset_imported_tables() as usize,
            offset_of!(VmCtx => imported_tables).get_byte_offset(),
        );

        assert_eq!(
            VmCtx::offset_imported_globals() as usize,
            offset_of!(VmCtx => imported_globals).get_byte_offset(),
        );

        assert_eq!(
            VmCtx::offset_imported_funcs() as usize,
            offset_of!(VmCtx => imported_funcs).get_byte_offset(),
        );

        assert_eq!(
            VmCtx::offset_sig_ids() as usize,
            offset_of!(VmCtx => sig_ids).get_byte_offset(),
        );
    }

    #[test]
    fn imported_func() {
        assert_eq!(
            ImportedFunc::offset_func() as usize,
            offset_of!(ImportedFunc => func).get_byte_offset(),
        );

        assert_eq!(
            ImportedFunc::offset_vmctx() as usize,
            offset_of!(ImportedFunc => vmctx).get_byte_offset(),
        );
    }

    #[test]
    fn local_table() {
        assert_eq!(
            LocalTable::offset_base() as usize,
            offset_of!(LocalTable => base).get_byte_offset(),
        );

        assert_eq!(
            LocalTable::offset_current_elements() as usize,
            offset_of!(LocalTable => current_elements).get_byte_offset(),
        );
    }

    #[test]
    fn imported_table() {
        assert_eq!(
            ImportedTable::offset_table() as usize,
            offset_of!(ImportedTable => table).get_byte_offset(),
        );

        assert_eq!(
            ImportedTable::offset_vmctx() as usize,
            offset_of!(ImportedTable => vmctx).get_byte_offset(),
        );
    }

    #[test]
    fn local_memory() {
        assert_eq!(
            LocalMemory::offset_base() as usize,
            offset_of!(LocalMemory => base).get_byte_offset(),
        );

        assert_eq!(
            LocalMemory::offset_size() as usize,
            offset_of!(LocalMemory => size).get_byte_offset(),
        );
    }

    #[test]
    fn imported_memory() {
        assert_eq!(
            ImportedMemory::offset_memory() as usize,
            offset_of!(ImportedMemory => memory).get_byte_offset(),
        );
    }

    #[test]
    fn local_global() {
        assert_eq!(
            LocalGlobal::offset_data() as usize,
            offset_of!(LocalGlobal => data).get_byte_offset(),
        );
    }

    #[test]
    fn imported_global() {
        assert_eq!(
            ImportedGlobal::offset_global() as usize,
            offset_of!(ImportedGlobal => global).get_byte_offset(),
        );
    }

    #[test]
    fn cc_anyfunc() {
        assert_eq!(
            CCAnyfunc::offset_func() as usize,
            offset_of!(CCAnyfunc => func_data: ImportedFunc => func).get_byte_offset(),
        );

        assert_eq!(
            CCAnyfunc::offset_vmctx() as usize,
            offset_of!(CCAnyfunc => func_data: ImportedFunc => vmctx).get_byte_offset(),
        );

        assert_eq!(
            CCAnyfunc::offset_sig_id() as usize,
            offset_of!(CCAnyfunc => sig_id).get_byte_offset(),
        );
    }
}