pub use crate::backing::{ImportBacking, LocalBacking, INTERNALS_SIZE};
use crate::{
    memory::{Memory, MemoryType},
    module::{ModuleInfo, ModuleInner},
    structures::TypedIndex,
    types::{LocalOrImport, MemoryIndex},
    vmcalls,
};
use std::{
    cell::UnsafeCell,
    ffi::c_void,
    mem, ptr,
    sync::atomic::{AtomicUsize, Ordering},
    sync::Once,
};

use hashbrown::HashMap;

/// The context of the currently running WebAssembly instance.
///
/// This is implicitly passed to every WebAssembly function.
/// Since this is per-instance, each field has a statically
/// (as in after compiling the wasm) known size, so no
/// runtime checks are necessary.
///
/// While the runtime currently just passes this around
/// as the first, implicit parameter of every function,
/// it may someday be pinned to a register (especially
/// on arm, which has a ton of registers) to reduce
/// register shuffling.
#[derive(Debug)]
#[repr(C)]
pub struct Ctx {
    // `internal` must be the first field of `Ctx`.
    pub internal: InternalCtx,

    pub(crate) local_functions: *const *const Func,

    /// These are pointers to things that are known to be owned
    /// by the owning `Instance`.
    pub local_backing: *mut LocalBacking,
    pub import_backing: *mut ImportBacking,
    pub module: *const ModuleInner,

    //// This is intended to be user-supplied, per-instance
    /// contextual data. There are currently some issue with it,
    /// notably that it cannot be set before running the `start`
    /// function in a WebAssembly module.
    ///
    /// [#219](https://github.com/wasmerio/wasmer/pull/219) fixes that
    /// issue, as well as allowing the user to have *per-function*
    /// context, instead of just per-instance.
    pub data: *mut c_void,

    /// If there's a function set in this field, it gets called
    /// when the context is destructed, e.g. when an `Instance`
    /// is dropped.
    pub data_finalizer: Option<fn(data: *mut c_void)>,
}

/// The internal context of the currently running WebAssembly instance.
///
///
#[doc(hidden)]
#[derive(Debug)]
#[repr(C)]
pub struct InternalCtx {
    /// A pointer to an array of locally-defined memories, indexed by `MemoryIndex`.
    pub memories: *mut *mut LocalMemory,

    /// A pointer to an array of locally-defined tables, indexed by `TableIndex`.
    pub tables: *mut *mut LocalTable,

    /// A pointer to an array of locally-defined globals, indexed by `GlobalIndex`.
    pub globals: *mut *mut LocalGlobal,

    /// A pointer to an array of imported memories, indexed by `MemoryIndex,
    pub imported_memories: *mut *mut LocalMemory,

    /// A pointer to an array of imported tables, indexed by `TableIndex`.
    pub imported_tables: *mut *mut LocalTable,

    /// A pointer to an array of imported globals, indexed by `GlobalIndex`.
    pub imported_globals: *mut *mut LocalGlobal,

    /// A pointer to an array of imported functions, indexed by `FuncIndex`.
    pub imported_funcs: *mut ImportedFunc,

    /// A pointer to an array of signature ids. Conceptually, this maps
    /// from a static, module-local signature id to a runtime-global
    /// signature id. This is used to allow call-indirect to other
    /// modules safely.
    pub dynamic_sigindices: *const SigId,

    pub intrinsics: *const Intrinsics,

    pub stack_lower_bound: *mut u8,

    pub memory_base: *mut u8,
    pub memory_bound: usize,

    pub internals: *mut [u64; INTERNALS_SIZE], // TODO: Make this dynamic?

    pub interrupt_signal_mem: *mut u8,
}

static INTERNAL_FIELDS: AtomicUsize = AtomicUsize::new(0);

pub struct InternalField {
    init: Once,
    inner: UnsafeCell<usize>,
}

unsafe impl Send for InternalField {}
unsafe impl Sync for InternalField {}

impl InternalField {
    pub const fn allocate() -> InternalField {
        InternalField {
            init: Once::new(),
            inner: UnsafeCell::new(::std::usize::MAX),
        }
    }

