// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

//! An `Instance` contains all the runtime state used by execution of a
//! wasm module (except its callstack and register state). An
//! `InstanceHandle` is a reference-counting handle for an `Instance`.
use crate::export::Export;
use crate::global::Global;
use crate::imports::Imports;
use crate::memory::{Memory, MemoryError};
use crate::table::Table;
use crate::trap::{catch_traps, init_traps, Trap, TrapCode};
use crate::vmcontext::{
    VMBuiltinFunctionsArray, VMCallerCheckedAnyfunc, VMContext, VMFunctionBody, VMFunctionImport,
    VMFunctionKind, VMGlobalDefinition, VMGlobalImport, VMMemoryDefinition, VMMemoryImport,
    VMSharedSignatureIndex, VMTableDefinition, VMTableImport,
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
use std::ptr::NonNull;
use std::sync::Arc;
use std::{mem, ptr, slice};
use wasmer_types::entity::{packed_option::ReservedValue, BoxedSlice, EntityRef, PrimaryMap};
use wasmer_types::{
    DataIndex, DataInitializer, ElemIndex, ExportIndex, FunctionIndex, GlobalIndex, GlobalInit,
    LocalFunctionIndex, LocalGlobalIndex, LocalMemoryIndex, LocalTableIndex, MemoryIndex, Pages,
    SignatureIndex, TableIndex, TableInitializer,
};

cfg_if::cfg_if! {
    if #[cfg(unix)] {
        pub type SignalHandler = dyn Fn(libc::c_int, *const libc::siginfo_t, *const libc::c_void) -> bool;

        impl InstanceHandle {
            /// Set a custom signal handler
            pub fn set_signal_handler<H>(&self, handler: H)
            where
                H: 'static + Fn(libc::c_int, *const libc::siginfo_t, *const libc::c_void) -> bool,
            {
                self.instance().signal_handler.set(Some(Box::new(handler)));
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
                self.instance().signal_handler.set(Some(Box::new(handler)));
            }
        }
    }
}

