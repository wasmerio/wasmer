//! Data types, functions and traits for `wasmi`'s `Memory` implementation.
//!
use std::{marker::PhantomData, mem::MaybeUninit};

use tracing::warn;
pub use wasmer_types::MemoryError;
use wasmer_types::{MemoryType, Pages, WASM_PAGE_SIZE};

use crate::{
    AsStoreMut, AsStoreRef, BackendMemory, MemoryAccessError,
    shared::SharedMemory,
    vm::{VMExtern, VMExternMemory},
    wasmi::{bindings::*, vm::VMMemory},
};

pub(crate) mod view;
pub use view::*;

#[derive(Debug, Clone)]
/// A WebAssembly `memory` in `wasmi`.
pub struct Memory {
    pub(crate) handle: VMMemory,
}

unsafe impl Send for Memory {}
unsafe impl Sync for Memory {}

impl Memory {
    pub fn new(store: &mut impl AsStoreMut, ty: MemoryType) -> Result<Self, MemoryError> {
        let max_requested = ty.maximum.unwrap_or(Pages::max_value());

        let min = ty.minimum.0;
        let max = max_requested.0;

        if max < min {
            return Err(MemoryError::InvalidMemory {
                reason: format!("the maximum ({max} pages) is less than the minimum ({min} pages)",),
            });
        }

        let max_allowed = Pages::max_value();
        if max_requested > max_allowed {
            return Err(MemoryError::MaximumMemoryTooLarge {
                max_requested,
                max_allowed,
            });
        }

        let limits = Box::into_raw(Box::new(wasm_limits_t { min, max }));

        let memorytype = unsafe { wasm_memorytype_new(limits) };

        let mut store = store.as_store_mut();
        let inner = store.inner.store.as_wasmi().inner;
        let c_memory = unsafe { wasm_memory_new(inner, memorytype) };

        Ok(Self { handle: c_memory })
    }

    pub fn new_from_existing(new_store: &mut impl AsStoreMut, memory: VMMemory) -> Self {
        Self { handle: memory }
    }

    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Wasmi(unsafe { wasm_memory_as_extern(self.handle) })
    }

    pub fn ty(&self, _store: &impl AsStoreRef) -> MemoryType {
        let memory_type: *mut wasm_memorytype_t = unsafe { wasm_memory_type(self.handle) };
        let limits: *const wasm_limits_t = unsafe { wasm_memorytype_limits(memory_type) };

        MemoryType {
            // [TODO]: Find a way to extract this from the inner memory type instead
            // of hardcoding.
            shared: false,
            minimum: unsafe { wasmer_types::Pages((*limits).min) },
            maximum: unsafe { Some(wasmer_types::Pages((*limits).max)) },
        }
    }

    pub fn size(&self, store: &impl AsStoreRef) -> Pages {
        let size = unsafe { wasm_memory_size(self.handle) };
        Pages(size)
    }

    pub fn view<'a>(&self, store: &'a impl AsStoreRef) -> MemoryView<'a> {
        MemoryView::new(self, store)
    }

    // Note: the return value is the memory size (in [`Pages`]) *before* growing it.
    pub fn grow<IntoPages>(
        &self,
        store: &mut impl AsStoreMut,
        delta: IntoPages,
    ) -> Result<Pages, MemoryError>
    where
        IntoPages: Into<Pages>,
    {
        unsafe {
            let delta: Pages = delta.into();
            let current = Pages(wasm_memory_size(self.handle));

            if !wasm_memory_grow(self.handle, delta.0) {
                Err(MemoryError::CouldNotGrow {
                    current,
                    attempted_delta: delta,
                })
            } else {
                Ok(current)
            }
        }
    }

    pub fn grow_at_least(
        &self,
        store: &mut impl AsStoreMut,
        min_size: u64,
    ) -> Result<(), MemoryError> {
        unsafe {
            let current = wasm_memory_size(self.handle);
            let delta = (min_size as u32) - current;
            if delta > 0 {
                self.grow(store, delta)?;
            }
        }

        Ok(())
    }

    pub fn reset(&self, _store: &mut impl AsStoreMut) -> Result<(), MemoryError> {
        Ok(())
    }

    pub fn copy_to_store(
        &self,
        store: &impl AsStoreRef,
        new_store: &mut impl AsStoreMut,
    ) -> Result<Self, MemoryError> {
        unimplemented!();
        // let view = self.view(store);
        // let ty = self.ty(store);
        // let amount = view.data_size() as usize;

        // let new_memory = Self::new(new_store, ty)?;
        // let mut new_view = new_memory.view(&new_store);
        // let new_view_size = new_view.data_size() as usize;
        // if amount > new_view_size {
        //     let delta = amount - new_view_size;
        //     let pages = ((delta - 1) / wasmer_types::WASM_PAGE_SIZE) + 1;
        //     new_memory.grow(new_store, Pages(pages as u32))?;
        //     new_view = new_memory.view(&new_store);
        // }

        // // Copy the bytes
        // view.copy_to_memory(amount as u64, &new_view)
        //     .map_err(|err| MemoryError::Generic(err.to_string()))?;
        // // // Return the new memory
        // Ok(new_memory)
    }

    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, internal: VMExternMemory) -> Self {
        Self {
            handle: internal.into_wasmi(),
        }
    }

    /// Cloning memory will create another reference to the same memory that
    /// can be put into a new store
    pub fn try_clone(&self, _store: &impl AsStoreRef) -> Result<VMMemory, MemoryError> {
        Ok(self.handle)
    }

    /// Copying the memory will actually copy all the bytes in the memory to
    /// a identical byte copy of the original that can be put into a new store
    pub fn try_copy(&self, store: &impl AsStoreRef) -> Result<VMMemory, MemoryError> {
        let res = unsafe { wasm_memory_copy(self.handle) };
        if res.is_null() {
            Err(MemoryError::Generic("memory copy failed".to_owned()))
        } else {
            Ok(res)
        }
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }

    #[allow(unused)]
    pub fn duplicate(&mut self, store: &impl AsStoreRef) -> Result<VMMemory, MemoryError> {
        unimplemented!();
        // self.handle.duplicate(store)
    }

    pub fn as_shared(&self, _store: &impl AsStoreRef) -> Option<SharedMemory> {
        // Not supported.
        None
    }
}

