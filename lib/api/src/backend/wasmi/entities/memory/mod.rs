//! Data types, functions and traits for `wasmi`'s `Memory` implementation.
#![allow(missing_docs)]
use std::{marker::PhantomData, mem::MaybeUninit};

use ::wasmi as wasmi_native;
use tracing::warn;
pub use wasmer_types::MemoryError;
use wasmer_types::{MemoryType, Pages};

use crate::{
    AsStoreMut, AsStoreRef, BackendMemory, MemoryAccessError,
    shared::SharedMemory,
    vm::{VMExtern, VMExternMemory},
    wasmi::vm::{VMMemory, handle_bits},
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
        let ty = wasmi_native::MemoryType::new(ty.minimum.0, ty.maximum.map(|v| v.0));
        let mut store = store.as_store_mut();
        let handle = wasmi_native::Memory::new(&mut store.inner.store.as_wasmi_mut().inner, ty)
            .map_err(|err| MemoryError::Generic(err.to_string()))?;
        Ok(Self { handle })
    }

    pub fn new_from_existing(_new_store: &mut impl AsStoreMut, memory: VMMemory) -> Self {
        Self { handle: memory }
    }

    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Wasmi(crate::backend::wasmi::vm::VMExtern::Memory(self.handle))
    }

    pub fn ty(&self, store: &impl AsStoreRef) -> MemoryType {
        let ty = self
            .handle
            .ty(&store.as_store_ref().inner.store.as_wasmi().inner);
        MemoryType {
            shared: false,
            minimum: Pages(ty.minimum() as u32),
            maximum: ty.maximum().map(|v| Pages(v as u32)),
        }
    }

    pub fn size(&self, store: &impl AsStoreRef) -> Pages {
        Pages(
            self.handle
                .size(&store.as_store_ref().inner.store.as_wasmi().inner) as u32,
        )
    }

    pub fn view<'a>(&self, store: &'a impl AsStoreRef) -> MemoryView<'a> {
        MemoryView::new(self, store)
    }

    pub fn grow<IntoPages>(
        &self,
        store: &mut impl AsStoreMut,
        delta: IntoPages,
    ) -> Result<Pages, MemoryError>
    where
        IntoPages: Into<Pages>,
    {
        let delta: Pages = delta.into();
        let mut store = store.as_store_mut();
        self.handle
            .grow(&mut store.inner.store.as_wasmi_mut().inner, delta.0 as u64)
            .map(|prev| Pages(prev as u32))
            .map_err(|err| MemoryError::Generic(err.to_string()))
    }

    pub fn grow_at_least(
        &self,
        store: &mut impl AsStoreMut,
        min_size: u64,
    ) -> Result<(), MemoryError> {
        let current = self.size(store).0 as u64;
        if min_size > current {
            self.grow(store, Pages((min_size - current) as u32))?;
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
        let view = self.view(store);
        let amount = view.data_size() as usize;
        let new_memory = Self::new(new_store, self.ty(store))?;
        if amount > new_memory.view(new_store).data_size() as usize {
            let delta = amount - new_memory.view(new_store).data_size() as usize;
            let pages = ((delta - 1) / wasmer_types::WASM_PAGE_SIZE) + 1;
            new_memory.grow(new_store, Pages(pages as u32))?;
        }
        view.copy_to_memory(amount as u64, &new_memory.view(new_store))
            .map_err(|err| MemoryError::Generic(err.to_string()))?;
        Ok(new_memory)
    }

    pub(crate) fn from_vm_extern(_store: &mut impl AsStoreMut, internal: VMExternMemory) -> Self {
        let crate::vm::VMExternMemory::Wasmi(handle) = internal else {
            panic!("Not a `wasmi` memory extern")
        };
        Self { handle }
    }

    pub fn try_clone(&self, _store: &impl AsStoreRef) -> Result<VMMemory, MemoryError> {
        Ok(self.handle)
    }

    pub fn try_copy(&self, _store: &impl AsStoreRef) -> Result<VMMemory, MemoryError> {
        Err(MemoryError::Generic(
            "copying native wasmi memories is not implemented".to_string(),
        ))
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }

    pub fn duplicate(&mut self, _store: &impl AsStoreRef) -> Result<VMMemory, MemoryError> {
        Ok(self.handle)
    }

    pub fn as_shared(&self, _store: &impl AsStoreRef) -> Option<SharedMemory> {
        None
    }
}

impl std::cmp::PartialEq for Memory {
    fn eq(&self, other: &Self) -> bool {
        handle_bits(self.handle) == handle_bits(other.handle)
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
    pub fn into_wasmi(self) -> crate::backend::wasmi::memory::Memory {
        match self.0 {
            BackendMemory::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` memory!"),
        }
    }

    pub fn as_wasmi(&self) -> &crate::backend::wasmi::memory::Memory {
        match &self.0 {
            BackendMemory::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` memory!"),
        }
    }

    pub fn as_wasmi_mut(&mut self) -> &mut crate::backend::wasmi::memory::Memory {
        match &mut self.0 {
            BackendMemory::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` memory!"),
        }
    }
}
