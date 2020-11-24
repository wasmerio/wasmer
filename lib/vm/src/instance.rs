// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

//! An `Instance` contains all the runtime state used by execution of
//! a WebAssembly module (except its callstack and register state). An
//! `InstanceAllocator` is a wrapper around `Instance` that manages
//! how it is allocated and deallocated. An `InstanceHandle` is a
//! wrapper around an `InstanceAllocator`.

use crate::export::Export;
use crate::global::Global;
use crate::imports::Imports;
use crate::memory::{Memory, MemoryError};
use crate::table::Table;
use crate::trap::{catch_traps, init_traps, Trap, TrapCode};
use crate::vmcontext::{
    VMBuiltinFunctionsArray, VMCallerCheckedAnyfunc, VMContext, VMFunctionBody,
    VMFunctionEnvironment, VMFunctionImport, VMFunctionKind, VMGlobalDefinition, VMGlobalImport,
    VMMemoryDefinition, VMMemoryImport, VMSharedSignatureIndex, VMTableDefinition, VMTableImport,
    VMTrampoline,
};
use crate::{ExportFunction, ExportGlobal, ExportMemory, ExportTable};
use crate::{FunctionBodyPtr, ModuleInfo, VMOffsets};
use memoffset::offset_of;
use more_asserts::assert_lt;
use std::alloc::{self, Layout};
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::fmt;
use std::ptr::NonNull;
use std::sync::{atomic, Arc};
use std::{mem, ptr, slice};
use wasmer_types::entity::{packed_option::ReservedValue, BoxedSlice, EntityRef, PrimaryMap};
use wasmer_types::{
    DataIndex, DataInitializer, ElemIndex, ExportIndex, FunctionIndex, GlobalIndex, GlobalInit,
    LocalFunctionIndex, LocalGlobalIndex, LocalMemoryIndex, LocalTableIndex, MemoryIndex, Pages,
    SignatureIndex, TableIndex, TableInitializer,
};

/// A WebAssembly instance.
///
/// The type is dynamically-sized. Indeed, the `vmctx` field can
/// contain various data. That's why the type has a C representation
/// to ensure that the `vmctx` field is last. See the documentation of
/// the `vmctx` field to learn more.
#[repr(C)]
pub(crate) struct Instance {
    /// The `ModuleInfo` this `Instance` was instantiated from.
    module: Arc<ModuleInfo>,

    /// Offsets in the `vmctx` region.
    offsets: VMOffsets,

    /// WebAssembly linear memory data.
    memories: BoxedSlice<LocalMemoryIndex, Arc<dyn Memory>>,

    /// WebAssembly table data.
    tables: BoxedSlice<LocalTableIndex, Arc<dyn Table>>,

    /// WebAssembly global data.
    globals: BoxedSlice<LocalGlobalIndex, Arc<Global>>,

    /// Pointers to functions in executable memory.
    functions: BoxedSlice<LocalFunctionIndex, FunctionBodyPtr>,

    /// Pointers to function call trampolines in executable memory.
    function_call_trampolines: BoxedSlice<SignatureIndex, VMTrampoline>,

    /// Passive elements in this instantiation. As `elem.drop`s happen, these
    /// entries get removed. A missing entry is considered equivalent to an
    /// empty slice.
    passive_elements: RefCell<HashMap<ElemIndex, Box<[VMCallerCheckedAnyfunc]>>>,

    /// Passive data segments from our module. As `data.drop`s happen, entries
    /// get removed. A missing entry is considered equivalent to an empty slice.
    passive_data: RefCell<HashMap<DataIndex, Arc<[u8]>>>,

    /// Hosts can store arbitrary per-instance information here.
    host_state: Box<dyn Any>,

    /// Handler run when `SIGBUS`, `SIGFPE`, `SIGILL`, or `SIGSEGV` are caught by the instance thread.
    pub(crate) signal_handler: Cell<Option<Box<SignalHandler>>>,

    /// Additional context used by compiled WebAssembly code. This
    /// field is last, and represents a dynamically-sized array that
    /// extends beyond the nominal end of the struct (similar to a
    /// flexible array member).
    vmctx: VMContext,
}

impl fmt::Debug for Instance {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.debug_struct("Instance").finish()
    }
}

#[allow(clippy::cast_ptr_alignment)]
impl Instance {
    /// Helper function to access various locations offset from our `*mut
    /// VMContext` object.
    unsafe fn vmctx_plus_offset<T>(&self, offset: u32) -> *mut T {
        (self.vmctx_ptr() as *mut u8)
            .add(usize::try_from(offset).unwrap())
            .cast()
    }

    /// Return the indexed `VMSharedSignatureIndex`.
    fn signature_id(&self, index: SignatureIndex) -> VMSharedSignatureIndex {
        let index = usize::try_from(index.as_u32()).unwrap();
        unsafe { *self.signature_ids_ptr().add(index) }
    }

    fn module(&self) -> &Arc<ModuleInfo> {
        &self.module
    }

    fn module_ref(&self) -> &ModuleInfo {
        &*self.module
    }

    /// Return a pointer to the `VMSharedSignatureIndex`s.
    fn signature_ids_ptr(&self) -> *mut VMSharedSignatureIndex {
        unsafe { self.vmctx_plus_offset(self.offsets.vmctx_signature_ids_begin()) }
    }

    /// Return the indexed `VMFunctionImport`.
    fn imported_function(&self, index: FunctionIndex) -> &VMFunctionImport {
        let index = usize::try_from(index.as_u32()).unwrap();
        unsafe { &*self.imported_functions_ptr().add(index) }
    }

    /// Return a pointer to the `VMFunctionImport`s.
    fn imported_functions_ptr(&self) -> *mut VMFunctionImport {
        unsafe { self.vmctx_plus_offset(self.offsets.vmctx_imported_functions_begin()) }
    }

    /// Return the index `VMTableImport`.
    fn imported_table(&self, index: TableIndex) -> &VMTableImport {
        let index = usize::try_from(index.as_u32()).unwrap();
        unsafe { &*self.imported_tables_ptr().add(index) }
    }

    /// Return a pointer to the `VMTableImports`s.
    fn imported_tables_ptr(&self) -> *mut VMTableImport {
        unsafe { self.vmctx_plus_offset(self.offsets.vmctx_imported_tables_begin()) }
    }

    /// Return the indexed `VMMemoryImport`.
    fn imported_memory(&self, index: MemoryIndex) -> &VMMemoryImport {
        let index = usize::try_from(index.as_u32()).unwrap();
        unsafe { &*self.imported_memories_ptr().add(index) }
    }

