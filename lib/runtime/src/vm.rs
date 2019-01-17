pub use crate::{
    backing::{ImportBacking, LocalBacking},
    types::LocalMemoryIndex,
};
use std::{mem, ptr};

#[derive(Debug)]
#[repr(C)]
pub struct Ctx {
    /// A pointer to an array of locally-defined memories, indexed by `MemoryIndex`.
    pub(crate) memories: *mut LocalMemory,

    /// A pointer to an array of locally-defined tables, indexed by `TableIndex`.
    pub(crate) tables: *mut LocalTable,

    /// A pointer to an array of locally-defined globals, indexed by `GlobalIndex`.
    pub(crate) globals: *mut LocalGlobal,

    /// A pointer to an array of imported memories, indexed by `MemoryIndex,
    pub(crate) imported_memories: *mut ImportedMemory,

    /// A pointer to an array of imported tables, indexed by `TableIndex`.
    pub(crate) imported_tables: *mut ImportedTable,

    /// A pointer to an array of imported globals, indexed by `GlobalIndex`.
    pub(crate) imported_globals: *mut ImportedGlobal,

    /// A pointer to an array of imported functions, indexed by `FuncIndex`.
    pub(crate) imported_funcs: *mut ImportedFunc,

    /// The parent instance.
    pub local_backing: *mut LocalBacking,
    pub import_backing: *mut ImportBacking,
}

impl Ctx {
    pub unsafe fn new(
        local_backing: &mut LocalBacking,
        import_backing: &mut ImportBacking,
    ) -> Self {
        Self {
            memories: local_backing.vm_memories.as_mut_ptr(),
            tables: local_backing.vm_tables.as_mut_ptr(),
            globals: local_backing.vm_globals.as_mut_ptr(),

            imported_memories: import_backing.memories.as_mut_ptr(),
            imported_tables: import_backing.tables.as_mut_ptr(),
            imported_globals: import_backing.globals.as_mut_ptr(),
            imported_funcs: import_backing.functions.as_mut_ptr(),

            local_backing,
            import_backing,
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

    pub fn offset_signatures() -> u8 {
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
    pub vmctx: *mut Ctx,
}

impl ImportedFunc {
    pub fn offset_func() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_vmctx() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }

    pub fn size() -> u8 {
        mem::size_of::<Self>() as u8
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
    /// The number of elements that can fit into the memory allocated for this table.
    pub capacity: usize,
}

impl LocalTable {
    pub fn offset_base() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_current_elements() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }

    pub fn size() -> u8 {
        mem::size_of::<Self>() as u8
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct ImportedTable {
    /// A pointer to the table definition.
    pub table: *mut LocalTable,
    /// A pointer to the vmcontext that owns this table definition.
    pub vmctx: *mut Ctx,
}

impl ImportedTable {
    pub fn offset_table() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_vmctx() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }

    pub fn size() -> u8 {
        mem::size_of::<Self>() as u8
    }
}

/// Definition of a memory used by the VM.
#[derive(Debug, Clone)]
#[repr(C)]
pub struct LocalMemory {
    /// Pointer to the bottom of this linear memory.
    pub base: *mut u8,
    /// Current size of this linear memory in bytes.
    pub size: usize,
    /// The local memory index.
    pub index: LocalMemoryIndex,
}

impl LocalMemory {
    pub fn offset_base() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_size() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }

    pub fn size() -> u8 {
        mem::size_of::<Self>() as u8
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct ImportedMemory {
    /// A pointer to the memory definition.
    pub memory: *mut LocalMemory,
    pub vmctx: *mut Ctx,
}

impl ImportedMemory {
    pub fn offset_memory() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_vmctx() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }

    pub fn size() -> u8 {
        mem::size_of::<Self>() as u8
    }
}

/// Definition of a global used by the VM.
#[derive(Debug, Clone)]
#[repr(C)]
pub struct LocalGlobal {
    pub data: u64,
}

impl LocalGlobal {
    pub fn offset_data() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    pub fn null() -> Self {
        Self { data: 0 }
    }

    pub fn size() -> u8 {
        mem::size_of::<Self>() as u8
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

    pub fn size() -> u8 {
        mem::size_of::<Self>() as u8
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct SigId(pub u32);

/// Caller-checked anyfunc
#[derive(Debug, Clone)]
#[repr(C)]
pub struct Anyfunc {
    pub func_data: ImportedFunc,
    pub sig_id: SigId,
}

impl Anyfunc {
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

    pub fn size() -> u8 {
        mem::size_of::<Self>() as u8
    }
}

#[cfg(test)]
mod vm_offset_tests {
    use super::{
        Anyfunc, Ctx, ImportedFunc, ImportedGlobal, ImportedMemory, ImportedTable, LocalGlobal,
        LocalMemory, LocalTable,
    };

    #[test]
    fn vmctx() {
        assert_eq!(
            Ctx::offset_memories() as usize,
            offset_of!(Ctx => memories).get_byte_offset(),
        );

        assert_eq!(
            Ctx::offset_tables() as usize,
            offset_of!(Ctx => tables).get_byte_offset(),
        );

        assert_eq!(
            Ctx::offset_globals() as usize,
            offset_of!(Ctx => globals).get_byte_offset(),
        );

        assert_eq!(
            Ctx::offset_imported_memories() as usize,
            offset_of!(Ctx => imported_memories).get_byte_offset(),
        );

        assert_eq!(
            Ctx::offset_imported_tables() as usize,
            offset_of!(Ctx => imported_tables).get_byte_offset(),
        );

        assert_eq!(
            Ctx::offset_imported_globals() as usize,
            offset_of!(Ctx => imported_globals).get_byte_offset(),
        );

        assert_eq!(
            Ctx::offset_imported_funcs() as usize,
            offset_of!(Ctx => imported_funcs).get_byte_offset(),
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

        assert_eq!(
            ImportedMemory::offset_vmctx() as usize,
            offset_of!(ImportedMemory => vmctx).get_byte_offset(),
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
            Anyfunc::offset_func() as usize,
            offset_of!(Anyfunc => func_data: ImportedFunc => func).get_byte_offset(),
        );

        assert_eq!(
            Anyfunc::offset_vmctx() as usize,
            offset_of!(Anyfunc => func_data: ImportedFunc => vmctx).get_byte_offset(),
        );

        assert_eq!(
            Anyfunc::offset_sig_id() as usize,
            offset_of!(Anyfunc => sig_id).get_byte_offset(),
        );
    }
}
