//! This file declares `VMContext` and several related structs which contain
//! fields that compiled wasm code accesses directly.

use crate::instance::Instance;
use crate::memory::LinearMemory;
use crate::table::Table;
use crate::trap::{Trap, TrapCode};
use std::any::Any;
use std::convert::TryFrom;
use std::{ptr, u32};

/// An imported function.
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct VMFunctionImport {
    /// A pointer to the imported function body.
    pub body: *const VMFunctionBody,

    /// A pointer to the `VMContext` that owns the function.
    pub vmctx: *mut VMContext,
}

#[cfg(test)]
mod test_vmfunction_import {
    use super::VMFunctionImport;
    use crate::{Module, VMOffsets};
    use memoffset::offset_of;
    use std::mem::size_of;

    #[test]
    fn check_vmfunction_import_offsets() {
        let module = Module::new();
        let offsets = VMOffsets::new(size_of::<*mut u8>() as u8, &module);
        assert_eq!(
            size_of::<VMFunctionImport>(),
            usize::from(offsets.size_of_vmfunction_import())
        );
        assert_eq!(
            offset_of!(VMFunctionImport, body),
            usize::from(offsets.vmfunction_import_body())
        );
        assert_eq!(
            offset_of!(VMFunctionImport, vmctx),
            usize::from(offsets.vmfunction_import_vmctx())
        );
    }
}

/// A placeholder byte-sized type which is just used to provide some amount of type
/// safety when dealing with pointers to JIT-compiled function bodies. Note that it's
/// deliberately not Copy, as we shouldn't be carelessly copying function body bytes
/// around.
#[repr(C)]
pub struct VMFunctionBody(u8);

#[cfg(test)]
mod test_vmfunction_body {
    use super::VMFunctionBody;
    use std::mem::size_of;

    #[test]
    fn check_vmfunction_body_offsets() {
        assert_eq!(size_of::<VMFunctionBody>(), 1);
    }
}

/// The fields compiled code needs to access to utilize a WebAssembly table
/// imported from another instance.
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct VMTableImport {
    /// A pointer to the imported table description.
    pub definition: *mut VMTableDefinition,

    /// A pointer to the `Table` that owns the table description.
    pub from: *mut Table,
}

#[cfg(test)]
mod test_vmtable_import {
    use super::VMTableImport;
    use crate::{Module, VMOffsets};
    use memoffset::offset_of;
    use std::mem::size_of;

    #[test]
    fn check_vmtable_import_offsets() {
        let module = Module::new();
        let offsets = VMOffsets::new(size_of::<*mut u8>() as u8, &module);
        assert_eq!(
            size_of::<VMTableImport>(),
            usize::from(offsets.size_of_vmtable_import())
        );
        assert_eq!(
            offset_of!(VMTableImport, definition),
            usize::from(offsets.vmtable_import_definition())
        );
        assert_eq!(
            offset_of!(VMTableImport, from),
            usize::from(offsets.vmtable_import_from())
        );
    }
}

/// The fields compiled code needs to access to utilize a WebAssembly linear
/// memory imported from another instance.
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct VMMemoryImport {
    /// A pointer to the imported memory description.
    pub definition: *mut VMMemoryDefinition,

    /// A pointer to the `LinearMemory` that owns the memory description.
    pub from: *mut LinearMemory,
}

#[cfg(test)]
mod test_vmmemory_import {
    use super::VMMemoryImport;
    use crate::{Module, VMOffsets};
    use memoffset::offset_of;
    use std::mem::size_of;

    #[test]
    fn check_vmmemory_import_offsets() {
        let module = Module::new();
        let offsets = VMOffsets::new(size_of::<*mut u8>() as u8, &module);
        assert_eq!(
            size_of::<VMMemoryImport>(),
            usize::from(offsets.size_of_vmmemory_import())
        );
        assert_eq!(
            offset_of!(VMMemoryImport, definition),
            usize::from(offsets.vmmemory_import_definition())
        );
        assert_eq!(
            offset_of!(VMMemoryImport, from),
            usize::from(offsets.vmmemory_import_from())
        );
    }
}

/// The fields compiled code needs to access to utilize a WebAssembly global
/// variable imported from another instance.
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct VMGlobalImport {
    /// A pointer to the imported global variable description.
    pub definition: *mut VMGlobalDefinition,
}

