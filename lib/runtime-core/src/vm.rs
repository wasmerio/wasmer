pub use crate::backing::{ImportBacking, LocalBacking};
use crate::{
    memory::Memory,
    module::ModuleInner,
    structures::TypedIndex,
    types::{LocalOrImport, MemoryIndex},
};
use std::{ffi::c_void, mem, ptr};

/// The context of the currently running WebAssembly instance.
///
///
#[derive(Debug)]
#[repr(C)]
pub struct Ctx {
    /// A pointer to an array of locally-defined memories, indexed by `MemoryIndex`.
    pub(crate) memories: *mut *mut LocalMemory,

    /// A pointer to an array of locally-defined tables, indexed by `TableIndex`.
    pub(crate) tables: *mut *mut LocalTable,

    /// A pointer to an array of locally-defined globals, indexed by `GlobalIndex`.
    pub(crate) globals: *mut *mut LocalGlobal,

    /// A pointer to an array of imported memories, indexed by `MemoryIndex,
    pub(crate) imported_memories: *mut *mut LocalMemory,

    /// A pointer to an array of imported tables, indexed by `TableIndex`.
    pub(crate) imported_tables: *mut *mut LocalTable,

    /// A pointer to an array of imported globals, indexed by `GlobalIndex`.
    pub(crate) imported_globals: *mut *mut LocalGlobal,

    /// A pointer to an array of imported functions, indexed by `FuncIndex`.
    pub(crate) imported_funcs: *mut ImportedFunc,

    local_backing: *mut LocalBacking,
    import_backing: *mut ImportBacking,
    module: *const ModuleInner,

    pub data: *mut c_void,
    pub data_finalizer: Option<extern "C" fn(data: *mut c_void)>,
}

impl Ctx {
    #[doc(hidden)]
    pub unsafe fn new(
        local_backing: &mut LocalBacking,
        import_backing: &mut ImportBacking,
        module: &ModuleInner,
    ) -> Self {
        Self {
            memories: local_backing.vm_memories.as_mut_ptr(),
            tables: local_backing.vm_tables.as_mut_ptr(),
            globals: local_backing.vm_globals.as_mut_ptr(),

            imported_memories: import_backing.vm_memories.as_mut_ptr(),
            imported_tables: import_backing.vm_tables.as_mut_ptr(),
            imported_globals: import_backing.vm_globals.as_mut_ptr(),
            imported_funcs: import_backing.vm_functions.as_mut_ptr(),

            local_backing,
            import_backing,
            module,

            data: ptr::null_mut(),
            data_finalizer: None,
        }
    }

    #[doc(hidden)]
    pub unsafe fn new_with_data(
        local_backing: &mut LocalBacking,
        import_backing: &mut ImportBacking,
        module: &ModuleInner,
        data: *mut c_void,
        data_finalizer: extern "C" fn(*mut c_void),
    ) -> Self {
        Self {
            memories: local_backing.vm_memories.as_mut_ptr(),
            tables: local_backing.vm_tables.as_mut_ptr(),
            globals: local_backing.vm_globals.as_mut_ptr(),

            imported_memories: import_backing.vm_memories.as_mut_ptr(),
            imported_tables: import_backing.vm_tables.as_mut_ptr(),
            imported_globals: import_backing.vm_globals.as_mut_ptr(),
            imported_funcs: import_backing.vm_functions.as_mut_ptr(),

            local_backing,
            import_backing,
            module,

            data,
            data_finalizer: Some(data_finalizer),
        }
    }

    /// This exposes the specified memory of the WebAssembly instance
    /// as a immutable slice.
    ///
    /// WebAssembly will soon support multiple linear memories, so this
    /// forces the user to specify.
    ///
    /// # Usage:
    ///
    /// ```
    /// # use wasmer_runtime_core::{
    /// #     vm::Ctx,
    /// # };
    /// fn read_memory(ctx: &Ctx) -> u8 {
    ///     let first_memory = ctx.memory(0);
    ///     // Read the first byte of that linear memory.
    ///     first_memory.view()[0].get()
    /// }
    /// ```
    pub fn memory(&self, mem_index: u32) -> &Memory {
        let module = unsafe { &*self.module };
        let mem_index = MemoryIndex::new(mem_index as usize);
        match mem_index.local_or_import(&module.info) {
            LocalOrImport::Local(local_mem_index) => unsafe {
                let local_backing = &*self.local_backing;
                &local_backing.memories[local_mem_index]
            },
            LocalOrImport::Import(import_mem_index) => unsafe {
                let import_backing = &*self.import_backing;
                &import_backing.memories[import_mem_index]
            },
        }
    }
}