    /// Return a pointer to the `VMMemoryImport`s.
    fn imported_memories_ptr(&self) -> *mut VMMemoryImport {
        unsafe { self.vmctx_plus_offset(self.offsets.vmctx_imported_memories_begin()) }
    }

    /// Return the indexed `VMGlobalImport`.
    fn imported_global(&self, index: GlobalIndex) -> &VMGlobalImport {
        let index = usize::try_from(index.as_u32()).unwrap();
        unsafe { &*self.imported_globals_ptr().add(index) }
    }

    /// Return a pointer to the `VMGlobalImport`s.
    fn imported_globals_ptr(&self) -> *mut VMGlobalImport {
        unsafe { self.vmctx_plus_offset(self.offsets.vmctx_imported_globals_begin()) }
    }

    /// Return the indexed `VMTableDefinition`.
    #[allow(dead_code)]
    fn table(&self, index: LocalTableIndex) -> VMTableDefinition {
        unsafe { *self.table_ptr(index).as_ref() }
    }

    #[allow(dead_code)]
    /// Updates the value for a defined table to `VMTableDefinition`.
    fn set_table(&self, index: LocalTableIndex, table: &VMTableDefinition) {
        unsafe {
            *self.table_ptr(index).as_ptr() = *table;
        }
    }

    /// Return the indexed `VMTableDefinition`.
    fn table_ptr(&self, index: LocalTableIndex) -> NonNull<VMTableDefinition> {
        let index = usize::try_from(index.as_u32()).unwrap();
        NonNull::new(unsafe { self.tables_ptr().add(index) }).unwrap()
    }

    /// Return a pointer to the `VMTableDefinition`s.
    fn tables_ptr(&self) -> *mut VMTableDefinition {
        unsafe { self.vmctx_plus_offset(self.offsets.vmctx_tables_begin()) }
    }

    /// Get a locally defined or imported memory.
    fn get_memory(&self, index: MemoryIndex) -> VMMemoryDefinition {
        if let Some(local_index) = self.module.local_memory_index(index) {
            self.memory(local_index)
        } else {
            let import = self.imported_memory(index);
            unsafe { *import.definition.as_ref() }
        }
    }

    /// Return the indexed `VMMemoryDefinition`.
    fn memory(&self, index: LocalMemoryIndex) -> VMMemoryDefinition {
        unsafe { *self.memory_ptr(index).as_ref() }
    }

    #[allow(dead_code)]
    /// Set the indexed memory to `VMMemoryDefinition`.
    fn set_memory(&self, index: LocalMemoryIndex, mem: &VMMemoryDefinition) {
        unsafe {
            *self.memory_ptr(index).as_ptr() = *mem;
        }
    }

    /// Return the indexed `VMMemoryDefinition`.
    fn memory_ptr(&self, index: LocalMemoryIndex) -> NonNull<VMMemoryDefinition> {
        let index = usize::try_from(index.as_u32()).unwrap();
        NonNull::new(unsafe { self.memories_ptr().add(index) }).unwrap()
    }

    /// Return a pointer to the `VMMemoryDefinition`s.
    fn memories_ptr(&self) -> *mut VMMemoryDefinition {
        unsafe { self.vmctx_plus_offset(self.offsets.vmctx_memories_begin()) }
    }

    /// Return the indexed `VMGlobalDefinition`.
    fn global(&self, index: LocalGlobalIndex) -> VMGlobalDefinition {
        unsafe { self.global_ptr(index).as_ref().clone() }
    }

    /// Set the indexed global to `VMGlobalDefinition`.
    #[allow(dead_code)]
    fn set_global(&self, index: LocalGlobalIndex, global: &VMGlobalDefinition) {
        unsafe {
            *self.global_ptr(index).as_ptr() = global.clone();
        }
    }

    /// Return the indexed `VMGlobalDefinition`.
    fn global_ptr(&self, index: LocalGlobalIndex) -> NonNull<VMGlobalDefinition> {
        let index = usize::try_from(index.as_u32()).unwrap();
        // TODO:
        NonNull::new(unsafe { *self.globals_ptr().add(index) }).unwrap()
    }

    /// Return a pointer to the `VMGlobalDefinition`s.
    fn globals_ptr(&self) -> *mut *mut VMGlobalDefinition {
        unsafe { self.vmctx_plus_offset(self.offsets.vmctx_globals_begin()) }
    }

    /// Return a pointer to the `VMBuiltinFunctionsArray`.
    fn builtin_functions_ptr(&self) -> *mut VMBuiltinFunctionsArray {
        unsafe { self.vmctx_plus_offset(self.offsets.vmctx_builtin_functions_begin()) }
    }

    /// Return a reference to the vmctx used by compiled wasm code.
    fn vmctx(&self) -> &VMContext {
        &self.vmctx
    }

    /// Return a raw pointer to the vmctx used by compiled wasm code.
    fn vmctx_ptr(&self) -> *mut VMContext {
        self.vmctx() as *const VMContext as *mut VMContext
    }

    /// Return a reference to the custom state attached to this instance.
    #[inline]
    pub fn host_state(&self) -> &dyn Any {
        &*self.host_state
    }

    /// Invoke the WebAssembly start function of the instance, if one is present.
    fn invoke_start_function(&self) -> Result<(), Trap> {
        let start_index = match self.module.start_function {
            Some(idx) => idx,
            None => return Ok(()),
        };

        let (callee_address, callee_vmctx) = match self.module.local_func_index(start_index) {
            Some(local_index) => {
                let body = self
                    .functions
                    .get(local_index)
                    .expect("function index is out of bounds")
                    .0;
                (
                    body as *const _,
                    VMFunctionEnvironment {
                        vmctx: self.vmctx_ptr(),
                    },
                )
            }
            None => {
                assert_lt!(start_index.index(), self.module.num_imported_functions);
                let import = self.imported_function(start_index);
                (import.body, import.environment)
            }
        };

        // Make the call.
        unsafe {
            catch_traps(callee_vmctx, || {
                mem::transmute::<*const VMFunctionBody, unsafe extern "C" fn(VMFunctionEnvironment)>(
                    callee_address,
                )(callee_vmctx)
            })
        }
    }

    /// Return the offset from the vmctx pointer to its containing `Instance`.
    #[inline]
    pub(crate) fn vmctx_offset() -> isize {
        offset_of!(Self, vmctx) as isize
    }

    /// Return the table index for the given `VMTableDefinition`.
    pub(crate) fn table_index(&self, table: &VMTableDefinition) -> LocalTableIndex {
        let begin: *const VMTableDefinition = self.tables_ptr() as *const _;
        let end: *const VMTableDefinition = table;
        // TODO: Use `offset_from` once it stablizes.
        let index = LocalTableIndex::new(
            (end as usize - begin as usize) / mem::size_of::<VMTableDefinition>(),
        );
        assert_lt!(index.index(), self.tables.len());
        index
    }