#[cfg(test)]
mod test_vmglobal_import {
    use super::VMGlobalImport;
    use crate::{Module, VMOffsets};
    use memoffset::offset_of;
    use std::mem::size_of;

    #[test]
    fn check_vmglobal_import_offsets() {
        let module = Module::new();
        let offsets = VMOffsets::new(size_of::<*mut u8>() as u8, &module);
        assert_eq!(
            size_of::<VMGlobalImport>(),
            usize::from(offsets.size_of_vmglobal_import())
        );
        assert_eq!(
            offset_of!(VMGlobalImport, definition),
            usize::from(offsets.vmglobal_import_definition())
        );
    }
}

/// The fields compiled code needs to access to utilize a WebAssembly linear
/// memory defined within the instance, namely the start address and the
/// size in bytes.
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct VMMemoryDefinition {
    /// The start address.
    pub base: *mut u8,

    /// The current logical size of this linear memory in bytes.
    pub current_length: usize,
}

impl VMMemoryDefinition {
    /// Do a `memory.copy` for the memory.
    ///
    /// # Errors
    ///
    /// Returns a `Trap` error when the source or destination ranges are out of
    /// bounds.
    pub(crate) fn memory_copy(&self, dst: u32, src: u32, len: u32) -> Result<(), Trap> {
        // https://webassembly.github.io/reference-types/core/exec/instructions.html#exec-memory-copy
        if src
            .checked_add(len)
            .map_or(true, |n| n as usize > self.current_length)
            || dst
                .checked_add(len)
                .map_or(true, |m| m as usize > self.current_length)
        {
            return Err(Trap::wasm(TrapCode::HeapAccessOutOfBounds));
        }

        let dst = usize::try_from(dst).unwrap();
        let src = usize::try_from(src).unwrap();

        // Bounds and casts are checked above, by this point we know that
        // everything is safe.
        unsafe {
            let dst = self.base.add(dst);
            let src = self.base.add(src);
            ptr::copy(src, dst, len as usize);
        }

        Ok(())
    }

    /// Perform the `memory.fill` operation for the memory.
    ///
    /// # Errors
    ///
    /// Returns a `Trap` error if the memory range is out of bounds.
    pub(crate) fn memory_fill(&self, dst: u32, val: u32, len: u32) -> Result<(), Trap> {
        if dst
            .checked_add(len)
            .map_or(true, |m| m as usize > self.current_length)
        {
            return Err(Trap::wasm(TrapCode::HeapAccessOutOfBounds));
        }

        let dst = isize::try_from(dst).unwrap();
        let val = val as u8;

        // Bounds and casts are checked above, by this point we know that
        // everything is safe.
        unsafe {
            let dst = self.base.offset(dst);
            ptr::write_bytes(dst, val, len as usize);
        }

        Ok(())
    }
}

#[cfg(test)]
mod test_vmmemory_definition {
    use super::VMMemoryDefinition;
    use crate::{Module, VMOffsets};
    use memoffset::offset_of;
    use std::mem::size_of;

    #[test]
    fn check_vmmemory_definition_offsets() {
        let module = Module::new();
        let offsets = VMOffsets::new(size_of::<*mut u8>() as u8, &module);
        assert_eq!(
            size_of::<VMMemoryDefinition>(),
            usize::from(offsets.size_of_vmmemory_definition())
        );
        assert_eq!(
            offset_of!(VMMemoryDefinition, base),
            usize::from(offsets.vmmemory_definition_base())
        );
        assert_eq!(
            offset_of!(VMMemoryDefinition, current_length),
            usize::from(offsets.vmmemory_definition_current_length())
        );
        /* TODO: Assert that the size of `current_length` matches.
        assert_eq!(
            size_of::<VMMemoryDefinition::current_length>(),
            usize::from(offsets.size_of_vmmemory_definition_current_length())
        );
        */
    }
}

/// The fields compiled code needs to access to utilize a WebAssembly table
/// defined within the instance.
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct VMTableDefinition {
    /// Pointer to the table data.
    pub base: *mut u8,

    /// The current number of elements in the table.
    pub current_elements: u32,
}

#[cfg(test)]
mod test_vmtable_definition {
    use super::VMTableDefinition;
    use crate::{Module, VMOffsets};
    use memoffset::offset_of;
    use std::mem::size_of;