#[doc(hidden)]
impl Ctx {
    #[allow(clippy::erasing_op)] // TODO
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

enum InnerFunc {}
/// Used to provide type safety (ish) for passing around function pointers.
/// The typesystem ensures this cannot be dereferenced since an
/// empty enum cannot actually exist.
#[repr(C)]
pub struct Func(InnerFunc);

/// An imported function, which contains the vmctx that owns this function.
#[derive(Debug, Clone)]
#[repr(C)]
pub struct ImportedFunc {
    pub func: *const Func,
    pub vmctx: *mut Ctx,
}

impl ImportedFunc {
    #[allow(clippy::erasing_op)] // TODO
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
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct LocalTable {
    /// pointer to the elements in the table.
    pub base: *mut u8,
    /// Number of elements in the table (NOT necessarily the size of the table in bytes!).
    pub count: usize,
    /// The table that this represents. At the moment, this can only be `*mut AnyfuncTable`.
    pub table: *mut (),
}

impl LocalTable {
    #[allow(clippy::erasing_op)] // TODO
    pub fn offset_base() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_count() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }

    pub fn size() -> u8 {
        mem::size_of::<Self>() as u8
    }
}

/// Definition of a memory used by the VM.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct LocalMemory {
    /// Pointer to the bottom of this linear memory.
    pub base: *mut u8,
    /// Current size of this linear memory in bytes.
    pub bound: usize,
    /// The actual memory that this represents.
    /// This is either `*mut DynamicMemory`, `*mut StaticMemory`,
    /// or `*mut SharedStaticMemory`.
    pub memory: *mut (),
}

impl LocalMemory {
    #[allow(clippy::erasing_op)] // TODO
    pub fn offset_base() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_bound() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }

    pub fn size() -> u8 {
        mem::size_of::<Self>() as u8
    }
}

/// Definition of a global used by the VM.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct LocalGlobal {
    pub data: u64,
}

impl LocalGlobal {
    #[allow(clippy::erasing_op)] // TODO
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

#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct SigId(pub u32);

/// Caller-checked anyfunc
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Anyfunc {
    pub func: *const Func,
    pub ctx: *mut Ctx,
    pub sig_id: SigId,
}

impl Anyfunc {
    pub fn null() -> Self {
        Self {
            func: ptr::null(),
            ctx: ptr::null_mut(),
            sig_id: SigId(u32::max_value()),
        }
    }

