//! The runtime vm module contains data structures and helper functions used during runtime to
//! execute wasm instance functions.
pub use crate::backing::{ImportBacking, LocalBacking, INTERNALS_SIZE};
use crate::{
    error::CallResult,
    instance::call_func_with_index_inner,
    memory::{BackingMemoryType, Memory},
    module::{ModuleInfo, ModuleInner},
    sig_registry::SigRegistry,
    structures::TypedIndex,
    types::{LocalOrImport, MemoryIndex, TableIndex, Value},
    vmcalls,
};
use std::{
    cell::UnsafeCell,
    ffi::c_void,
    mem,
    ptr::{self, NonNull},
    sync::atomic::{AtomicUsize, Ordering},
    sync::Once,
};

use std::collections::HashMap;

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
    /// InternalCtx data field
    pub internal: InternalCtx,

    pub(crate) local_functions: *const *const Func,

    /// These are pointers to things that are known to be owned
    /// by the owning `Instance`.
    pub local_backing: *mut LocalBacking,
    /// Mutable pointer to import data
    pub import_backing: *mut ImportBacking,
    /// Const pointer to module inner data
    pub module: *const ModuleInner,

    /// This is intended to be user-supplied, per-instance
    /// contextual data. There are currently some issue with it,
    /// notably that it cannot be set before running the `start`
    /// function in a WebAssembly module. Additionally, the `data`
    /// field may be taken by another ABI implementation that the user
    /// wishes to use in addition to their own, such as WASI.  This issue is
    /// being discussed at [#1111](https://github.com/wasmerio/wasmer/pull/1111).
    ///
    /// Alternatively, per-function data can be used if the function in the
    /// [`ImportObject`] is a closure.  This cannot duplicate data though,
    /// so if data may be shared if the [`ImportObject`] is reused.
    pub data: *mut c_void,

    /// If there's a function set in this field, it gets called
    /// when the context is destructed, e.g. when an `Instance`
    /// is dropped.
    pub data_finalizer: Option<fn(data: *mut c_void)>,
}

/// When an instance context is destructed, we're calling its `data_finalizer`
/// In order avoid leaking resources.
///
/// Implementing the `data_finalizer` function is the responsibility of the `wasmer` end-user.
///
/// See test: `test_data_finalizer` as an example
impl Drop for Ctx {
    fn drop(&mut self) {
        if let Some(ref finalizer) = self.data_finalizer {
            finalizer(self.data);
        }
    }
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

    /// A pointer to an array of imported memories, indexed by `MemoryIndex`,
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

    /// Const pointer to Intrinsics.
    pub intrinsics: *const Intrinsics,

    /// Stack lower bound.
    pub stack_lower_bound: *mut u8,

    /// Mutable pointer to memory base.
    pub memory_base: *mut u8,
    /// Memory bound.
    pub memory_bound: usize,

    /// Mutable pointer to internal fields.
    pub internals: *mut [u64; INTERNALS_SIZE], // TODO: Make this dynamic?

    /// Interrupt signal mem.
    pub interrupt_signal_mem: *mut u8,
}

static INTERNAL_FIELDS: AtomicUsize = AtomicUsize::new(0);

/// An internal field.
pub struct InternalField {
    /// Init once field.
    init: Once,
    /// Inner field.
    inner: UnsafeCell<usize>,
}

unsafe impl Send for InternalField {}
unsafe impl Sync for InternalField {}

impl InternalField {
    /// Allocate and return an `InternalField`.
    pub const fn allocate() -> InternalField {
        InternalField {
            init: Once::new(),
            inner: UnsafeCell::new(::std::usize::MAX),
        }
    }