    #[test]
    fn check_vmtable_definition_offsets() {
        let module = Module::new();
        let offsets = VMOffsets::new(size_of::<*mut u8>() as u8, &module);
        assert_eq!(
            size_of::<VMTableDefinition>(),
            usize::from(offsets.size_of_vmtable_definition())
        );
        assert_eq!(
            offset_of!(VMTableDefinition, base),
            usize::from(offsets.vmtable_definition_base())
        );
        assert_eq!(
            offset_of!(VMTableDefinition, current_elements),
            usize::from(offsets.vmtable_definition_current_elements())
        );
    }
}

/// The storage for a WebAssembly global defined within the instance.
///
/// TODO: Pack the globals more densely, rather than using the same size
/// for every type.
#[derive(Debug, Copy, Clone)]
#[repr(C, align(16))]
pub struct VMGlobalDefinition {
    // TODO: use `UnsafeCell` here, make this not Copy; there's probably a ton of UB in this code right now
    storage: [u8; 16],
    // If more elements are added here, remember to add offset_of tests below!
}

#[cfg(test)]
mod test_vmglobal_definition {
    use super::VMGlobalDefinition;
    use crate::{Module, VMOffsets};
    use more_asserts::assert_ge;
    use std::mem::{align_of, size_of};

    #[test]
    fn check_vmglobal_definition_alignment() {
        assert_ge!(align_of::<VMGlobalDefinition>(), align_of::<i32>());
        assert_ge!(align_of::<VMGlobalDefinition>(), align_of::<i64>());
        assert_ge!(align_of::<VMGlobalDefinition>(), align_of::<f32>());
        assert_ge!(align_of::<VMGlobalDefinition>(), align_of::<f64>());
        assert_ge!(align_of::<VMGlobalDefinition>(), align_of::<[u8; 16]>());
    }

    #[test]
    fn check_vmglobal_definition_offsets() {
        let module = Module::new();
        let offsets = VMOffsets::new(size_of::<*mut u8>() as u8, &module);
        assert_eq!(
            size_of::<VMGlobalDefinition>(),
            usize::from(offsets.size_of_vmglobal_definition())
        );
    }

    #[test]
    fn check_vmglobal_begins_aligned() {
        let module = Module::new();
        let offsets = VMOffsets::new(size_of::<*mut u8>() as u8, &module);
        assert_eq!(offsets.vmctx_globals_begin() % 16, 0);
    }
}

impl VMGlobalDefinition {
    /// Construct a `VMGlobalDefinition`.
    pub fn new() -> Self {
        Self { storage: [0; 16] }
    }

    /// Return a reference to the value as an i32.
    #[allow(clippy::cast_ptr_alignment)]
    pub unsafe fn as_i32(&self) -> &i32 {
        &*(self.storage.as_ref().as_ptr() as *const i32)
    }

    /// Return a mutable reference to the value as an i32.
    #[allow(clippy::cast_ptr_alignment)]
    pub unsafe fn as_i32_mut(&mut self) -> &mut i32 {
        &mut *(self.storage.as_mut().as_mut_ptr() as *mut i32)
    }

    /// Return a reference to the value as a u32.
    #[allow(clippy::cast_ptr_alignment)]
    pub unsafe fn as_u32(&self) -> &u32 {
        &*(self.storage.as_ref().as_ptr() as *const u32)
    }

    /// Return a mutable reference to the value as an u32.
    #[allow(clippy::cast_ptr_alignment)]
    pub unsafe fn as_u32_mut(&mut self) -> &mut u32 {
        &mut *(self.storage.as_mut().as_mut_ptr() as *mut u32)
    }

    /// Return a reference to the value as an i64.
    #[allow(clippy::cast_ptr_alignment)]
    pub unsafe fn as_i64(&self) -> &i64 {
        &*(self.storage.as_ref().as_ptr() as *const i64)
    }

    /// Return a mutable reference to the value as an i64.
    #[allow(clippy::cast_ptr_alignment)]
    pub unsafe fn as_i64_mut(&mut self) -> &mut i64 {
        &mut *(self.storage.as_mut().as_mut_ptr() as *mut i64)
    }

    /// Return a reference to the value as an u64.
    #[allow(clippy::cast_ptr_alignment)]
    pub unsafe fn as_u64(&self) -> &u64 {
        &*(self.storage.as_ref().as_ptr() as *const u64)
    }

    /// Return a mutable reference to the value as an u64.
    #[allow(clippy::cast_ptr_alignment)]
    pub unsafe fn as_u64_mut(&mut self) -> &mut u64 {
        &mut *(self.storage.as_mut().as_mut_ptr() as *mut u64)
    }