    /// Return the memory index for the given `VMMemoryDefinition`.
    pub(crate) fn memory_index(&self, memory: &VMMemoryDefinition) -> LocalMemoryIndex {
        let begin: *const VMMemoryDefinition = self.memories_ptr() as *const _;
        let end: *const VMMemoryDefinition = memory;
        // TODO: Use `offset_from` once it stablizes.
        let index = LocalMemoryIndex::new(
            (end as usize - begin as usize) / mem::size_of::<VMMemoryDefinition>(),
        );
        assert_lt!(index.index(), self.memories.len());
        index
    }

    /// Grow memory by the specified amount of pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of pages.
    pub(crate) fn memory_grow<IntoPages>(
        &self,
        memory_index: LocalMemoryIndex,
        delta: IntoPages,
    ) -> Result<Pages, MemoryError>
    where
        IntoPages: Into<Pages>,
    {
        let mem = self
            .memories
            .get(memory_index)
            .unwrap_or_else(|| panic!("no memory for index {}", memory_index.index()));
        let result = mem.grow(delta.into());

        result
    }

    /// Grow imported memory by the specified amount of pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of pages.
    ///
    /// # Safety
    /// This and `imported_memory_size` are currently unsafe because they
    /// dereference the memory import's pointers.
    pub(crate) unsafe fn imported_memory_grow<IntoPages>(
        &self,
        memory_index: MemoryIndex,
        delta: IntoPages,
    ) -> Result<Pages, MemoryError>
    where
        IntoPages: Into<Pages>,
    {
        let import = self.imported_memory(memory_index);
        let from = import.from.as_ref();
        from.grow(delta.into())
    }

    /// Returns the number of allocated wasm pages.
    pub(crate) fn memory_size(&self, memory_index: LocalMemoryIndex) -> Pages {
        self.memories
            .get(memory_index)
            .unwrap_or_else(|| panic!("no memory for index {}", memory_index.index()))
            .size()
    }

    /// Returns the number of allocated wasm pages in an imported memory.
    ///
    /// # Safety
    /// This and `imported_memory_grow` are currently unsafe because they
    /// dereference the memory import's pointers.
    pub(crate) unsafe fn imported_memory_size(&self, memory_index: MemoryIndex) -> Pages {
        let import = self.imported_memory(memory_index);
        let from = import.from.as_ref();
        from.size()
    }

    /// Grow table by the specified amount of elements.
    ///
    /// Returns `None` if table can't be grown by the specified amount
    /// of elements.
    pub(crate) fn table_grow(&self, table_index: LocalTableIndex, delta: u32) -> Option<u32> {
        let result = self
            .tables
            .get(table_index)
            .unwrap_or_else(|| panic!("no table for index {}", table_index.index()))
            .grow(delta);

        result
    }

    /// Get table element by index.
    fn table_get(
        &self,
        table_index: LocalTableIndex,
        index: u32,
    ) -> Option<VMCallerCheckedAnyfunc> {
        self.tables
            .get(table_index)
            .unwrap_or_else(|| panic!("no table for index {}", table_index.index()))
            .get(index)
    }

    /// Set table element by index.
    fn table_set(
        &self,
        table_index: LocalTableIndex,
        index: u32,
        val: VMCallerCheckedAnyfunc,
    ) -> Result<(), Trap> {
        self.tables
            .get(table_index)
            .unwrap_or_else(|| panic!("no table for index {}", table_index.index()))
            .set(index, val)
    }

    /// Get a `VMCallerCheckedAnyfunc` for the given `FunctionIndex`.
    fn get_caller_checked_anyfunc(&self, index: FunctionIndex) -> VMCallerCheckedAnyfunc {
        if index == FunctionIndex::reserved_value() {
            return VMCallerCheckedAnyfunc::default();
        }

        let sig = self.module.functions[index];
        let type_index = self.signature_id(sig);

        let (func_ptr, vmctx) = if let Some(def_index) = self.module.local_func_index(index) {
            (
                self.functions[def_index].0 as *const _,
                VMFunctionEnvironment {
                    vmctx: self.vmctx_ptr(),
                },
            )
        } else {
            let import = self.imported_function(index);
            (import.body, import.environment)
        };

        VMCallerCheckedAnyfunc {
            func_ptr,
            type_index,
            vmctx,
        }
    }

    /// The `table.init` operation: initializes a portion of a table with a
    /// passive element.
    ///
    /// # Errors
    ///
    /// Returns a `Trap` error when the range within the table is out of bounds
    /// or the range within the passive element is out of bounds.
    pub(crate) fn table_init(
        &self,
        table_index: TableIndex,
        elem_index: ElemIndex,
        dst: u32,
        src: u32,
        len: u32,
    ) -> Result<(), Trap> {
        // https://webassembly.github.io/bulk-memory-operations/core/exec/instructions.html#exec-table-init

        let table = self.get_table(table_index);
        let passive_elements = self.passive_elements.borrow();
        let elem = passive_elements
            .get(&elem_index)
            .map_or_else(|| -> &[VMCallerCheckedAnyfunc] { &[] }, |e| &**e);

        if src
            .checked_add(len)
            .map_or(true, |n| n as usize > elem.len())
            || dst.checked_add(len).map_or(true, |m| m > table.size())
        {
            return Err(Trap::new_from_runtime(TrapCode::TableAccessOutOfBounds));
        }

        for (dst, src) in (dst..dst + len).zip(src..src + len) {
            table
                .set(dst, elem[src as usize].clone())
                .expect("should never panic because we already did the bounds check above");
        }

        Ok(())
    }

    /// Drop an element.
    pub(crate) fn elem_drop(&self, elem_index: ElemIndex) {
        // https://webassembly.github.io/reference-types/core/exec/instructions.html#exec-elem-drop

        let mut passive_elements = self.passive_elements.borrow_mut();
        passive_elements.remove(&elem_index);
        // Note that we don't check that we actually removed an element because
        // dropping a non-passive element is a no-op (not a trap).
    }

    /// Do a `memory.copy` for a locally defined memory.
    ///
    /// # Errors
    ///
    /// Returns a `Trap` error when the source or destination ranges are out of
    /// bounds.
    pub(crate) fn local_memory_copy(
        &self,
        memory_index: LocalMemoryIndex,
        dst: u32,
        src: u32,
        len: u32,
    ) -> Result<(), Trap> {
        // https://webassembly.github.io/reference-types/core/exec/instructions.html#exec-memory-copy

        let memory = self.memory(memory_index);
        // The following memory copy is not synchronized and is not atomic:
        unsafe { memory.memory_copy(dst, src, len) }
    }