    /// Get the index of this `InternalField`.
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

/// A container for VM instrinsic functions
#[repr(C)]
pub struct Intrinsics {
    /// Const pointer to memory grow `Func`.
    pub memory_grow: *const Func,
    /// Const pointer to memory size `Func`.
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
    /// Offset of the `memory_grow` field.
    #[allow(clippy::erasing_op)]
    pub const fn offset_memory_grow() -> u8 {
        (0 * ::std::mem::size_of::<usize>()) as u8
    }
    /// Offset of the `memory_size` field.
    pub const fn offset_memory_size() -> u8 {
        (1 * ::std::mem::size_of::<usize>()) as u8
    }
}

/// Local static memory intrinsics
pub static INTRINSICS_LOCAL_STATIC_MEMORY: Intrinsics = Intrinsics {
    memory_grow: vmcalls::local_static_memory_grow as _,
    memory_size: vmcalls::local_static_memory_size as _,
};
/// Local dynamic memory intrinsics
pub static INTRINSICS_LOCAL_DYNAMIC_MEMORY: Intrinsics = Intrinsics {
    memory_grow: vmcalls::local_dynamic_memory_grow as _,
    memory_size: vmcalls::local_dynamic_memory_size as _,
};
/// Imported static memory intrinsics
pub static INTRINSICS_IMPORTED_STATIC_MEMORY: Intrinsics = Intrinsics {
    memory_grow: vmcalls::imported_static_memory_grow as _,
    memory_size: vmcalls::imported_static_memory_size as _,
};
/// Imported dynamic memory intrinsics
pub static INTRINSICS_IMPORTED_DYNAMIC_MEMORY: Intrinsics = Intrinsics {
    memory_grow: vmcalls::imported_dynamic_memory_grow as _,
    memory_size: vmcalls::imported_dynamic_memory_size as _,
};

fn get_intrinsics_for_module(m: &ModuleInfo) -> *const Intrinsics {
    if m.memories.is_empty() && m.imported_memories.is_empty() {
        ptr::null()
    } else {
        match MemoryIndex::new(0).local_or_import(m) {
            LocalOrImport::Local(local_mem_index) => {
                let mem_desc = &m.memories[local_mem_index];
                match mem_desc.memory_type() {
                    BackingMemoryType::Dynamic => &INTRINSICS_LOCAL_DYNAMIC_MEMORY,
                    BackingMemoryType::Static => &INTRINSICS_LOCAL_STATIC_MEMORY,
                    BackingMemoryType::SharedStatic => &INTRINSICS_LOCAL_STATIC_MEMORY,
                }
            }
            LocalOrImport::Import(import_mem_index) => {
                let mem_desc = &m.imported_memories[import_mem_index].1;
                match mem_desc.memory_type() {
                    BackingMemoryType::Dynamic => &INTRINSICS_IMPORTED_DYNAMIC_MEMORY,
                    BackingMemoryType::Static => &INTRINSICS_IMPORTED_STATIC_MEMORY,
                    BackingMemoryType::SharedStatic => &INTRINSICS_IMPORTED_STATIC_MEMORY,
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
            if module.info.memories.is_empty() && module.info.imported_memories.is_empty() {
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
            if module.info.memories.is_empty() && module.info.imported_memories.is_empty() {
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

                stack_lower_bound: ptr::null_mut(),

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

    /// Get access to [`Memory`] and mutable access to the user defined data
    /// field as the type, `T`.
    ///
    /// This method is required to access both at the same time.
    /// This is useful for updating a data type that stores information about
    /// locations in Wasm memory.
    ///
    /// # Safety
    ///
    /// This function must be called with the same type, `T`, that the `data`
    /// was initialized with.
    pub unsafe fn memory_and_data_mut<T>(&mut self, mem_index: u32) -> (&Memory, &mut T) {
        (self.memory(mem_index), &mut *(self.data as *mut T))
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

    /// Calls a host or Wasm function at the given table index
    pub fn call_with_table_index(
        &mut self,
        index: TableIndex,
        args: &[Value],
    ) -> CallResult<Vec<Value>> {
        let anyfunc_table =
            unsafe { &*((**self.internal.tables).table as *mut crate::table::AnyfuncTable) };
        let Anyfunc { func, ctx, sig_id } = anyfunc_table.backing[index.index()];

        let signature = SigRegistry.lookup_signature(unsafe { std::mem::transmute(sig_id.0) });
        let mut rets = vec![];

        let wasm = {
            let module = unsafe { &*self.module };
            let runnable = &module.runnable_module;

            let sig_index = SigRegistry.lookup_sig_index(signature.clone());
            runnable
                .get_trampoline(&module.info, sig_index)
                .expect("wasm trampoline")
        };

        call_func_with_index_inner(
            ctx,
            NonNull::new(func as *mut _).unwrap(),
            &signature,
            wasm,
            args,
            &mut rets,
        )?;

        Ok(rets)
    }
}

#[doc(hidden)]
impl Ctx {
    #[allow(clippy::erasing_op)] // TODO
    pub const fn offset_memories() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_tables() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_globals() -> u8 {
        2 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_imported_memories() -> u8 {
        3 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_imported_tables() -> u8 {
        4 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_imported_globals() -> u8 {
        5 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_imported_funcs() -> u8 {
        6 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_signatures() -> u8 {
        7 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_intrinsics() -> u8 {
        8 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_stack_lower_bound() -> u8 {
        9 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_memory_base() -> u8 {
        10 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_memory_bound() -> u8 {
        11 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_internals() -> u8 {
        12 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_interrupt_signal_mem() -> u8 {
        13 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_local_functions() -> u8 {
        14 * (mem::size_of::<usize>() as u8)
    }
}

/// Represents a function pointer. It is mostly used in the
/// `typed_func` module within the `wrap` functions, to wrap imported
/// functions.
#[repr(transparent)]
pub struct Func(*mut c_void);

/// Represents a function environment pointer, like a captured
/// environment of a closure. It is mostly used in the `typed_func`
/// module within the `wrap` functions, to wrap imported functions.
#[repr(transparent)]
pub struct FuncEnv(*mut c_void);

/// Represents a function context. It is used by imported functions
/// only.
#[derive(Debug)]
#[repr(C)]
pub struct FuncCtx {
    /// The `Ctx` pointer.
    pub(crate) vmctx: NonNull<Ctx>,

    /// A pointer to the function environment. It is used by imported
    /// functions only to store the pointer to the real host function,
    /// whether it is a regular function, or a closure with or without
    /// a captured environment.
    pub(crate) func_env: Option<NonNull<FuncEnv>>,
}

impl FuncCtx {
    /// Offset to the `vmctx` field.
    #[allow(clippy::erasing_op)]
    pub const fn offset_vmctx() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    /// Offset to the `func_env` field.
    pub const fn offset_func_env() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }

    /// Size of a `FuncCtx`.
    pub const fn size() -> u8 {
        mem::size_of::<Self>() as u8
    }
}

/// An imported function is a function pointer associated to a
/// function context.
#[derive(Debug, Clone)]
#[repr(C)]
pub struct ImportedFunc {
    /// Pointer to the function itself.
    pub(crate) func: *const Func,

    /// Mutable non-null pointer to [`FuncCtx`].
    pub(crate) func_ctx: NonNull<FuncCtx>,
}

// Manually implemented because ImportedFunc contains raw pointers
// directly; `Func` is marked Send (But `Ctx` actually isn't! (TODO:
// review this, shouldn't `Ctx` be Send?))
unsafe impl Send for ImportedFunc {}

impl ImportedFunc {
    /// Offset to the `func` field.
    #[allow(clippy::erasing_op)] // TODO
    pub const fn offset_func() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    /// Offset to the `func_ctx` field.
    pub const fn offset_func_ctx() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }

    /// Size of an `ImportedFunc`.
    pub const fn size() -> u8 {
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

// manually implemented because LocalTable contains raw pointers directly
unsafe impl Send for LocalTable {}

impl LocalTable {
    /// Offset to the `base` field.
    #[allow(clippy::erasing_op)] // TODO
    pub const fn offset_base() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    /// Offset to the `count` field.
    pub const fn offset_count() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }

    /// Size of a `LocalTable`.
    pub const fn size() -> u8 {
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

// manually implemented because LocalMemory contains raw pointers
unsafe impl Send for LocalMemory {}

impl LocalMemory {
    /// Offset to the `base` field.
    #[allow(clippy::erasing_op)] // TODO
    pub const fn offset_base() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    /// Offset to the `bound` field.
    pub const fn offset_bound() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }

    /// Size of a `LocalMemory`.
    pub const fn size() -> u8 {
        mem::size_of::<Self>() as u8
    }
}

/// Definition of a global used by the VM.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct LocalGlobal {
    /// Data.
    pub data: u128,
}

impl LocalGlobal {
    /// Offset to the `data` field.
    #[allow(clippy::erasing_op)] // TODO
    pub const fn offset_data() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    /// A null `LocalGlobal`.
    pub const fn null() -> Self {
        Self { data: 0 }
    }

    /// Size of a `LocalGlobal`.
    pub const fn size() -> u8 {
        mem::size_of::<Self>() as u8
    }
}

/// Identifier for a function signature.
///
/// A transparent `SigIndex`
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct SigId(pub u32);

use crate::types::SigIndex;
impl From<SigId> for SigIndex {
    fn from(other: SigId) -> SigIndex {
        SigIndex::new(other.0 as _)
    }
}

/// Caller-checked anyfunc
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Anyfunc {
    /// Const pointer to `Func`.
    pub func: *const Func,
    /// Mutable pointer to `Ctx`.
    pub ctx: *mut Ctx,
    /// Sig id of this function
    pub sig_id: SigId,
}

// manually implemented because Anyfunc contains raw pointers directly
unsafe impl Send for Anyfunc {}

impl Anyfunc {
    /// A null `Anyfunc` value.
    pub const fn null() -> Self {
        Self {
            func: ptr::null(),
            ctx: ptr::null_mut(),
            sig_id: SigId(u32::max_value()),
        }
    }

    /// Offset to the `func` field.
    #[allow(clippy::erasing_op)] // TODO
    pub const fn offset_func() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    /// Offset to the `vmctx` field..
    pub const fn offset_vmctx() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }

    /// Offset to the `sig_id` field.
    pub const fn offset_sig_id() -> u8 {
        2 * (mem::size_of::<usize>() as u8)
    }

    /// The size of `Anyfunc`.
    pub const fn size() -> u8 {
        mem::size_of::<Self>() as u8
    }
}

#[cfg(test)]
mod vm_offset_tests {
    use super::{
        Anyfunc, Ctx, FuncCtx, ImportedFunc, InternalCtx, LocalGlobal, LocalMemory, LocalTable,
    };

    // Inspired by https://internals.rust-lang.org/t/discussion-on-offset-of/7440/2.
    macro_rules! offset_of {
        ($struct:path, $field:ident) => {{
            fn offset() -> usize {
                use std::mem;

                let structure = mem::MaybeUninit::<$struct>::uninit();

                let &$struct {
                    $field: ref field, ..
                } = unsafe { &*structure.as_ptr() };

                let offset =
                    (field as *const _ as usize).wrapping_sub(&structure as *const _ as usize);

                assert!((0..=mem::size_of_val(&structure)).contains(&offset));

                offset
            }

            offset()
        }};
    }

    #[test]
    fn offset_of() {
        use std::{mem, ptr::NonNull};

        struct S0;

        #[repr(C)]
        struct S1 {
            f1: u8,
            f2: u16,
            f3: u32,
            f4: u64,
            f5: u128,
            f6: f32,
            f7: f64,
            f8: NonNull<S0>,
            f9: Option<NonNull<S0>>,
            f10: *mut S0,
            z: u8,
        }

        assert_eq!(offset_of!(S1, f1), 0);
        assert_eq!(offset_of!(S1, f2), 2);
        assert_eq!(offset_of!(S1, f3), 4);
        assert_eq!(offset_of!(S1, f4), 8);
        assert_eq!(offset_of!(S1, f5), 16);
        assert_eq!(offset_of!(S1, f6), 32);
        assert_eq!(offset_of!(S1, f7), 40);
        assert_eq!(offset_of!(S1, f8), 40 + mem::size_of::<usize>());
        assert_eq!(offset_of!(S1, f9), 48 + mem::size_of::<usize>());
        assert_eq!(offset_of!(S1, f10), 56 + mem::size_of::<usize>());
        assert_eq!(offset_of!(S1, z), 64 + mem::size_of::<usize>());
    }

    #[test]
    fn vmctx() {
        assert_eq!(0usize, offset_of!(Ctx, internal));

        assert_eq!(
            Ctx::offset_memories() as usize,
            offset_of!(InternalCtx, memories),
        );

        assert_eq!(
            Ctx::offset_tables() as usize,
            offset_of!(InternalCtx, tables),
        );

        assert_eq!(
            Ctx::offset_globals() as usize,
            offset_of!(InternalCtx, globals),
        );

        assert_eq!(
            Ctx::offset_imported_memories() as usize,
            offset_of!(InternalCtx, imported_memories),
        );

        assert_eq!(
            Ctx::offset_imported_tables() as usize,
            offset_of!(InternalCtx, imported_tables),
        );

        assert_eq!(
            Ctx::offset_imported_globals() as usize,
            offset_of!(InternalCtx, imported_globals),
        );

        assert_eq!(
            Ctx::offset_imported_funcs() as usize,
            offset_of!(InternalCtx, imported_funcs),
        );

        assert_eq!(
            Ctx::offset_intrinsics() as usize,
            offset_of!(InternalCtx, intrinsics),
        );

        assert_eq!(
            Ctx::offset_stack_lower_bound() as usize,
            offset_of!(InternalCtx, stack_lower_bound),
        );

        assert_eq!(
            Ctx::offset_memory_base() as usize,
            offset_of!(InternalCtx, memory_base),
        );

        assert_eq!(
            Ctx::offset_memory_bound() as usize,
            offset_of!(InternalCtx, memory_bound),
        );

        assert_eq!(
            Ctx::offset_internals() as usize,
            offset_of!(InternalCtx, internals),
        );

        assert_eq!(
            Ctx::offset_interrupt_signal_mem() as usize,
            offset_of!(InternalCtx, interrupt_signal_mem),
        );

        assert_eq!(
            Ctx::offset_local_functions() as usize,
            offset_of!(Ctx, local_functions),
        );
    }

    #[test]
    fn func_ctx() {
        assert_eq!(FuncCtx::offset_vmctx() as usize, 0,);

        assert_eq!(FuncCtx::offset_func_env() as usize, 8,);
    }

    #[test]
    fn imported_func() {
        assert_eq!(
            ImportedFunc::offset_func() as usize,
            offset_of!(ImportedFunc, func),
        );

        assert_eq!(
            ImportedFunc::offset_func_ctx() as usize,
            offset_of!(ImportedFunc, func_ctx),
        );
    }

    #[test]
    fn local_table() {
        assert_eq!(
            LocalTable::offset_base() as usize,
            offset_of!(LocalTable, base),
        );

        assert_eq!(
            LocalTable::offset_count() as usize,
            offset_of!(LocalTable, count),
        );
    }

    #[test]
    fn local_memory() {
        assert_eq!(
            LocalMemory::offset_base() as usize,
            offset_of!(LocalMemory, base),
        );

        assert_eq!(
            LocalMemory::offset_bound() as usize,
            offset_of!(LocalMemory, bound),
        );
    }

    #[test]
    fn local_global() {
        assert_eq!(
            LocalGlobal::offset_data() as usize,
            offset_of!(LocalGlobal, data),
        );
    }

    #[test]
    fn cc_anyfunc() {
        assert_eq!(Anyfunc::offset_func() as usize, offset_of!(Anyfunc, func),);

        assert_eq!(Anyfunc::offset_vmctx() as usize, offset_of!(Anyfunc, ctx),);

        assert_eq!(
            Anyfunc::offset_sig_id() as usize,
            offset_of!(Anyfunc, sig_id),
        );
    }
}

#[cfg(test)]
mod vm_ctx_tests {
    use super::{Ctx, ImportBacking, LocalBacking};
    use crate::module::{ModuleInfo, ModuleInner, StringTable};
    use crate::structures::Map;
    use std::ffi::c_void;
    use std::sync::Arc;

    struct TestData {
        x: u32,
        y: bool,
        str: String,
        finalizer: Box<dyn FnMut()>,
    }

    impl Drop for TestData {
        fn drop(&mut self) {
            (*self.finalizer)();
        }
    }

    fn test_data_finalizer(data: *mut c_void) {
        let test_data: &mut TestData = unsafe { &mut *(data as *mut TestData) };

        assert_eq!(10, test_data.x);
        assert_eq!(true, test_data.y);
        assert_eq!("Test".to_string(), test_data.str,);

        println!("hello from finalizer");

        drop(test_data);
    }

    #[test]
    fn test_callback_on_drop() {
        let mut data = TestData {
            x: 10,
            y: true,
            str: "Test".to_string(),
            finalizer: Box::new(move || {}),
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
        let data_ptr = &mut data as *mut _ as *mut c_void;
        let ctx = unsafe {
            Ctx::new_with_data(
                &mut local_backing,
                &mut import_backing,
                &module,
                data_ptr,
                test_data_finalizer,
            )
        };

        let ctx_test_data = cast_test_data(ctx.data);
        assert_eq!(10, ctx_test_data.x);
        assert_eq!(true, ctx_test_data.y);
        assert_eq!("Test".to_string(), ctx_test_data.str);

        drop(ctx);
    }

    fn cast_test_data(data: *mut c_void) -> &'static mut TestData {
        let test_data: &mut TestData = unsafe { &mut *(data as *mut TestData) };
        test_data
    }

    fn generate_module() -> ModuleInner {
        use super::Func;
        use crate::backend::{sys::Memory, CacheGen, RunnableModule};
        use crate::cache::Error as CacheError;
        use crate::typed_func::Wasm;
        use crate::types::{LocalFuncIndex, SigIndex};
        use indexmap::IndexMap;
        use std::any::Any;
        use std::collections::HashMap;
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
                unimplemented!("generate_module::get_trampoline")
            }
            unsafe fn do_early_trap(&self, _: Box<dyn Any + Send>) -> ! {
                unimplemented!("generate_module::do_early_trap")
            }
        }
        impl CacheGen for Placeholder {
            fn generate_cache(&self) -> Result<(Box<[u8]>, Memory), CacheError> {
                unimplemented!("generate_module::generate_cache")
            }
        }

        ModuleInner {
            runnable_module: Arc::new(Box::new(Placeholder)),
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

                exports: IndexMap::new(),

                data_initializers: Vec::new(),
                elem_initializers: Vec::new(),

                start_func: None,

                func_assoc: Map::new(),
                signatures: Map::new(),
                backend: Default::default(),

                namespace_table: StringTable::new(),
                name_table: StringTable::new(),

                em_symbol_map: None,

                custom_sections: HashMap::new(),

                generate_debug_info: false,
                #[cfg(feature = "generate-debug-information")]
                debug_info_manager: crate::jit_debug::JitCodeDebugInfoManager::new(),
            },
        }
    }
}