    /// Return a reference to the value as an f32.
    #[allow(clippy::cast_ptr_alignment)]
    pub unsafe fn as_f32(&self) -> &f32 {
        &*(self.storage.as_ref().as_ptr() as *const f32)
    }

    /// Return a mutable reference to the value as an f32.
    #[allow(clippy::cast_ptr_alignment)]
    pub unsafe fn as_f32_mut(&mut self) -> &mut f32 {
        &mut *(self.storage.as_mut().as_mut_ptr() as *mut f32)
    }

    /// Return a reference to the value as f32 bits.
    #[allow(clippy::cast_ptr_alignment)]
    pub unsafe fn as_f32_bits(&self) -> &u32 {
        &*(self.storage.as_ref().as_ptr() as *const u32)
    }

    /// Return a mutable reference to the value as f32 bits.
    #[allow(clippy::cast_ptr_alignment)]
    pub unsafe fn as_f32_bits_mut(&mut self) -> &mut u32 {
        &mut *(self.storage.as_mut().as_mut_ptr() as *mut u32)
    }

    /// Return a reference to the value as an f64.
    #[allow(clippy::cast_ptr_alignment)]
    pub unsafe fn as_f64(&self) -> &f64 {
        &*(self.storage.as_ref().as_ptr() as *const f64)
    }

    /// Return a mutable reference to the value as an f64.
    #[allow(clippy::cast_ptr_alignment)]
    pub unsafe fn as_f64_mut(&mut self) -> &mut f64 {
        &mut *(self.storage.as_mut().as_mut_ptr() as *mut f64)
    }

    /// Return a reference to the value as f64 bits.
    #[allow(clippy::cast_ptr_alignment)]
    pub unsafe fn as_f64_bits(&self) -> &u64 {
        &*(self.storage.as_ref().as_ptr() as *const u64)
    }

    /// Return a mutable reference to the value as f64 bits.
    #[allow(clippy::cast_ptr_alignment)]
    pub unsafe fn as_f64_bits_mut(&mut self) -> &mut u64 {
        &mut *(self.storage.as_mut().as_mut_ptr() as *mut u64)
    }

    /// Return a reference to the value as an u128.
    #[allow(clippy::cast_ptr_alignment)]
    pub unsafe fn as_u128(&self) -> &u128 {
        &*(self.storage.as_ref().as_ptr() as *const u128)
    }

    /// Return a mutable reference to the value as an u128.
    #[allow(clippy::cast_ptr_alignment)]
    pub unsafe fn as_u128_mut(&mut self) -> &mut u128 {
        &mut *(self.storage.as_mut().as_mut_ptr() as *mut u128)
    }

    /// Return a reference to the value as u128 bits.
    #[allow(clippy::cast_ptr_alignment)]
    pub unsafe fn as_u128_bits(&self) -> &[u8; 16] {
        &*(self.storage.as_ref().as_ptr() as *const [u8; 16])
    }

    /// Return a mutable reference to the value as u128 bits.
    #[allow(clippy::cast_ptr_alignment)]
    pub unsafe fn as_u128_bits_mut(&mut self) -> &mut [u8; 16] {
        &mut *(self.storage.as_mut().as_mut_ptr() as *mut [u8; 16])
    }
}

/// An index into the shared signature registry, usable for checking signatures
/// at indirect calls.
#[repr(C)]
#[derive(Debug, Eq, PartialEq, Clone, Copy, Hash)]
pub struct VMSharedSignatureIndex(u32);

#[cfg(test)]
mod test_vmshared_signature_index {
    use super::VMSharedSignatureIndex;
    use crate::module::Module;
    use crate::vmoffsets::{TargetSharedSignatureIndex, VMOffsets};
    use std::mem::size_of;

    #[test]
    fn check_vmshared_signature_index() {
        let module = Module::new();
        let offsets = VMOffsets::new(size_of::<*mut u8>() as u8, &module);
        assert_eq!(
            size_of::<VMSharedSignatureIndex>(),
            usize::from(offsets.size_of_vmshared_signature_index())
        );
    }

    #[test]
    fn check_target_shared_signature_index() {
        assert_eq!(
            size_of::<VMSharedSignatureIndex>(),
            size_of::<TargetSharedSignatureIndex>()
        );
    }
}

impl VMSharedSignatureIndex {
    /// Create a new `VMSharedSignatureIndex`.
    pub fn new(value: u32) -> Self {
        Self(value)
    }
}