    /// Perform a `memory.copy` on an imported memory.
    pub(crate) fn imported_memory_copy(
        &self,
        memory_index: MemoryIndex,
        dst: u32,
        src: u32,
        len: u32,
    ) -> Result<(), Trap> {
        let import = self.imported_memory(memory_index);
        let memory = unsafe { import.definition.as_ref() };
        // The following memory copy is not synchronized and is not atomic:
        unsafe { memory.memory_copy(dst, src, len) }
    }

    /// Perform the `memory.fill` operation on a locally defined memory.
    ///
    /// # Errors
    ///
    /// Returns a `Trap` error if the memory range is out of bounds.
    pub(crate) fn local_memory_fill(
        &self,
        memory_index: LocalMemoryIndex,
        dst: u32,
        val: u32,
        len: u32,
    ) -> Result<(), Trap> {
        let memory = self.memory(memory_index);
        // The following memory fill is not synchronized and is not atomic:
        unsafe { memory.memory_fill(dst, val, len) }
    }

    /// Perform the `memory.fill` operation on an imported memory.
    ///
    /// # Errors
    ///
    /// Returns a `Trap` error if the memory range is out of bounds.
    pub(crate) fn imported_memory_fill(
        &self,
        memory_index: MemoryIndex,
        dst: u32,
        val: u32,
        len: u32,
    ) -> Result<(), Trap> {
        let import = self.imported_memory(memory_index);
        let memory = unsafe { import.definition.as_ref() };
        // The following memory fill is not synchronized and is not atomic:
        unsafe { memory.memory_fill(dst, val, len) }
    }

    /// Performs the `memory.init` operation.
    ///
    /// # Errors
    ///
    /// Returns a `Trap` error if the destination range is out of this module's
    /// memory's bounds or if the source range is outside the data segment's
    /// bounds.
    pub(crate) fn memory_init(
        &self,
        memory_index: MemoryIndex,
        data_index: DataIndex,
        dst: u32,
        src: u32,
        len: u32,
    ) -> Result<(), Trap> {
        // https://webassembly.github.io/bulk-memory-operations/core/exec/instructions.html#exec-memory-init

        let memory = self.get_memory(memory_index);
        let passive_data = self.passive_data.borrow();
        let data = passive_data
            .get(&data_index)
            .map_or(&[][..], |data| &**data);

        if src
            .checked_add(len)
            .map_or(true, |n| n as usize > data.len())
            || dst
                .checked_add(len)
                .map_or(true, |m| m > memory.current_length)
        {
            return Err(Trap::new_from_runtime(TrapCode::HeapAccessOutOfBounds));
        }

        let src_slice = &data[src as usize..(src + len) as usize];

        unsafe {
            let dst_start = memory.base.add(dst as usize);
            let dst_slice = slice::from_raw_parts_mut(dst_start, len as usize);
            dst_slice.copy_from_slice(src_slice);
        }

        Ok(())
    }

    /// Drop the given data segment, truncating its length to zero.
    pub(crate) fn data_drop(&self, data_index: DataIndex) {
        let mut passive_data = self.passive_data.borrow_mut();
        passive_data.remove(&data_index);
    }

    /// Get a table by index regardless of whether it is locally-defined or an
    /// imported, foreign table.
    pub(crate) fn get_table(&self, table_index: TableIndex) -> &dyn Table {
        if let Some(local_table_index) = self.module.local_table_index(table_index) {
            self.get_local_table(local_table_index)
        } else {
            self.get_foreign_table(table_index)
        }
    }

    /// Get a locally-defined table.
    pub(crate) fn get_local_table(&self, index: LocalTableIndex) -> &dyn Table {
        self.tables[index].as_ref()
    }

    /// Get an imported, foreign table.
    pub(crate) fn get_foreign_table(&self, index: TableIndex) -> &dyn Table {
        let import = self.imported_table(index);
        &*import.from
    }
}

/// An `InstanceAllocator` is responsible to allocate, to deallocate,
/// and to give access to an `Instance`, in such a way that `Instance`
/// is unique, can be shared, safely, across threads, without
/// duplicating the pointer. `InstanceAllocator` must be the only
/// owner of an `Instance`.
///
/// Consequently, one must not share `Instance` but
/// `InstanceAllocator` behind a reference-counting pointer. It is
/// designed like that. That's why the astute reader will notice there
/// is only the `Arc<InstanceAllocator>` type in the codebase, never
/// `InstanceAllocator` alone.
///
/// It is important to remind that `Instance` has a dynamic size
/// defined by `VMOffsets`: The `Instance.vmctx` field represents a
/// dynamically-sized array that extends beyond the nominal end of the
/// type. So in order to create an instance of it, we must:
///
/// 1. Define the correct layout for `Instance` (size and alignment),
/// 2. Allocate it properly.
///
/// It must be considered that not only the layout of `Instance` is
/// needed, but the layout of `Arc<InstanceAllocator>`
/// entirely. That's exactly what `InstanceAllocator::instance_layout`
/// does.
///
/// Then `InstanceAllocator::allocate_self` will use this layout
/// to allocate an empty `Arc<InstanceAllocator>` properly. This
/// allocation must be freed with
/// `InstanceAllocator::deallocate_self` if and only if it has
/// been set correctly. The `Drop` implementation of
/// `InstanceAllocator` calls its `deallocate_self` method without
/// checking if this property holds.
///
/// Note for the curious reader: `InstanceHandle::allocate_self`
/// and `InstanceHandle::new` will respectively allocate a proper
/// `Arc<InstanceAllocator>` and will fill it correctly.
///
/// A little bit of background: The initial goal was to be able to
/// shared an `Instance` between an `InstanceHandle` and the module
/// exports, so that one can drop a `InstanceHandle` but still being
/// able to use the exports properly.
///
/// This structure has a C representation because `Instance` is
/// dynamically-sized, and the `instance` field must be last.
#[derive(Debug)]
#[repr(C)]
pub struct InstanceAllocator {
    /// The pointer to the allocation for `Self`, from the
    /// `Self::allocate_self` method.
    //self_ptr: *const Self,
    strong: Arc<atomic::AtomicUsize>,

    /// The layout of `Self` (which can vary depending of the
    /// `Instance`'s layout).
    instance_layout: Layout,

    /// The `Instance` itself. It must be the last field of
    /// `InstanceAllocator` since `Instance` is dyamically-sized.
    ///
    /// `Instance` must not be dropped manually by Rust, because it's
    /// allocated manually with `alloc` and a specific layout (Rust
    /// would be able to drop `Instance` itself but it will imply a
    /// memory leak because of `alloc`), hence the `ManuallyDrop`.
    instance: NonNull<Instance>,
}