    pub fn index(&self) -> usize {
        let inner: *mut usize = self.inner.get();
        self.init.call_once(|| {
            let idx = INTERNAL_FIELDS.fetch_add(1, Ordering::SeqCst);
            if idx >= INTERNALS_SIZE {
                INTERNAL_FIELDS.fetch_sub(1, Ordering::SeqCst);
                panic!("at most {} internal fields are supported", INTERNALS_SIZE);
            } else {
                unsafe {
                    *inner = idx;
                }
            }
        });
        unsafe { *inner }
    }
}

#[repr(C)]
pub struct Intrinsics {
    pub memory_grow: *const Func,
    pub memory_size: *const Func,
    /*pub memory_grow: unsafe extern "C" fn(
        ctx: &mut Ctx,
        memory_index: usize,
        delta: Pages,
    ) -> i32,
    pub memory_size: unsafe extern "C" fn(
        ctx: &Ctx,
        memory_index: usize,
    ) -> Pages,*/
}

unsafe impl Send for Intrinsics {}
unsafe impl Sync for Intrinsics {}

impl Intrinsics {
    #[allow(clippy::erasing_op)]
    pub fn offset_memory_grow() -> u8 {
        (0 * ::std::mem::size_of::<usize>()) as u8
    }
    pub fn offset_memory_size() -> u8 {
        (1 * ::std::mem::size_of::<usize>()) as u8
    }
}

pub static INTRINSICS_LOCAL_STATIC_MEMORY: Intrinsics = Intrinsics {
    memory_grow: vmcalls::local_static_memory_grow as _,
    memory_size: vmcalls::local_static_memory_size as _,
};
pub static INTRINSICS_LOCAL_DYNAMIC_MEMORY: Intrinsics = Intrinsics {
    memory_grow: vmcalls::local_dynamic_memory_grow as _,
    memory_size: vmcalls::local_dynamic_memory_size as _,
};
pub static INTRINSICS_IMPORTED_STATIC_MEMORY: Intrinsics = Intrinsics {
    memory_grow: vmcalls::imported_static_memory_grow as _,
    memory_size: vmcalls::imported_static_memory_size as _,
};
pub static INTRINSICS_IMPORTED_DYNAMIC_MEMORY: Intrinsics = Intrinsics {
    memory_grow: vmcalls::imported_dynamic_memory_grow as _,
    memory_size: vmcalls::imported_dynamic_memory_size as _,
};

fn get_intrinsics_for_module(m: &ModuleInfo) -> *const Intrinsics {
    if m.memories.len() == 0 && m.imported_memories.len() == 0 {
        ::std::ptr::null()
    } else {
        match MemoryIndex::new(0).local_or_import(m) {
            LocalOrImport::Local(local_mem_index) => {
                let mem_desc = &m.memories[local_mem_index];
                match mem_desc.memory_type() {
                    MemoryType::Dynamic => &INTRINSICS_LOCAL_DYNAMIC_MEMORY,
                    MemoryType::Static => &INTRINSICS_LOCAL_STATIC_MEMORY,
                    MemoryType::SharedStatic => unimplemented!(),
                }
            }
            LocalOrImport::Import(import_mem_index) => {
                let mem_desc = &m.imported_memories[import_mem_index].1;
                match mem_desc.memory_type() {
                    MemoryType::Dynamic => &INTRINSICS_IMPORTED_DYNAMIC_MEMORY,
                    MemoryType::Static => &INTRINSICS_IMPORTED_STATIC_MEMORY,
                    MemoryType::SharedStatic => unimplemented!(),
                }
            }
        }
    }
}

#[cfg(all(unix, target_arch = "x86_64"))]
fn get_interrupt_signal_mem() -> *mut u8 {
    unsafe { crate::fault::get_wasm_interrupt_signal_mem() }
}

#[cfg(not(all(unix, target_arch = "x86_64")))]
fn get_interrupt_signal_mem() -> *mut u8 {
    static mut REGION: u64 = 0;
    unsafe { &mut REGION as *mut u64 as *mut u8 }
}