/// A WebAssembly instance.
///
/// This is repr(C) to ensure that the vmctx field is last.
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

    /// Additional context used by compiled wasm code. This field is last, and
    /// represents a dynamically-sized array that extends beyond the nominal
    /// end of the struct (similar to a flexible array member).
    vmctx: VMContext,
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

    pub(crate) fn module(&self) -> &Arc<ModuleInfo> {
        &self.module
    }

    pub(crate) fn module_ref(&self) -> &ModuleInfo {
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
        unsafe { self.table_ptr(index).as_ref().clone() }
    }

    /// Updates the value for a defined table to `VMTableDefinition`.
    fn set_table(&self, index: LocalTableIndex, table: &VMTableDefinition) {
        unsafe {
            *self.table_ptr(index).as_ptr() = table.clone();
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
    pub(crate) fn get_memory(&self, index: MemoryIndex) -> VMMemoryDefinition {
        if let Some(local_index) = self.module.local_memory_index(index) {
            self.memory(local_index)
        } else {
            let import = self.imported_memory(index);
            unsafe { import.definition.as_ref().clone() }
        }
    }

    /// Return the indexed `VMMemoryDefinition`.
    fn memory(&self, index: LocalMemoryIndex) -> VMMemoryDefinition {
        unsafe { self.memory_ptr(index).as_ref().clone() }
    }

    /// Set the indexed memory to `VMMemoryDefinition`.
    fn set_memory(&self, index: LocalMemoryIndex, mem: &VMMemoryDefinition) {
        unsafe {
            *self.memory_ptr(index).as_ptr() = mem.clone();
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
    pub fn vmctx(&self) -> &VMContext {
        &self.vmctx
    }

    /// Return a raw pointer to the vmctx used by compiled wasm code.
    pub fn vmctx_ptr(&self) -> *const VMContext {
        self.vmctx() as *const VMContext
    }

    /// Lookup an export with the given name.
    pub fn lookup(&self, field: &str) -> Option<Export> {
        let export = if let Some(export) = self.module.exports.get(field) {
            export.clone()
        } else {
            return None;
        };
        Some(self.lookup_by_declaration(&export))
    }

    /// Lookup an export with the given export declaration.
    pub fn lookup_by_declaration(&self, export: &ExportIndex) -> Export {
        match export {
            ExportIndex::Function(index) => {
                let sig_index = &self.module.functions[*index];
                let (address, vmctx) = if let Some(def_index) = self.module.local_func_index(*index)
                {
                    (self.functions[def_index].0 as *const _, self.vmctx_ptr())
                } else {
                    let import = self.imported_function(*index);
                    (import.body, import.vmctx)
                };
                let signature = self.module.signatures[*sig_index].clone();
                ExportFunction {
                    address,
                    // Any function received is already static at this point as:
                    // 1. All locally defined functions in the Wasm have a static signature.
                    // 2. All the imported functions are already static (because
                    //    they point to the trampolines rather than the dynamic addresses).
                    kind: VMFunctionKind::Static,
                    signature,
                    vmctx,
                }
                .into()
            }
            ExportIndex::Table(index) => {
                let from = if let Some(def_index) = self.module.local_table_index(*index) {
                    self.tables[def_index].clone()
                } else {
                    let import = self.imported_table(*index);
                    import.from.clone()
                };
                ExportTable { from }.into()
            }
            ExportIndex::Memory(index) => {
                let from = if let Some(def_index) = self.module.local_memory_index(*index) {
                    self.memories[def_index].clone()
                } else {
                    let import = self.imported_memory(*index);
                    import.from.clone()
                };
                ExportMemory { from }.into()
            }
            ExportIndex::Global(index) => {
                let from = {
                    if let Some(def_index) = self.module.local_global_index(*index) {
                        self.globals[def_index].clone()
                    } else {
                        let import = self.imported_global(*index);
                        import.from.clone()
                    }
                };
                ExportGlobal { from }.into()
            }
        }
    }

    /// Return an iterator over the exports of this instance.
    ///
    /// Specifically, it provides access to the key-value pairs, where the keys
    /// are export names, and the values are export declarations which can be
    /// resolved `lookup_by_declaration`.
    pub fn exports(&self) -> indexmap::map::Iter<String, ExportIndex> {
        self.module.exports.iter()
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
                (body as *const _, self.vmctx_ptr())
            }
            None => {
                assert_lt!(start_index.index(), self.module.num_imported_functions);
                let import = self.imported_function(start_index);
                (import.body, import.vmctx)
            }
        };

        // Make the call.
        unsafe {
            catch_traps(callee_vmctx, || {
                mem::transmute::<*const VMFunctionBody, unsafe extern "C" fn(*const VMContext)>(
                    callee_address,
                )(callee_vmctx)
            })
        }
    }

    /// Return the offset from the vmctx pointer to its containing Instance.
    #[inline]
    pub(crate) fn vmctx_offset() -> isize {
        offset_of!(Self, vmctx) as isize
    }

    /// Return the table index for the given `VMTableDefinition`.
    pub(crate) fn table_index(&self, table: &VMTableDefinition) -> LocalTableIndex {
        let offsets = &self.offsets;
        let begin = unsafe {
            (&self.vmctx as *const VMContext as *const u8)
                .add(usize::try_from(offsets.vmctx_tables_begin()).unwrap())
        } as *const VMTableDefinition;
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
        let offsets = &self.offsets;
        let begin = unsafe {
            (&self.vmctx as *const VMContext as *const u8)
                .add(usize::try_from(offsets.vmctx_memories_begin()).unwrap())
        } as *const VMMemoryDefinition;
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

        // Keep current the VMContext pointers used by compiled wasm code.
        let memory_ptr = self.memories[memory_index].vmmemory();
        let vmmemory = unsafe { memory_ptr.as_ref() };
        self.set_memory(memory_index, vmmemory);

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

        // Keep current the VMContext pointers used by compiled wasm code.
        let table_ptr = self.tables[table_index].vmtable();
        let vmtable = unsafe { table_ptr.as_ref() };
        self.set_table(table_index, vmtable);

        result
    }

    // Get table element by index.
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

    fn alloc_layout(&self) -> Layout {
        let size = mem::size_of_val(self)
            .checked_add(usize::try_from(self.offsets.size_of_vmctx()).unwrap())
            .unwrap();
        let align = mem::align_of_val(self);
        Layout::from_size_align(size, align).unwrap()
    }

    /// Get a `VMCallerCheckedAnyfunc` for the given `FunctionIndex`.
    fn get_caller_checked_anyfunc(&self, index: FunctionIndex) -> VMCallerCheckedAnyfunc {
        if index == FunctionIndex::reserved_value() {
            return VMCallerCheckedAnyfunc::default();
        }

        let sig = self.module.functions[index];
        let type_index = self.signature_id(sig);

        let (func_ptr, vmctx) = if let Some(def_index) = self.module.local_func_index(index) {
            (self.functions[def_index].0 as *const _, self.vmctx_ptr())
        } else {
            let import = self.imported_function(index);
            (import.body, import.vmctx)
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

        // TODO(#983): investigate replacing this get/set loop with a `memcpy`.
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

/// A handle holding an `Instance` of a WebAssembly module.
#[derive(Hash, PartialEq, Eq)]
pub struct InstanceHandle {
    instance: *mut Instance,
}

/// # Safety
/// This is safe because there is no thread-specific logic in `InstanceHandle`.
/// TODO: this needs extra review
unsafe impl Send for InstanceHandle {}

impl InstanceHandle {
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
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn new(
        module: Arc<ModuleInfo>,
        finished_functions: BoxedSlice<LocalFunctionIndex, FunctionBodyPtr>,
        finished_memories: BoxedSlice<LocalMemoryIndex, Arc<dyn Memory>>,
        finished_tables: BoxedSlice<LocalTableIndex, Arc<dyn Table>>,
        finished_globals: BoxedSlice<LocalGlobalIndex, Arc<Global>>,
        imports: Imports,
        vmshared_signatures: BoxedSlice<SignatureIndex, VMSharedSignatureIndex>,
        host_state: Box<dyn Any>,
    ) -> Result<Self, Trap> {
        // TODO: investigate `vmctx_tables` and `vmctx_memories`: both of these
        // appear to be dropped in this function which may cause memory problems
        // depending on the ownership of the types in the `PrimaryMap`.
        let vmctx_tables = finished_tables
            .values()
            .map(|t| {
                let vmtable_ptr = t.vmtable();
                vmtable_ptr.as_ref().clone()
            })
            .collect::<PrimaryMap<LocalTableIndex, _>>()
            .into_boxed_slice();

        let vmctx_memories = finished_memories
            .values()
            .map(|m| {
                let vmmemory_ptr = m.as_ref().vmmemory();
                vmmemory_ptr.as_ref().clone()
            })
            .collect::<PrimaryMap<LocalMemoryIndex, _>>()
            .into_boxed_slice();

        let vmctx_globals = finished_globals
            .values()
            .map(|m| m.vmglobal())
            .collect::<PrimaryMap<LocalGlobalIndex, _>>()
            .into_boxed_slice();

        let offsets = VMOffsets::new(mem::size_of::<*const u8>() as u8, &module);

        let passive_data = RefCell::new(module.passive_data.clone());

        let handle = {
            let instance = Instance {
                module,
                offsets,
                memories: finished_memories,
                tables: finished_tables,
                globals: finished_globals,
                functions: finished_functions,
                passive_elements: Default::default(),
                passive_data,
                host_state,
                signal_handler: Cell::new(None),
                vmctx: VMContext {},
            };
            let layout = instance.alloc_layout();
            #[allow(clippy::cast_ptr_alignment)]
            let instance_ptr = alloc::alloc(layout) as *mut Instance;
            if instance_ptr.is_null() {
                alloc::handle_alloc_error(layout);
            }
            ptr::write(instance_ptr, instance);
            Self {
                instance: instance_ptr,
            }
        };
        let instance = handle.instance();

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
        ptr::copy(
            vmctx_tables.values().as_slice().as_ptr(),
            instance.tables_ptr() as *mut VMTableDefinition,
            vmctx_tables.len(),
        );
        ptr::copy(
            vmctx_memories.values().as_slice().as_ptr(),
            instance.memories_ptr() as *mut VMMemoryDefinition,
            vmctx_memories.len(),
        );
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

    /// Finishes the instantiation process started by `Instance::new`.
    ///
    /// # Safety
    ///
    /// Only safe to call immediately after instantiation.
    pub unsafe fn finish_instantiation(
        &self,
        data_initializers: &[DataInitializer<'_>],
    ) -> Result<(), Trap> {
        check_table_init_bounds(self.instance())?;
        check_memory_init_bounds(self.instance(), data_initializers)?;

        // Apply the initializers.
        initialize_tables(self.instance())?;
        initialize_memories(self.instance(), data_initializers)?;

        // The WebAssembly spec specifies that the start function is
        // invoked automatically at instantiation time.
        self.instance().invoke_start_function()?;
        Ok(())
    }

    /// Create a new `InstanceHandle` pointing at the instance
    /// pointed to by the given `VMContext` pointer.
    ///
    /// # Safety
    /// This is unsafe because it doesn't work on just any `VMContext`, it must
    /// be a `VMContext` allocated as part of an `Instance`.
    pub unsafe fn from_vmctx(vmctx: *const VMContext) -> Self {
        let instance = (&*vmctx).instance();

        Self {
            instance: instance as *const Instance as *mut Instance,
        }
    }

    /// Return a reference to the vmctx used by compiled wasm code.
    pub fn vmctx(&self) -> &VMContext {
        self.instance().vmctx()
    }

    /// Return a raw pointer to the vmctx used by compiled wasm code.
    pub fn vmctx_ptr(&self) -> *const VMContext {
        self.instance().vmctx_ptr()
    }

    /// Return a reference-counting pointer to a module.
    pub fn module(&self) -> &Arc<ModuleInfo> {
        self.instance().module()
    }

    /// Return a reference to a module.
    pub fn module_ref(&self) -> &ModuleInfo {
        self.instance().module_ref()
    }

    /// Lookup an export with the given name.
    pub fn lookup(&self, field: &str) -> Option<Export> {
        self.instance().lookup(field)
    }

    /// Lookup an export with the given export declaration.
    pub fn lookup_by_declaration(&self, export: &ExportIndex) -> Export {
        self.instance().lookup_by_declaration(export)
    }

    /// Return an iterator over the exports of this instance.
    ///
    /// Specifically, it provides access to the key-value pairs, where the keys
    /// are export names, and the values are export declarations which can be
    /// resolved `lookup_by_declaration`.
    pub fn exports(&self) -> indexmap::map::Iter<String, ExportIndex> {
        self.instance().exports()
    }

    /// Return a reference to the custom state attached to this instance.
    pub fn host_state(&self) -> &dyn Any {
        self.instance().host_state()
    }

    /// Return the memory index for the given `VMMemoryDefinition` in this instance.
    pub fn memory_index(&self, memory: &VMMemoryDefinition) -> LocalMemoryIndex {
        self.instance().memory_index(memory)
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
        self.instance().memory_grow(memory_index, delta)
    }

    /// Return the table index for the given `VMTableDefinition` in this instance.
    pub fn table_index(&self, table: &VMTableDefinition) -> LocalTableIndex {
        self.instance().table_index(table)
    }

    /// Grow table in this instance by the specified amount of pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of pages.
    pub fn table_grow(&self, table_index: LocalTableIndex, delta: u32) -> Option<u32> {
        self.instance().table_grow(table_index, delta)
    }

    /// Get table element reference.
    ///
    /// Returns `None` if index is out of bounds.
    pub fn table_get(
        &self,
        table_index: LocalTableIndex,
        index: u32,
    ) -> Option<VMCallerCheckedAnyfunc> {
        self.instance().table_get(table_index, index)
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
        self.instance().table_set(table_index, index, val)
    }

    /// Get a table defined locally within this module.
    pub fn get_local_table(&self, index: LocalTableIndex) -> &dyn Table {
        self.instance().get_local_table(index)
    }

    /// Return a reference to the contained `Instance`.
    pub(crate) fn instance(&self) -> &Instance {
        unsafe { &*(self.instance as *const Instance) }
    }

    /// Deallocates memory associated with this instance.
    ///
    /// # Safety
    ///
    /// This is unsafe because there might be other handles to this
    /// `InstanceHandle` elsewhere, and there's nothing preventing
    /// usage of this handle after this function is called.
    pub unsafe fn dealloc(&self) {
        let instance = self.instance();
        let layout = instance.alloc_layout();
        ptr::drop_in_place(self.instance);
        alloc::dealloc(self.instance.cast(), layout);
    }
}

impl Clone for InstanceHandle {
    fn clone(&self) -> Self {
        Self {
            instance: self.instance,
        }
    }
}

// TODO: uncomment this, as we need to store the handles
// in the store, and once the store is dropped, then the instances
// will too.
// impl Drop for InstanceHandle {
//     fn drop(&mut self) {
//         unsafe { self.dealloc() }
//     }
// }

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
                *instance.global(def_index).as_u32()
            } else {
                *instance.imported_global(base).definition.as_ref().as_u32()
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
        import.definition.as_ref().clone()
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
                *instance.global(def_index).as_u32()
            } else {
                *instance.imported_global(base).definition.as_ref().as_u32()
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
                GlobalInit::V128Const(x) => *(*to).as_u128_bits_mut() = *x.bytes(),
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