impl std::cmp::PartialEq for Memory {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
    }
}

impl std::cmp::Eq for Memory {}

/// Underlying buffer for a memory.
#[derive(Debug, Copy, Clone)]
pub(crate) struct MemoryBuffer<'a> {
    pub(crate) base: *mut u8,
    pub(crate) len: usize,
    pub(crate) marker: PhantomData<&'a MemoryView<'a>>,
}

impl<'a> MemoryBuffer<'a> {
    pub(crate) fn read(&self, offset: u64, buf: &mut [u8]) -> Result<(), MemoryAccessError> {
        let end = offset
            .checked_add(buf.len() as u64)
            .ok_or(MemoryAccessError::Overflow)?;

        let len: u64 = self.len.try_into().unwrap();
        if end > len {
            warn!(
                "attempted to read {} bytes, but the end offset is beyond the bounds of the memory view ({} > {}, diff. {} bytes)",
                buf.len(),
                end,
                len,
                end - len,
            );
            return Err(MemoryAccessError::HeapOutOfBounds);
        }
        unsafe {
            volatile_memcpy_read(self.base.add(offset as usize), buf.as_mut_ptr(), buf.len());
        }
        Ok(())
    }

    pub(crate) fn read_uninit<'b>(
        &self,
        offset: u64,
        buf: &'b mut [MaybeUninit<u8>],
    ) -> Result<&'b mut [u8], MemoryAccessError> {
        let end = offset
            .checked_add(buf.len() as u64)
            .ok_or(MemoryAccessError::Overflow)?;

        let len: u64 = self.len.try_into().unwrap();
        if end > len {
            warn!(
                "attempted to read {} bytes, but the end offset is beyond the bounds of the memory view ({} > {}, diff. {} bytes)",
                buf.len(),
                end,
                len,
                end - len,
            );
            return Err(MemoryAccessError::HeapOutOfBounds);
        }
        let buf_ptr = buf.as_mut_ptr() as *mut u8;
        unsafe {
            volatile_memcpy_read(self.base.add(offset as usize), buf_ptr, buf.len());
        }

        Ok(unsafe { std::slice::from_raw_parts_mut(buf_ptr, buf.len()) })
    }

    pub(crate) fn write(&self, offset: u64, data: &[u8]) -> Result<(), MemoryAccessError> {
        let end = offset
            .checked_add(data.len() as u64)
            .ok_or(MemoryAccessError::Overflow)?;
        if end > self.len.try_into().unwrap() {
            warn!(
                "attempted to write ({} bytes) beyond the bounds of the memory view ({} > {})",
                data.len(),
                end,
                self.len
            );
            return Err(MemoryAccessError::HeapOutOfBounds);
        }
        unsafe {
            volatile_memcpy_write(data.as_ptr(), self.base.add(offset as usize), data.len());
        }
        Ok(())
    }
}