impl Default for VMSharedSignatureIndex {
    fn default() -> Self {
        Self::new(u32::MAX)
    }
}

/// The VM caller-checked "anyfunc" record, for caller-side signature checking.
/// It consists of the actual function pointer and a signature id to be checked
/// by the caller.
#[derive(Debug, Clone)]
#[repr(C)]
pub struct VMCallerCheckedAnyfunc {
    /// Function body.
    pub func_ptr: *const VMFunctionBody,
    /// Function signature id.
    pub type_index: VMSharedSignatureIndex,
    /// Function `VMContext`.
    pub vmctx: *mut VMContext,
    // If more elements are added here, remember to add offset_of tests below!
}

#[cfg(test)]
mod test_vmcaller_checked_anyfunc {
    use super::VMCallerCheckedAnyfunc;
    use crate::{Module, VMOffsets};
    use memoffset::offset_of;
    use std::mem::size_of;

    #[test]
    fn check_vmcaller_checked_anyfunc_offsets() {
        let module = Module::new();
        let offsets = VMOffsets::new(size_of::<*mut u8>() as u8, &module);
        assert_eq!(
            size_of::<VMCallerCheckedAnyfunc>(),
            usize::from(offsets.size_of_vmcaller_checked_anyfunc())
        );
        assert_eq!(
            offset_of!(VMCallerCheckedAnyfunc, func_ptr),
            usize::from(offsets.vmcaller_checked_anyfunc_func_ptr())
        );
        assert_eq!(
            offset_of!(VMCallerCheckedAnyfunc, type_index),
            usize::from(offsets.vmcaller_checked_anyfunc_type_index())
        );
        assert_eq!(
            offset_of!(VMCallerCheckedAnyfunc, vmctx),
            usize::from(offsets.vmcaller_checked_anyfunc_vmctx())
        );
    }
}

impl Default for VMCallerCheckedAnyfunc {
    fn default() -> Self {
        Self {
            func_ptr: ptr::null_mut(),
            type_index: Default::default(),
            vmctx: ptr::null_mut(),
        }
    }
}

/// An index type for builtin functions.
#[derive(Copy, Clone, Debug)]
pub struct VMBuiltinFunctionIndex(u32);

impl VMBuiltinFunctionIndex {
    /// Returns an index for wasm's `memory.grow` builtin function.
    pub const fn get_memory32_grow_index() -> Self {
        Self(0)
    }
    /// Returns an index for wasm's imported `memory.grow` builtin function.
    pub const fn get_imported_memory32_grow_index() -> Self {
        Self(1)
    }
    /// Returns an index for wasm's `memory.size` builtin function.
    pub const fn get_memory32_size_index() -> Self {
        Self(2)
    }
    /// Returns an index for wasm's imported `memory.size` builtin function.
    pub const fn get_imported_memory32_size_index() -> Self {
        Self(3)
    }
    /// Returns an index for wasm's `table.copy` when both tables are locally
    /// defined.
    pub const fn get_table_copy_index() -> Self {
        Self(4)
    }
    /// Returns an index for wasm's `table.init`.
    pub const fn get_table_init_index() -> Self {
        Self(5)
    }
    /// Returns an index for wasm's `elem.drop`.
    pub const fn get_elem_drop_index() -> Self {
        Self(6)
    }
    /// Returns an index for wasm's `memory.copy` for locally defined memories.
    pub const fn get_local_memory_copy_index() -> Self {
        Self(7)
    }
    /// Returns an index for wasm's `memory.copy` for imported memories.
    pub const fn get_imported_memory_copy_index() -> Self {
        Self(8)
    }
    /// Returns an index for wasm's `memory.fill` for locally defined memories.
    pub const fn get_memory_fill_index() -> Self {
        Self(9)
    }
    /// Returns an index for wasm's `memory.fill` for imported memories.
    pub const fn get_imported_memory_fill_index() -> Self {
        Self(10)
    }
    /// Returns an index for wasm's `memory.init` instruction.
    pub const fn get_memory_init_index() -> Self {
        Self(11)
    }
    /// Returns an index for wasm's `data.drop` instruction.
    pub const fn get_data_drop_index() -> Self {
        Self(12)
    }
    /// Returns an index for wasm's `raise_trap` instruction.
    pub const fn get_raise_trap_index() -> Self {
        Self(13)
    }
    /// Returns the total number of builtin functions.
    pub const fn builtin_functions_total_number() -> u32 {
        14
    }