    #[allow(clippy::erasing_op)] // TODO
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
    use super::{Anyfunc, Ctx, ImportedFunc, LocalGlobal, LocalMemory, LocalTable};

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
            LocalTable::offset_count() as usize,
            offset_of!(LocalTable => count).get_byte_offset(),
        );
    }

    #[test]
    fn local_memory() {
        assert_eq!(
            LocalMemory::offset_base() as usize,
            offset_of!(LocalMemory => base).get_byte_offset(),
        );

        assert_eq!(
            LocalMemory::offset_bound() as usize,
            offset_of!(LocalMemory => bound).get_byte_offset(),
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
    fn cc_anyfunc() {
        assert_eq!(
            Anyfunc::offset_func() as usize,
            offset_of!(Anyfunc => func).get_byte_offset(),
        );

        assert_eq!(
            Anyfunc::offset_vmctx() as usize,
            offset_of!(Anyfunc => ctx).get_byte_offset(),
        );

        assert_eq!(
            Anyfunc::offset_sig_id() as usize,
            offset_of!(Anyfunc => sig_id).get_byte_offset(),
        );
    }
}

#[cfg(test)]
mod vm_ctx_tests {
    use super::{Ctx, ImportBacking, LocalBacking};
    use crate::module::{ModuleInfo, ModuleInner, StringTable};
    use crate::structures::Map;
    use std::ffi::c_void;

    struct TestData {
        x: u32,
        y: bool,
        str: String,
    }

    extern "C" fn test_data_finalizer(data: *mut c_void) {
        let test_data: &mut TestData = unsafe { &mut *(data as *mut TestData) };
        assert_eq!(test_data.x, 10);
        assert_eq!(test_data.y, true);
        assert_eq!(test_data.str, "Test".to_string());
        println!("hello from finalizer");
        drop(test_data);
    }

    #[test]
    fn test_callback_on_drop() {
        let mut data = TestData {
            x: 10,
            y: true,
            str: "Test".to_string(),
        };
        let mut local_backing = LocalBacking {
            memories: Map::new().into_boxed_map(),
            tables: Map::new().into_boxed_map(),
            globals: Map::new().into_boxed_map(),

            vm_memories: Map::new().into_boxed_map(),
            vm_tables: Map::new().into_boxed_map(),
            vm_globals: Map::new().into_boxed_map(),
        };
        let mut import_backing = ImportBacking {
            memories: Map::new().into_boxed_map(),
            tables: Map::new().into_boxed_map(),
            globals: Map::new().into_boxed_map(),

            vm_functions: Map::new().into_boxed_map(),
            vm_memories: Map::new().into_boxed_map(),
            vm_tables: Map::new().into_boxed_map(),
            vm_globals: Map::new().into_boxed_map(),
        };
        let module = generate_module();
        let data = &mut data as *mut _ as *mut c_void;
        let ctx = unsafe {
            Ctx::new_with_data(
                &mut local_backing,
                &mut import_backing,
                &module,
                data,
                test_data_finalizer,
            )
        };
        let ctx_test_data = cast_test_data(ctx.data);
        assert_eq!(ctx_test_data.x, 10);
        assert_eq!(ctx_test_data.y, true);
        assert_eq!(ctx_test_data.str, "Test".to_string());
        drop(ctx);
    }

    fn cast_test_data(data: *mut c_void) -> &'static mut TestData {
        let test_data: &mut TestData = unsafe { &mut *(data as *mut TestData) };
        test_data
    }

    fn generate_module() -> ModuleInner {
        use super::Func;
        use crate::backend::{
            sys::Memory, Backend, CacheGen, FuncResolver, ProtectedCaller, Token, UserTrapper,
        };
        use crate::cache::{Error as CacheError, WasmHash};
        use crate::error::RuntimeResult;
        use crate::types::{FuncIndex, LocalFuncIndex, Value};
        use hashbrown::HashMap;
        use std::ptr::NonNull;
        struct Placeholder;
        impl FuncResolver for Placeholder {
            fn get(
                &self,
                _module: &ModuleInner,
                _local_func_index: LocalFuncIndex,
            ) -> Option<NonNull<Func>> {
                None
            }
        }
        impl ProtectedCaller for Placeholder {
            fn call(
                &self,
                _module: &ModuleInner,
                _func_index: FuncIndex,
                _params: &[Value],
                _import_backing: &ImportBacking,
                _vmctx: *mut Ctx,
                _: Token,
            ) -> RuntimeResult<Vec<Value>> {
                Ok(vec![])
            }
            fn get_early_trapper(&self) -> Box<dyn UserTrapper> {
                unimplemented!()
            }
        }
        impl CacheGen for Placeholder {
            fn generate_cache(
                &self,
                module: &ModuleInner,
            ) -> Result<(Box<ModuleInfo>, Box<[u8]>, Memory), CacheError> {
                unimplemented!()
            }
        }

        ModuleInner {
            func_resolver: Box::new(Placeholder),
            protected_caller: Box::new(Placeholder),
            cache_gen: Box::new(Placeholder),
            info: ModuleInfo {
                memories: Map::new(),
                globals: Map::new(),
                tables: Map::new(),

                // These are strictly imported and the typesystem ensures that.
                imported_functions: Map::new(),
                imported_memories: Map::new(),
                imported_tables: Map::new(),
                imported_globals: Map::new(),

                exports: HashMap::new(),

                data_initializers: Vec::new(),
                elem_initializers: Vec::new(),

                start_func: None,

                func_assoc: Map::new(),
                signatures: Map::new(),
                backend: Backend::Cranelift,

                namespace_table: StringTable::new(),
                name_table: StringTable::new(),
            },
        }
    }
}
