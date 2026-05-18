//! Data types, functions and traits for `v8` runtime's `Memory` implementation.
use std::{marker::PhantomData, mem::MaybeUninit};

use tracing::warn;
pub use wasmer_types::MemoryError;
use wasmer_types::{MemoryType, Pages, WASM_PAGE_SIZE};

use crate::{
    AsStoreMut, AsStoreRef, BackendMemory, MemoryAccessError, OwnedMemory, OwnedOrSharedMemory,
    SharedMemory,
    entities::memory::shared_memory_detach_error,
    v8::{bindings::*, vm::VMMemory},
    vm::{VMExtern, VMExternMemory},
};

pub(crate) mod view;
pub use view::*;

use super::check_isolate;

#[derive(Debug, Clone)]
/// A WebAssembly `memory` in the `v8` runtime.
pub struct Memory {
    pub(crate) handle: VMMemory,
}

impl Memory {
    pub fn new(store: &mut impl AsStoreMut, ty: MemoryType) -> Result<Self, MemoryError> {
        check_isolate(store);

        let mut store_mut = store.as_store_mut();
        let v8_store = store_mut.inner.store.as_v8();

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

        let limits = Box::into_raw(Box::new(wasm_limits_t {
            min,
            max,
            shared: ty.shared,
        }));

        let memorytype = unsafe { wasm_memorytype_new(limits) };
        let c_memory = unsafe { wasm_memory_new(v8_store.inner, memorytype) };

        Ok(Self {
            handle: VMMemory(c_memory),
        })
    }

    pub fn new_from_existing(new_store: &mut impl AsStoreMut, memory: VMMemory) -> Self {
        check_isolate(new_store);
        let store_mut = new_store.as_store_mut();
        Self { handle: memory }
    }

    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::V8(unsafe { wasm_memory_as_extern(self.handle.0) })
    }

    pub fn ty(&self, store: &impl AsStoreRef) -> MemoryType {
        check_isolate(store);
        let store = store.as_store_ref();

        let memory_type: *mut wasm_memorytype_t = unsafe { wasm_memory_type(self.handle.0) };
        let limits: *const wasm_limits_t = unsafe { wasm_memorytype_limits(memory_type) };

        MemoryType {
            shared: unsafe { (*limits).shared },
            minimum: unsafe { wasmer_types::Pages((*limits).min) },
            maximum: unsafe { Some(wasmer_types::Pages((*limits).max)) },
        }
    }

    pub fn size(&self, store: &impl AsStoreRef) -> Pages {
        check_isolate(store);
        let size = unsafe { wasm_memory_size(self.handle.0) };
        Pages(size)
    }

    pub fn view<'a>(&self, store: &'a impl AsStoreRef) -> MemoryView<'a> {
        check_isolate(store);
        let store_ref = store.as_store_ref();
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
        check_isolate(store);
        let store_mut = store.as_store_mut();
        unsafe {
            let delta: Pages = delta.into();
            let current = Pages(wasm_memory_size(self.handle.0));

            if !wasm_memory_grow(self.handle.0, delta.0) {
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
        check_isolate(store);
        let store_mut = store.as_store_mut();

        unsafe {
            let current = wasm_memory_size(self.handle.0);
            let delta = (min_size as u32) - current;
            if delta > 0 {
                self.grow(store, delta)?;
            }
        }

        Ok(())
    }

    pub fn reset(&self, store: &mut impl AsStoreMut) -> Result<(), MemoryError> {
        check_isolate(store);
        Err(MemoryError::UnsupportedOperation {
            message: "reset not supported for V8 memory".to_owned(),
        })
    }

    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, internal: VMExternMemory) -> Self {
        check_isolate(store);
        Self {
            handle: VMMemory(internal.unwrap_v_8()),
        }
    }

    pub fn copy(&self, store: &impl AsStoreRef) -> Result<OwnedOrSharedMemory, MemoryError> {
        check_isolate(store);

        if self.ty(store).shared {
            return self
                .as_shared(store)
                .ok_or_else(shared_memory_detach_error)
                .map(OwnedOrSharedMemory::Shared);
        }

        let res = unsafe { wasm_memory_copy(self.handle.0) };
        if res.is_null() {
            Err(MemoryError::Generic("memory copy failed".to_owned()))
        } else {
            Ok(OwnedOrSharedMemory::Owned(OwnedMemory::from_vm_memory(
                crate::vm::VMMemory::V8(VMMemory(res)),
            )))
        }
    }

    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        check_isolate(store);
        true
    }

    pub fn as_shared(&self, store: &impl AsStoreRef) -> Option<SharedMemory> {
        if !self.ty(store).shared {
            return None;
        }

        Some(SharedMemory::from_vm_memory(crate::vm::VMMemory::V8(
            self.handle.clone(),
        )))
    }
}

impl std::cmp::PartialEq for Memory {
    fn eq(&self, other: &Self) -> bool {
        self.handle.0 == other.handle.0
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
        unsafe {
            copy_one::<u64>(&mut src, &mut dst, &mut len);
        }
    }
    if len >= 4 {
        unsafe {
            copy_one::<u32>(&mut src, &mut dst, &mut len);
        }
    }
    if len >= 2 {
        unsafe {
            copy_one::<u16>(&mut src, &mut dst, &mut len);
        }
    }
    if len >= 1 {
        unsafe {
            copy_one::<u8>(&mut src, &mut dst, &mut len);
        }
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
        unsafe {
            copy_one::<u64>(&mut src, &mut dst, &mut len);
        }
    }
    if len >= 4 {
        unsafe {
            copy_one::<u32>(&mut src, &mut dst, &mut len);
        }
    }
    if len >= 2 {
        unsafe {
            copy_one::<u16>(&mut src, &mut dst, &mut len);
        }
    }
    if len >= 1 {
        unsafe {
            copy_one::<u8>(&mut src, &mut dst, &mut len);
        }
    }
}

impl crate::Memory {
    /// Consume [`self`] into a [`crate::backend::v8::memory::Memory`].
    pub fn into_v8(self) -> crate::backend::v8::memory::Memory {
        match self.0 {
            BackendMemory::V8(s) => s,
            _ => panic!("Not a `v8` memory!"),
        }
    }

    /// Convert a reference to [`self`] into a reference to [`crate::backend::v8::memory::Memory`].
    pub fn as_v8(&self) -> &crate::backend::v8::memory::Memory {
        match self.0 {
            BackendMemory::V8(ref s) => s,
            _ => panic!("Not a `v8` memory!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference to [`crate::backend::v8::memory::Memory`].
    pub fn as_v8_mut(&mut self) -> &mut crate::backend::v8::memory::Memory {
        match self.0 {
            BackendMemory::V8(ref mut s) => s,
            _ => panic!("Not a `v8` memory!"),
        }
    }
}