    /// Return the index as an u32 number.
    pub const fn index(&self) -> u32 {
        self.0
    }
}

/// An array that stores addresses of builtin functions. We translate code
/// to use indirect calls. This way, we don't have to patch the code.
#[repr(C)]
pub struct VMBuiltinFunctionsArray {
    ptrs: [usize; Self::len()],
}

impl VMBuiltinFunctionsArray {
    pub const fn len() -> usize {
        VMBuiltinFunctionIndex::builtin_functions_total_number() as usize
    }

    pub fn initialized() -> Self {
        use crate::libcalls::*;

        let mut ptrs = [0; Self::len()];

        ptrs[VMBuiltinFunctionIndex::get_memory32_grow_index().index() as usize] =
            wasmer_memory32_grow as usize;
        ptrs[VMBuiltinFunctionIndex::get_imported_memory32_grow_index().index() as usize] =
            wasmer_imported_memory32_grow as usize;

        ptrs[VMBuiltinFunctionIndex::get_memory32_size_index().index() as usize] =
            wasmer_memory32_size as usize;
        ptrs[VMBuiltinFunctionIndex::get_imported_memory32_size_index().index() as usize] =
            wasmer_imported_memory32_size as usize;

        ptrs[VMBuiltinFunctionIndex::get_table_copy_index().index() as usize] =
            wasmer_table_copy as usize;

        ptrs[VMBuiltinFunctionIndex::get_table_init_index().index() as usize] =
            wasmer_table_init as usize;
        ptrs[VMBuiltinFunctionIndex::get_elem_drop_index().index() as usize] =
            wasmer_elem_drop as usize;

        ptrs[VMBuiltinFunctionIndex::get_local_memory_copy_index().index() as usize] =
            wasmer_local_memory_copy as usize;
        ptrs[VMBuiltinFunctionIndex::get_imported_memory_copy_index().index() as usize] =
            wasmer_imported_memory_copy as usize;
        ptrs[VMBuiltinFunctionIndex::get_memory_fill_index().index() as usize] =
            wasmer_memory_fill as usize;
        ptrs[VMBuiltinFunctionIndex::get_imported_memory_fill_index().index() as usize] =
            wasmer_imported_memory_fill as usize;
        ptrs[VMBuiltinFunctionIndex::get_memory_init_index().index() as usize] =
            wasmer_memory_init as usize;
        ptrs[VMBuiltinFunctionIndex::get_data_drop_index().index() as usize] =
            wasmer_data_drop as usize;
        ptrs[VMBuiltinFunctionIndex::get_raise_trap_index().index() as usize] =
            wasmer_raise_trap as usize;

        debug_assert!(ptrs.iter().cloned().all(|p| p != 0));

        Self { ptrs }
    }
}

/// The VM "context", which is pointed to by the `vmctx` arg in the compiler.
/// This has information about globals, memories, tables, and other runtime
/// state associated with the current instance.
///
/// The struct here is empty, as the sizes of these fields are dynamic, and
/// we can't describe them in Rust's type system. Sufficient memory is
/// allocated at runtime.
///
/// TODO: We could move the globals into the `vmctx` allocation too.
#[derive(Debug)]
#[repr(C, align(16))] // align 16 since globals are aligned to that and contained inside
pub struct VMContext {}

impl VMContext {
    /// Return a mutable reference to the associated `Instance`.
    ///
    /// # Safety
    /// This is unsafe because it doesn't work on just any `VMContext`, it must
    /// be a `VMContext` allocated as part of an `Instance`.
    #[allow(clippy::cast_ptr_alignment)]
    #[inline]
    pub(crate) unsafe fn instance(&self) -> &Instance {
        &*((self as *const Self as *mut u8).offset(-Instance::vmctx_offset()) as *const Instance)
    }

    /// Return a reference to the host state associated with this `Instance`.
    ///
    /// # Safety
    /// This is unsafe because it doesn't work on just any `VMContext`, it must
    /// be a `VMContext` allocated as part of an `Instance`.
    #[inline]
    pub unsafe fn host_state(&self) -> &dyn Any {
        self.instance().host_state()
    }
}

///
pub type VMTrampoline = unsafe extern "C" fn(
    *mut VMContext,        // callee vmctx
    *mut VMContext,        // caller vmctx
    *const VMFunctionBody, // function we're actually calling
    *mut u128,             // space for arguments and return values
);