// We can't use a normal memcpy here because it has undefined behavior if the
// memory is being concurrently modified. So we need to write our own memcpy
// implementation which uses volatile operations.
//
// The implementation of these functions can optimize very well when inlined
// with a fixed length: they should compile down to a single load/store
// instruction for small (8/16/32/64-bit) copies.
#[inline]
unsafe fn volatile_memcpy_read(mut src: *const u8, mut dst: *mut u8, mut len: usize) {
    #[inline]
    unsafe fn copy_one<T>(src: &mut *const u8, dst: &mut *mut u8, len: &mut usize) {
        #[repr(C, packed)]
        struct Unaligned<T>(T);
        unsafe {
            let val = (*src as *const Unaligned<T>).read_volatile();
            (*dst as *mut Unaligned<T>).write(val);
            *src = src.add(std::mem::size_of::<T>());
            *dst = dst.add(std::mem::size_of::<T>());
            *len -= std::mem::size_of::<T>();
        }
    }

    while len >= 8 {
        unsafe { copy_one::<u64>(&mut src, &mut dst, &mut len) };
    }
    if len >= 4 {
        unsafe { copy_one::<u32>(&mut src, &mut dst, &mut len) };
    }
    if len >= 2 {
        unsafe { copy_one::<u16>(&mut src, &mut dst, &mut len) };
    }
    if len >= 1 {
        unsafe { copy_one::<u8>(&mut src, &mut dst, &mut len) };
    }
}

#[inline]
unsafe fn volatile_memcpy_write(mut src: *const u8, mut dst: *mut u8, mut len: usize) {
    #[inline]
    unsafe fn copy_one<T>(src: &mut *const u8, dst: &mut *mut u8, len: &mut usize) {
        #[repr(C, packed)]
        struct Unaligned<T>(T);
        unsafe {
            let val = (*src as *const Unaligned<T>).read();
            (*dst as *mut Unaligned<T>).write_volatile(val);
            *src = src.add(std::mem::size_of::<T>());
            *dst = dst.add(std::mem::size_of::<T>());
            *len -= std::mem::size_of::<T>();
        }
    }

    while len >= 8 {
        unsafe { copy_one::<u64>(&mut src, &mut dst, &mut len) };
    }
    if len >= 4 {
        unsafe { copy_one::<u32>(&mut src, &mut dst, &mut len) };
    }
    if len >= 2 {
        unsafe { copy_one::<u16>(&mut src, &mut dst, &mut len) };
    }
    if len >= 1 {
        unsafe { copy_one::<u8>(&mut src, &mut dst, &mut len) };
    }
}

impl crate::Memory {
    /// Consume [`self`] into a [`crate::backend::wasmi::memory::Memory`].
    pub fn into_wasmi(self) -> crate::backend::wasmi::memory::Memory {
        match self.0 {
            BackendMemory::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` memory!"),
        }
    }

    /// Convert a reference to [`self`] into a reference to [`crate::backend::wasmi::memory::Memory`].
    pub fn as_wasmi(&self) -> &crate::backend::wasmi::memory::Memory {
        match &self.0 {
            BackendMemory::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` memory!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference to [`crate::backend::wasmi::memory::Memory`].
    pub fn as_wasmi_mut(&mut self) -> &mut crate::backend::wasmi::memory::Memory {
        match &mut self.0 {
            BackendMemory::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` memory!"),
        }
    }
}