impl InstanceAllocator {
    /// Create a new `InstanceAllocator`. It allocates nothing. It
    /// fills nothing. The `Instance` must be already valid and
    /// filled. `self_ptr` and `self_layout` must be the pointer and
    /// the layout returned by `Self::allocate_self` used to build
    /// `Self`.
    #[inline(always)]
    fn new(
        instance: NonNull<Instance>,
        /*self_ptr: *const Self, */ instance_layout: Layout,
    ) -> Self {
        Self {
            //self_ptr,
            strong: Arc::new(atomic::AtomicUsize::new(1)),
            instance_layout,
            instance,
        }
    }

    /// Calculate the appropriate layout for `Self`.
    fn instance_layout(offsets: &VMOffsets) -> Layout {
        let size = mem::size_of::<Instance>()
            .checked_add(
                usize::try_from(offsets.size_of_vmctx())
                    .expect("Failed to convert the size of `vmctx` to a `usize`"),
            )
            .unwrap();
        let align = mem::align_of::<Instance>();

        Layout::from_size_align(size, align).unwrap()
    }

    /// Allocate `Self`.
    ///
    /// `offsets` is used to compute the layout with `Self::instance_layout`.
    fn allocate_instance(offsets: &VMOffsets) -> (NonNull<Instance>, Layout) {
        let layout = Self::instance_layout(offsets);

        #[allow(clippy::cast_ptr_alignment)]
        let instance_ptr = unsafe { alloc::alloc(layout) as *mut Instance };

        let ptr = if let Some(ptr) = NonNull::new(instance_ptr) {
            ptr
        } else {
            alloc::handle_alloc_error(layout);
        };

        (ptr, layout)
    }

    /// Deallocate `Self`.
    ///
    /// # Safety
    ///
    /// `Self` must be correctly set and filled before being dropped
    /// and deallocated.
    unsafe fn deallocate_instance(&mut self) {
        //mem::ManuallyDrop::drop(&mut self.instance);
        let instance_ptr = self.instance.as_ptr();
        ptr::drop_in_place(instance_ptr);
        std::alloc::dealloc(instance_ptr as *mut u8, self.instance_layout);
    }

    /// Get a reference to the `Instance`.
    #[inline(always)]
    pub(crate) fn instance_ref(&self) -> &Instance {
        unsafe { self.instance.as_ref() }
    }

    fn instance_ptr(&self) -> *const Instance {
        self.instance.as_ptr()
    }
}

impl Clone for InstanceAllocator {
    #[inline]
    fn clone(&self) -> Self {
        let _old_size = self.strong.fetch_add(1, atomic::Ordering::Relaxed);

        /*
        let old_size > MAX_REFCOUNT {
            abort();
        }
        */

        Self {
            //self_ptr: self.self_ptr,
            strong: self.strong.clone(),
            instance_layout: self.instance_layout,
            instance: self.instance.clone(),
        }
    }
}

impl PartialEq for InstanceAllocator {
    fn eq(&self, other: &Self) -> bool {
        self.instance == other.instance
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        println!("Dropping `wasmer_vm::Instance`");
    }
}

impl Drop for InstanceAllocator {
    fn drop(&mut self) {
        println!("Trying to drop `wasmer_vm::InstanceAllocator`...");

        if self.strong.fetch_sub(1, atomic::Ordering::Release) != 1 {
            println!("... not now");

            return;
        }

        println!("Yep, dropping it.");

        atomic::fence(atomic::Ordering::Acquire);

        unsafe { Self::deallocate_instance(self) };
    }
}

/// A handle holding an `Instance` of a WebAssembly module.
//#[derive(Hash, PartialEq, Eq)]
pub struct InstanceHandle {
    instance: InstanceAllocator,
}

/// # Safety
/// This is safe because there is no thread-specific logic in `InstanceHandle`.
/// TODO: this needs extra review
unsafe impl Send for InstanceHandle {}

impl InstanceHandle {
    /// Allocates an instance for use with `InstanceHandle::new`.
    ///
    /// Returns the instance pointer and the [`VMOffsets`] that describe the
    /// memory buffer pointed to by the instance pointer.
    pub fn allocate_instance(module: &ModuleInfo) -> (NonNull<u8>, VMOffsets) {
        let offsets = VMOffsets::new(mem::size_of::<*const u8>() as u8, module);
        let (instance_ptr, _instance_layout) = InstanceAllocator::allocate_instance(&offsets);

        (instance_ptr.cast(), offsets)
    }