impl Ctx {
    #[doc(hidden)]
    pub unsafe fn new(
        local_backing: &mut LocalBacking,
        import_backing: &mut ImportBacking,
        module: &ModuleInner,
    ) -> Self {
        let (mem_base, mem_bound): (*mut u8, usize) =
            if module.info.memories.len() == 0 && module.info.imported_memories.len() == 0 {
                (::std::ptr::null_mut(), 0)
            } else {
                let mem = match MemoryIndex::new(0).local_or_import(&module.info) {
                    LocalOrImport::Local(index) => local_backing.vm_memories[index],
                    LocalOrImport::Import(index) => import_backing.vm_memories[index],
                };
                ((*mem).base, (*mem).bound)
            };
        Self {
            internal: InternalCtx {
                memories: local_backing.vm_memories.as_mut_ptr(),
                tables: local_backing.vm_tables.as_mut_ptr(),
                globals: local_backing.vm_globals.as_mut_ptr(),

                imported_memories: import_backing.vm_memories.as_mut_ptr(),
                imported_tables: import_backing.vm_tables.as_mut_ptr(),
                imported_globals: import_backing.vm_globals.as_mut_ptr(),
                imported_funcs: import_backing.vm_functions.as_mut_ptr(),

                dynamic_sigindices: local_backing.dynamic_sigindices.as_ptr(),

                intrinsics: get_intrinsics_for_module(&module.info),

                stack_lower_bound: ::std::ptr::null_mut(),

                memory_base: mem_base,
                memory_bound: mem_bound,

                internals: &mut local_backing.internals.0,

                interrupt_signal_mem: get_interrupt_signal_mem(),
            },
            local_functions: local_backing.local_functions.as_ptr(),

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
        data_finalizer: fn(*mut c_void),
    ) -> Self {
        let (mem_base, mem_bound): (*mut u8, usize) =
            if module.info.memories.len() == 0 && module.info.imported_memories.len() == 0 {
                (::std::ptr::null_mut(), 0)
            } else {
                let mem = match MemoryIndex::new(0).local_or_import(&module.info) {
                    LocalOrImport::Local(index) => local_backing.vm_memories[index],
                    LocalOrImport::Import(index) => import_backing.vm_memories[index],
                };
                ((*mem).base, (*mem).bound)
            };
        Self {
            internal: InternalCtx {
                memories: local_backing.vm_memories.as_mut_ptr(),
                tables: local_backing.vm_tables.as_mut_ptr(),
                globals: local_backing.vm_globals.as_mut_ptr(),

                imported_memories: import_backing.vm_memories.as_mut_ptr(),
                imported_tables: import_backing.vm_tables.as_mut_ptr(),
                imported_globals: import_backing.vm_globals.as_mut_ptr(),
                imported_funcs: import_backing.vm_functions.as_mut_ptr(),

                dynamic_sigindices: local_backing.dynamic_sigindices.as_ptr(),

                intrinsics: get_intrinsics_for_module(&module.info),

                stack_lower_bound: ::std::ptr::null_mut(),

                memory_base: mem_base,
                memory_bound: mem_bound,

                internals: &mut local_backing.internals.0,

                interrupt_signal_mem: get_interrupt_signal_mem(),
            },
            local_functions: local_backing.local_functions.as_ptr(),

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

    /// Gives access to the emscripten symbol map, used for debugging
    pub unsafe fn borrow_symbol_map(&self) -> &Option<HashMap<u32, String>> {
        &(*self.module).info.em_symbol_map
    }

    /// Returns the number of dynamic sigindices.
    pub fn dynamic_sigindice_count(&self) -> usize {
        unsafe { (*self.local_backing).dynamic_sigindices.len() }
    }

    /// Returns the value of the specified internal field.
    pub fn get_internal(&self, field: &InternalField) -> u64 {
        unsafe { (*self.internal.internals)[field.index()] }
    }

    /// Writes the value to the specified internal field.
    pub fn set_internal(&mut self, field: &InternalField, value: u64) {
        unsafe {
            (*self.internal.internals)[field.index()] = value;
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

    pub fn offset_intrinsics() -> u8 {
        8 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_stack_lower_bound() -> u8 {
        9 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_memory_base() -> u8 {
        10 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_memory_bound() -> u8 {
        11 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_internals() -> u8 {
        12 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_interrupt_signal_mem() -> u8 {
        13 * (mem::size_of::<usize>() as u8)
    }

    pub fn offset_local_functions() -> u8 {
        14 * (mem::size_of::<usize>() as u8)
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
    use super::{Anyfunc, Ctx, ImportedFunc, InternalCtx, LocalGlobal, LocalMemory, LocalTable};

    #[test]
    fn vmctx() {
        assert_eq!(0usize, offset_of!(Ctx => internal).get_byte_offset(),);

        assert_eq!(
            Ctx::offset_memories() as usize,
            offset_of!(InternalCtx => memories).get_byte_offset(),
        );

        assert_eq!(
            Ctx::offset_tables() as usize,
            offset_of!(InternalCtx => tables).get_byte_offset(),
        );

        assert_eq!(
            Ctx::offset_globals() as usize,
            offset_of!(InternalCtx => globals).get_byte_offset(),
        );

        assert_eq!(
            Ctx::offset_imported_memories() as usize,
            offset_of!(InternalCtx => imported_memories).get_byte_offset(),
        );

        assert_eq!(
            Ctx::offset_imported_tables() as usize,
            offset_of!(InternalCtx => imported_tables).get_byte_offset(),
        );

        assert_eq!(
            Ctx::offset_imported_globals() as usize,
            offset_of!(InternalCtx => imported_globals).get_byte_offset(),
        );

        assert_eq!(
            Ctx::offset_imported_funcs() as usize,
            offset_of!(InternalCtx => imported_funcs).get_byte_offset(),
        );

        assert_eq!(
            Ctx::offset_intrinsics() as usize,
            offset_of!(InternalCtx => intrinsics).get_byte_offset(),
        );

        assert_eq!(
            Ctx::offset_stack_lower_bound() as usize,
            offset_of!(InternalCtx => stack_lower_bound).get_byte_offset(),
        );

        assert_eq!(
            Ctx::offset_memory_base() as usize,
            offset_of!(InternalCtx => memory_base).get_byte_offset(),
        );

        assert_eq!(
            Ctx::offset_memory_bound() as usize,
            offset_of!(InternalCtx => memory_bound).get_byte_offset(),
        );

        assert_eq!(
            Ctx::offset_internals() as usize,
            offset_of!(InternalCtx => internals).get_byte_offset(),
        );

        assert_eq!(
            Ctx::offset_interrupt_signal_mem() as usize,
            offset_of!(InternalCtx => interrupt_signal_mem).get_byte_offset(),
        );

        assert_eq!(
            Ctx::offset_local_functions() as usize,
            offset_of!(Ctx => local_functions).get_byte_offset(),
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

    fn test_data_finalizer(data: *mut c_void) {
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

            dynamic_sigindices: Map::new().into_boxed_map(),
            local_functions: Map::new().into_boxed_map(),

            internals: crate::backing::Internals([0; crate::backing::INTERNALS_SIZE]),
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
        use crate::backend::{sys::Memory, Backend, CacheGen, RunnableModule};
        use crate::cache::Error as CacheError;
        use crate::typed_func::Wasm;
        use crate::types::{LocalFuncIndex, SigIndex};
        use hashbrown::HashMap;
        use std::any::Any;
        use std::ptr::NonNull;
        struct Placeholder;
        impl RunnableModule for Placeholder {
            fn get_func(
                &self,
                _module: &ModuleInfo,
                _local_func_index: LocalFuncIndex,
            ) -> Option<NonNull<Func>> {
                None
            }

            fn get_trampoline(&self, _module: &ModuleInfo, _sig_index: SigIndex) -> Option<Wasm> {
                unimplemented!()
            }
            unsafe fn do_early_trap(&self, _: Box<dyn Any>) -> ! {
                unimplemented!()
            }
        }
        impl CacheGen for Placeholder {
            fn generate_cache(&self) -> Result<(Box<[u8]>, Memory), CacheError> {
                unimplemented!()
            }
        }

        ModuleInner {
            runnable_module: Box::new(Placeholder),
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

                em_symbol_map: None,

                custom_sections: HashMap::new(),
            },
        }
    }
}