    /// Create a new `InstanceHandle` pointing at a new `Instance`.
    ///
    /// # Safety
    ///
    /// This method is not necessarily inherently unsafe to call, but in general
    /// the APIs of an `Instance` are quite unsafe and have not been really
    /// audited for safety that much. As a result the unsafety here on this
    /// method is a low-overhead way of saying "this is an extremely unsafe type
    /// to work with".
    ///
    /// Extreme care must be taken when working with `InstanceHandle` and it's
    /// recommended to have relatively intimate knowledge of how it works
    /// internally if you'd like to do so. If possible it's recommended to use
    /// the `wasmer` crate API rather than this type since that is vetted for
    /// safety.
    ///
    /// However the following must be taken care of before calling this function:
    /// - `instance_ptr` must point to valid memory sufficiently large
    ///    for the `Instance`. `instance_ptr` will be owned by
    ///    `InstanceHandle`, see `InstanceHandle::owned_instance` to
    ///    learn more.
    /// - The memory at `instance.tables_ptr()` must be initialized with data for
    ///   all the local tables.
    /// - The memory at `instance.memories_ptr()` must be initialized with data for
    ///   all the local memories.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn new(
        instance_ptr: NonNull<u8>,
        offsets: VMOffsets,
        module: Arc<ModuleInfo>,
        finished_functions: BoxedSlice<LocalFunctionIndex, FunctionBodyPtr>,
        finished_function_call_trampolines: BoxedSlice<SignatureIndex, VMTrampoline>,
        finished_memories: BoxedSlice<LocalMemoryIndex, Arc<dyn Memory>>,
        finished_tables: BoxedSlice<LocalTableIndex, Arc<dyn Table>>,
        finished_globals: BoxedSlice<LocalGlobalIndex, Arc<Global>>,
        imports: Imports,
        vmshared_signatures: BoxedSlice<SignatureIndex, VMSharedSignatureIndex>,
        host_state: Box<dyn Any>,
    ) -> Result<Self, Trap> {
        let instance_ptr: NonNull<Instance> = instance_ptr.cast();
        let vmctx_globals = finished_globals
            .values()
            .map(|m| m.vmglobal())
            .collect::<PrimaryMap<LocalGlobalIndex, _>>()
            .into_boxed_slice();
        let passive_data = RefCell::new(module.passive_data.clone());

        let handle = {
            dbg!(instance_ptr);
            let instance_layout = InstanceAllocator::instance_layout(&offsets);
            let instance = Instance {
                module,
                offsets,
                memories: finished_memories,
                tables: finished_tables,
                globals: finished_globals,
                functions: finished_functions,
                function_call_trampolines: finished_function_call_trampolines,
                passive_elements: Default::default(),
                passive_data,
                host_state,
                signal_handler: Cell::new(None),
                vmctx: VMContext {},
            };

            ptr::write(instance_ptr.as_ptr(), instance);

            let instance_allocator = InstanceAllocator::new(instance_ptr, instance_layout);

            /*
            let instance_ptr = Arc::as_ptr(&arc_instance);
            dbg!(instance_ptr);
            debug_assert_eq!(Arc::strong_count(&arc_instance), 1);

            ptr::write(arc_instance_allocator_ptr, arc_instance);

            let instance = Arc::from_raw(instance_ptr);
            debug_assert_eq!(Arc::strong_count(&instance), 1);
            */

            Self {
                instance: instance_allocator,
            }
        };
        let instance = handle.instance().instance_ref();

        ptr::copy(
            vmshared_signatures.values().as_slice().as_ptr(),
            instance.signature_ids_ptr() as *mut VMSharedSignatureIndex,
            vmshared_signatures.len(),
        );
        ptr::copy(
            imports.functions.values().as_slice().as_ptr(),
            instance.imported_functions_ptr() as *mut VMFunctionImport,
            imports.functions.len(),
        );
        ptr::copy(
            imports.tables.values().as_slice().as_ptr(),
            instance.imported_tables_ptr() as *mut VMTableImport,
            imports.tables.len(),
        );
        ptr::copy(
            imports.memories.values().as_slice().as_ptr(),
            instance.imported_memories_ptr() as *mut VMMemoryImport,
            imports.memories.len(),
        );
        ptr::copy(
            imports.globals.values().as_slice().as_ptr(),
            instance.imported_globals_ptr() as *mut VMGlobalImport,
            imports.globals.len(),
        );
        // these should already be set, add asserts here? for:
        // - instance.tables_ptr() as *mut VMTableDefinition
        // - instance.memories_ptr() as *mut VMMemoryDefinition
        ptr::copy(
            vmctx_globals.values().as_slice().as_ptr(),
            instance.globals_ptr() as *mut NonNull<VMGlobalDefinition>,
            vmctx_globals.len(),
        );
        ptr::write(
            instance.builtin_functions_ptr() as *mut VMBuiltinFunctionsArray,
            VMBuiltinFunctionsArray::initialized(),
        );

        // Ensure that our signal handlers are ready for action.
        init_traps();

        // Perform infallible initialization in this constructor, while fallible
        // initialization is deferred to the `initialize` method.
        initialize_passive_elements(instance);
        initialize_globals(instance);

        Ok(handle)
    }

    /// Create a new `InstanceHandle` pointing at the instance
    /// pointed to by the given `VMContext` pointer.
    ///
    /// # Safety
    /// This is unsafe because it doesn't work on just any `VMContext`, it must
    /// be a `VMContext` allocated as part of an `Instance`.
    pub(crate) unsafe fn from_vmctx(vmctx: *const VMContext) -> Self {
        /*
        let instance = (&*vmctx).instance();

        Self {
            instance: Arc::from_raw(instance as *const InstanceAllocator),
            // We consider `vmctx` owns the `Instance`, not `Self`.
            //owned_instance: false,
        }
        */
        todo!()
    }

    /// Return a reference to the contained `Instance`.
    pub(crate) fn instance(&self) -> &InstanceAllocator {
        &self.instance
    }

    /// Get the locations of where the local `VMMemoryDefinition`s should be stored.
    ///
    /// This function lets us create `Memory` objects on the host with backing
    /// memory in the VM.
    ///
    /// # Safety
    /// - `instance_ptr` must point to enough memory that all of the offsets in
    ///   `offsets` point to valid locations in memory.
    pub unsafe fn memory_definition_locations(
        instance_ptr: NonNull<u8>,
        offsets: &VMOffsets,
    ) -> Vec<NonNull<VMMemoryDefinition>> {
        let instance_ptr: NonNull<Instance> = instance_ptr.cast();
        let num_memories = offsets.num_local_memories;
        let num_memories = usize::try_from(num_memories).unwrap();
        let mut out = Vec::with_capacity(num_memories);

        let base_ptr = instance_ptr.as_ptr().add(mem::size_of::<Instance>());

        for i in 0..num_memories {
            let mem_offset = offsets.vmctx_vmmemory_definition(LocalMemoryIndex::new(i));
            let mem_offset = usize::try_from(mem_offset).unwrap();

            let new_ptr = NonNull::new_unchecked(base_ptr.add(mem_offset));

            out.push(new_ptr.cast());
        }

        out
    }

    /// Get the locations of where the `VMTableDefinition`s should be stored.
    ///
    /// This function lets us create `Table` objects on the host with backing
    /// memory in the VM.
    ///
    /// # Safety
    /// - `instance_ptr` must point to enough memory that all of the offsets in
    ///   `offsets` point to valid locations in memory.
    pub unsafe fn table_definition_locations(
        instance_ptr: NonNull<u8>,
        offsets: &VMOffsets,
    ) -> Vec<NonNull<VMTableDefinition>> {
        let instance_ptr: NonNull<Instance> = instance_ptr.cast();
        let num_tables = offsets.num_local_tables;
        let num_tables = usize::try_from(num_tables).unwrap();
        let mut out = Vec::with_capacity(num_tables);

        let base_ptr = instance_ptr.as_ptr().add(std::mem::size_of::<Instance>());

        for i in 0..num_tables {
            let table_offset = offsets.vmctx_vmtable_definition(LocalTableIndex::new(i));
            let table_offset = usize::try_from(table_offset).unwrap();

            let new_ptr = NonNull::new_unchecked(base_ptr.add(table_offset));

            out.push(new_ptr.cast());
        }
        out
    }

    /// Finishes the instantiation process started by `Instance::new`.
    ///
    /// # Safety
    ///
    /// Only safe to call immediately after instantiation.
    pub unsafe fn finish_instantiation(
        &self,
        data_initializers: &[DataInitializer<'_>],
    ) -> Result<(), Trap> {
        let instance = self.instance().instance_ref();
        check_table_init_bounds(instance)?;
        check_memory_init_bounds(instance, data_initializers)?;

        // Apply the initializers.
        initialize_tables(instance)?;
        initialize_memories(instance, data_initializers)?;

        // The WebAssembly spec specifies that the start function is
        // invoked automatically at instantiation time.
        instance.invoke_start_function()?;
        Ok(())
    }

    /// Return a reference to the vmctx used by compiled wasm code.
    pub fn vmctx(&self) -> &VMContext {
        self.instance().instance_ref().vmctx()
    }

    /// Return a raw pointer to the vmctx used by compiled wasm code.
    pub fn vmctx_ptr(&self) -> *mut VMContext {
        self.instance().instance_ref().vmctx_ptr()
    }

    /// Return a reference-counting pointer to a module.
    pub fn module(&self) -> &Arc<ModuleInfo> {
        self.instance().instance_ref().module()
    }

    /// Return a reference to a module.
    pub fn module_ref(&self) -> &ModuleInfo {
        self.instance().instance_ref().module_ref()
    }

    /// Lookup an export with the given name.
    pub fn lookup(&self, field: &str) -> Option<Export> {
        let export = self.module().exports.get(field)?;

        Some(self.lookup_by_declaration(&export))
    }

    /// Lookup an export with the given export declaration.
    pub fn lookup_by_declaration(&self, export: &ExportIndex) -> Export {
        let instance = self.instance().clone();
        let instance_ref = instance.instance_ref();

        match export {
            ExportIndex::Function(index) => {
                let sig_index = &instance_ref.module.functions[*index];
                let (address, vmctx) =
                    if let Some(def_index) = instance_ref.module.local_func_index(*index) {
                        (
                            instance_ref.functions[def_index].0 as *const _,
                            VMFunctionEnvironment {
                                vmctx: instance_ref.vmctx_ptr(),
                            },
                        )
                    } else {
                        let import = instance_ref.imported_function(*index);
                        (import.body, import.environment)
                    };
                let call_trampoline = Some(instance_ref.function_call_trampolines[*sig_index]);
                let signature = instance_ref.module.signatures[*sig_index].clone();

                ExportFunction {
                    address,
                    // Any function received is already static at this point as:
                    // 1. All locally defined functions in the Wasm have a static signature.
                    // 2. All the imported functions are already static (because
                    //    they point to the trampolines rather than the dynamic addresses).
                    kind: VMFunctionKind::Static,
                    signature,
                    vmctx,
                    call_trampoline,
                    instance_allocator: Some(instance),
                }
                .into()
            }

            ExportIndex::Table(index) => {
                let from = if let Some(def_index) = instance_ref.module.local_table_index(*index) {
                    instance_ref.tables[def_index].clone()
                } else {
                    let import = instance_ref.imported_table(*index);
                    import.from.clone()
                };

                ExportTable {
                    from,
                    instance_allocator: Some(instance),
                }
                .into()
            }

            ExportIndex::Memory(index) => {
                let from = if let Some(def_index) = instance_ref.module.local_memory_index(*index) {
                    instance_ref.memories[def_index].clone()
                } else {
                    let import = instance_ref.imported_memory(*index);
                    import.from.clone()
                };

                ExportMemory {
                    from,
                    instance_allocator: Some(instance),
                }
                .into()
            }

            ExportIndex::Global(index) => {
                let from = {
                    if let Some(def_index) = instance_ref.module.local_global_index(*index) {
                        instance_ref.globals[def_index].clone()
                    } else {
                        let import = instance_ref.imported_global(*index);
                        import.from.clone()
                    }
                };

                ExportGlobal {
                    from,
                    instance_allocator: Some(instance),
                }
                .into()
            }
        }
    }

    /// Return an iterator over the exports of this instance.
    ///
    /// Specifically, it provides access to the key-value pairs, where the keys
    /// are export names, and the values are export declarations which can be
    /// resolved `lookup_by_declaration`.
    pub fn exports(&self) -> indexmap::map::Iter<String, ExportIndex> {
        self.module().exports.iter()
    }

    /// Return a reference to the custom state attached to this instance.
    pub fn host_state(&self) -> &dyn Any {
        self.instance().instance_ref().host_state()
    }

    /// Return the memory index for the given `VMMemoryDefinition` in this instance.
    pub fn memory_index(&self, memory: &VMMemoryDefinition) -> LocalMemoryIndex {
        self.instance().instance_ref().memory_index(memory)
    }

    /// Grow memory in this instance by the specified amount of pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of pages.
    pub fn memory_grow<IntoPages>(
        &self,
        memory_index: LocalMemoryIndex,
        delta: IntoPages,
    ) -> Result<Pages, MemoryError>
    where
        IntoPages: Into<Pages>,
    {
        self.instance()
            .instance_ref()
            .memory_grow(memory_index, delta)
    }

    /// Return the table index for the given `VMTableDefinition` in this instance.
    pub fn table_index(&self, table: &VMTableDefinition) -> LocalTableIndex {
        self.instance().instance_ref().table_index(table)
    }

    /// Grow table in this instance by the specified amount of pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of pages.
    pub fn table_grow(&self, table_index: LocalTableIndex, delta: u32) -> Option<u32> {
        self.instance()
            .instance_ref()
            .table_grow(table_index, delta)
    }

    /// Get table element reference.
    ///
    /// Returns `None` if index is out of bounds.
    pub fn table_get(
        &self,
        table_index: LocalTableIndex,
        index: u32,
    ) -> Option<VMCallerCheckedAnyfunc> {
        self.instance().instance_ref().table_get(table_index, index)
    }

    /// Set table element reference.
    ///
    /// Returns an error if the index is out of bounds
    pub fn table_set(
        &self,
        table_index: LocalTableIndex,
        index: u32,
        val: VMCallerCheckedAnyfunc,
    ) -> Result<(), Trap> {
        self.instance()
            .instance_ref()
            .table_set(table_index, index, val)
    }

    /// Get a table defined locally within this module.
    pub fn get_local_table(&self, index: LocalTableIndex) -> &dyn Table {
        self.instance().instance_ref().get_local_table(index)
    }
}

cfg_if::cfg_if! {
    if #[cfg(unix)] {
        pub type SignalHandler = dyn Fn(libc::c_int, *const libc::siginfo_t, *const libc::c_void) -> bool;

        impl InstanceHandle {
            /// Set a custom signal handler
            pub fn set_signal_handler<H>(&self, handler: H)
            where
                H: 'static + Fn(libc::c_int, *const libc::siginfo_t, *const libc::c_void) -> bool,
            {
                self.instance().instance_ref().signal_handler.set(Some(Box::new(handler)));
            }
        }
    } else if #[cfg(target_os = "windows")] {
        pub type SignalHandler = dyn Fn(winapi::um::winnt::PEXCEPTION_POINTERS) -> bool;

        impl InstanceHandle {
            /// Set a custom signal handler
            pub fn set_signal_handler<H>(&self, handler: H)
            where
                H: 'static + Fn(winapi::um::winnt::PEXCEPTION_POINTERS) -> bool,
            {
                self.instance().instance_ref().signal_handler.set(Some(Box::new(handler)));
            }
        }
    }
}

fn check_table_init_bounds(instance: &Instance) -> Result<(), Trap> {
    let module = Arc::clone(&instance.module);
    for init in &module.table_initializers {
        let start = get_table_init_start(init, instance);
        let table = instance.get_table(init.table_index);

        let size = usize::try_from(table.size()).unwrap();
        if size < start + init.elements.len() {
            return Err(Trap::new_from_runtime(TrapCode::TableSetterOutOfBounds));
        }
    }

    Ok(())
}

/// Compute the offset for a memory data initializer.
fn get_memory_init_start(init: &DataInitializer<'_>, instance: &Instance) -> usize {
    let mut start = init.location.offset;

    if let Some(base) = init.location.base {
        let val = unsafe {
            if let Some(def_index) = instance.module.local_global_index(base) {
                instance.global(def_index).to_u32()
            } else {
                instance.imported_global(base).definition.as_ref().to_u32()
            }
        };
        start += usize::try_from(val).unwrap();
    }

    start
}

#[allow(clippy::mut_from_ref)]
/// Return a byte-slice view of a memory's data.
unsafe fn get_memory_slice<'instance>(
    init: &DataInitializer<'_>,
    instance: &'instance Instance,
) -> &'instance mut [u8] {
    let memory = if let Some(local_memory_index) = instance
        .module
        .local_memory_index(init.location.memory_index)
    {
        instance.memory(local_memory_index)
    } else {
        let import = instance.imported_memory(init.location.memory_index);
        *import.definition.as_ref()
    };
    slice::from_raw_parts_mut(memory.base, memory.current_length.try_into().unwrap())
}

fn check_memory_init_bounds(
    instance: &Instance,
    data_initializers: &[DataInitializer<'_>],
) -> Result<(), Trap> {
    for init in data_initializers {
        let start = get_memory_init_start(init, instance);
        unsafe {
            let mem_slice = get_memory_slice(init, instance);
            if mem_slice.get_mut(start..start + init.data.len()).is_none() {
                return Err(Trap::new_from_runtime(TrapCode::HeapSetterOutOfBounds));
            }
        }
    }

    Ok(())
}

/// Compute the offset for a table element initializer.
fn get_table_init_start(init: &TableInitializer, instance: &Instance) -> usize {
    let mut start = init.offset;

    if let Some(base) = init.base {
        let val = unsafe {
            if let Some(def_index) = instance.module.local_global_index(base) {
                instance.global(def_index).to_u32()
            } else {
                instance.imported_global(base).definition.as_ref().to_u32()
            }
        };
        start += usize::try_from(val).unwrap();
    }

    start
}

/// Initialize the table memory from the provided initializers.
fn initialize_tables(instance: &Instance) -> Result<(), Trap> {
    let module = Arc::clone(&instance.module);
    for init in &module.table_initializers {
        let start = get_table_init_start(init, instance);
        let table = instance.get_table(init.table_index);

        if start
            .checked_add(init.elements.len())
            .map_or(true, |end| end > table.size() as usize)
        {
            return Err(Trap::new_from_runtime(TrapCode::TableAccessOutOfBounds));
        }

        for (i, func_idx) in init.elements.iter().enumerate() {
            let anyfunc = instance.get_caller_checked_anyfunc(*func_idx);
            table
                .set(u32::try_from(start + i).unwrap(), anyfunc)
                .unwrap();
        }
    }

    Ok(())
}

/// Initialize the `Instance::passive_elements` map by resolving the
/// `ModuleInfo::passive_elements`'s `FunctionIndex`s into `VMCallerCheckedAnyfunc`s for
/// this instance.
fn initialize_passive_elements(instance: &Instance) {
    let mut passive_elements = instance.passive_elements.borrow_mut();
    debug_assert!(
        passive_elements.is_empty(),
        "should only be called once, at initialization time"
    );

    passive_elements.extend(
        instance
            .module
            .passive_elements
            .iter()
            .filter(|(_, segments)| !segments.is_empty())
            .map(|(idx, segments)| {
                (
                    *idx,
                    segments
                        .iter()
                        .map(|s| instance.get_caller_checked_anyfunc(*s))
                        .collect(),
                )
            }),
    );
}

/// Initialize the table memory from the provided initializers.
fn initialize_memories(
    instance: &Instance,
    data_initializers: &[DataInitializer<'_>],
) -> Result<(), Trap> {
    for init in data_initializers {
        let memory = instance.get_memory(init.location.memory_index);

        let start = get_memory_init_start(init, instance);
        if start
            .checked_add(init.data.len())
            .map_or(true, |end| end > memory.current_length.try_into().unwrap())
        {
            return Err(Trap::new_from_runtime(TrapCode::HeapAccessOutOfBounds));
        }

        unsafe {
            let mem_slice = get_memory_slice(init, instance);
            let end = start + init.data.len();
            let to_init = &mut mem_slice[start..end];
            to_init.copy_from_slice(init.data);
        }
    }

    Ok(())
}

fn initialize_globals(instance: &Instance) {
    let module = Arc::clone(&instance.module);
    for (index, initializer) in module.global_initializers.iter() {
        unsafe {
            let to = instance.global_ptr(index).as_ptr();
            match initializer {
                GlobalInit::I32Const(x) => *(*to).as_i32_mut() = *x,
                GlobalInit::I64Const(x) => *(*to).as_i64_mut() = *x,
                GlobalInit::F32Const(x) => *(*to).as_f32_mut() = *x,
                GlobalInit::F64Const(x) => *(*to).as_f64_mut() = *x,
                GlobalInit::V128Const(x) => *(*to).as_bytes_mut() = *x.bytes(),
                GlobalInit::GetGlobal(x) => {
                    let from: VMGlobalDefinition =
                        if let Some(def_x) = module.local_global_index(*x) {
                            instance.global(def_x)
                        } else {
                            instance.imported_global(*x).definition.as_ref().clone()
                        };
                    *to = from;
                }
                GlobalInit::RefNullConst | GlobalInit::RefFunc(_) => unimplemented!(),
            }
        }
    }
}
