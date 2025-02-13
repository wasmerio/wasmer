//! Data types, functions and traits for `sys` runtime's `Memory` implementation.
use std::{
    convert::TryInto,
    marker::PhantomData,
    mem::{self, MaybeUninit},
    slice,
};

use tracing::warn;
use wasmer_types::{MemoryType, Pages};
use wasmer_vm::{LinearMemory, MemoryError, StoreHandle, ThreadConditionsHandle, VMMemory};

use crate::{
    backend::sys::entities::{engine::NativeEngineExt, memory::MemoryView},
    entities::store::{AsStoreMut, AsStoreRef},
    location::{MemoryLocation, SharedMemoryOps},
    vm::{VMExtern, VMExternMemory},
    BackendMemory, MemoryAccessError,
};

pub(crate) mod view;
pub use view::*;

use super::store::Store;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
/// A WebAssembly `memory` in the `sys` runtime.
pub struct Memory {
    pub(crate) handle: StoreHandle<VMMemory>,
}

impl Memory {
    pub(crate) fn new(store: &mut impl AsStoreMut, ty: MemoryType) -> Result<Self, MemoryError> {
        let mut store = store.as_store_mut();
        let tunables = store.engine().tunables();
        let style = tunables.memory_style(&ty);
        let memory = tunables.create_host_memory(&ty, &style)?;

        Ok(Self {
            handle: StoreHandle::new(store.as_store_mut().objects_mut().as_sys_mut(), memory),
        })
    }

    pub(crate) fn new_from_existing(new_store: &mut impl AsStoreMut, memory: VMMemory) -> Self {
        let handle = StoreHandle::new(new_store.objects_mut().as_sys_mut(), memory);
        Self::from_vm_extern(new_store, VMExternMemory::Sys(handle.internal_handle()))
    }

    pub(crate) fn ty(&self, store: &impl AsStoreRef) -> MemoryType {
        self.handle
            .get(store.as_store_ref().objects().as_sys())
            .ty()
    }

    pub(crate) fn grow<IntoPages>(
        &self,
        store: &mut impl AsStoreMut,
        delta: IntoPages,
    ) -> Result<Pages, MemoryError>
    where
        IntoPages: Into<Pages>,
    {
        self.handle
            .get_mut(store.objects_mut().as_sys_mut())
            .grow(delta.into())
    }

    pub(crate) fn grow_at_least(
        &self,
        store: &mut impl AsStoreMut,
        min_size: u64,
    ) -> Result<(), MemoryError> {
        self.handle
            .get_mut(store.objects_mut().as_sys_mut())
            .grow_at_least(min_size)
    }

    pub(crate) fn reset(&self, store: &mut impl AsStoreMut) -> Result<(), MemoryError> {
        self.handle
            .get_mut(store.as_store_mut().objects_mut().as_sys_mut())
            .reset()?;
        Ok(())
    }

    pub(crate) fn from_vm_extern(store: &impl AsStoreRef, vm_extern: VMExternMemory) -> Self {
        Self {
            handle: unsafe {
                StoreHandle::from_internal(
                    store.as_store_ref().objects().id(),
                    vm_extern.into_sys(),
                )
            },
        }
    }

    /// Checks whether this `Memory` can be used with the given context.
    pub(crate) fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.handle.store_id() == store.as_store_ref().objects().id()
    }

    /// Cloning memory will create another reference to the same memory that
    /// can be put into a new store
    pub(crate) fn try_clone(&self, store: &impl AsStoreRef) -> Result<VMMemory, MemoryError> {
        let mem = self.handle.get(store.as_store_ref().objects().as_sys());
        let cloned = mem.try_clone()?;
        Ok(cloned.into())
    }

    /// Copying the memory will actually copy all the bytes in the memory to
    /// a identical byte copy of the original that can be put into a new store
    pub(crate) fn try_copy(
        &self,
        store: &impl AsStoreRef,
    ) -> Result<Box<dyn LinearMemory + 'static>, MemoryError> {
        let mut mem = self.try_clone(store)?;
        mem.copy()
    }

    pub(crate) fn as_shared(
        &self,
        store: &impl AsStoreRef,
    ) -> Option<crate::memory::shared::SharedMemory> {
        let mem = self.handle.get(store.as_store_ref().objects().as_sys());
        let conds = mem.thread_conditions()?.downgrade();

        Some(crate::memory::shared::SharedMemory::new(
            crate::Memory(BackendMemory::Sys(self.clone())),
            conds,
        ))
    }

    /// To `VMExtern`.
    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Sys(wasmer_vm::VMExtern::Memory(self.handle.internal_handle()))
    }
}

impl SharedMemoryOps for ThreadConditionsHandle {
    fn notify(&self, dst: MemoryLocation, count: u32) -> Result<u32, crate::AtomicsError> {
        let count = self
            .upgrade()
            .ok_or(crate::AtomicsError::Unimplemented)?
            .do_notify(
                wasmer_vm::NotifyLocation {
                    address: dst.address,
                },
                count,
            );
        Ok(count)
    }

    fn wait(
        &self,
        dst: MemoryLocation,
        timeout: Option<std::time::Duration>,
    ) -> Result<u32, crate::AtomicsError> {
        self.upgrade()
            .ok_or(crate::AtomicsError::Unimplemented)?
            .do_wait(
                wasmer_vm::NotifyLocation {
                    address: dst.address,
                },
                timeout,
            )
            .map_err(|e| match e {
                wasmer_vm::WaiterError::Unimplemented => crate::AtomicsError::Unimplemented,
                wasmer_vm::WaiterError::TooManyWaiters => crate::AtomicsError::TooManyWaiters,
                wasmer_vm::WaiterError::AtomicsDisabled => crate::AtomicsError::AtomicsDisabled,
                _ => crate::AtomicsError::Unimplemented,
            })
    }

    fn disable_atomics(&self) -> Result<(), MemoryError> {
        self.upgrade()
            .ok_or_else(|| MemoryError::Generic("memory was dropped".to_string()))?
            .disable_atomics();
        Ok(())
    }

    fn wake_all_atomic_waiters(&self) -> Result<(), MemoryError> {
        self.upgrade()
            .ok_or_else(|| MemoryError::Generic("memory was dropped".to_string()))?
            .wake_all_atomic_waiters();
        Ok(())
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
        if end > self.len.try_into().unwrap() {
            warn!(
                "attempted to read ({} bytes) beyond the bounds of the memory view ({} > {})",
                buf.len(),
                end,
                self.len
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
        if end > self.len.try_into().unwrap() {
            warn!(
                "attempted to read ({} bytes) beyond the bounds of the memory view ({} > {})",
                buf.len(),
                end,
                self.len
            );
            return Err(MemoryAccessError::HeapOutOfBounds);
        }
        let buf_ptr = buf.as_mut_ptr() as *mut u8;
        unsafe {
            volatile_memcpy_read(self.base.add(offset as usize), buf_ptr, buf.len());
        }

        Ok(unsafe { slice::from_raw_parts_mut(buf_ptr, buf.len()) })
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
        #[repr(packed)]
        struct Unaligned<T>(T);
        let val = (*src as *const Unaligned<T>).read_volatile();
        (*dst as *mut Unaligned<T>).write(val);
        *src = src.add(mem::size_of::<T>());
        *dst = dst.add(mem::size_of::<T>());
        *len -= mem::size_of::<T>();
    }

    while len >= 8 {
        copy_one::<u64>(&mut src, &mut dst, &mut len);
    }
    if len >= 4 {
        copy_one::<u32>(&mut src, &mut dst, &mut len);
    }
    if len >= 2 {
        copy_one::<u16>(&mut src, &mut dst, &mut len);
    }
    if len >= 1 {
        copy_one::<u8>(&mut src, &mut dst, &mut len);
    }
}
#[inline]
unsafe fn volatile_memcpy_write(mut src: *const u8, mut dst: *mut u8, mut len: usize) {
    #[inline]
    unsafe fn copy_one<T>(src: &mut *const u8, dst: &mut *mut u8, len: &mut usize) {
        #[repr(packed)]
        struct Unaligned<T>(T);
        let val = (*src as *const Unaligned<T>).read();
        (*dst as *mut Unaligned<T>).write_volatile(val);
        *src = src.add(mem::size_of::<T>());
        *dst = dst.add(mem::size_of::<T>());
        *len -= mem::size_of::<T>();
    }

    while len >= 8 {
        copy_one::<u64>(&mut src, &mut dst, &mut len);
    }
    if len >= 4 {
        copy_one::<u32>(&mut src, &mut dst, &mut len);
    }
    if len >= 2 {
        copy_one::<u16>(&mut src, &mut dst, &mut len);
    }
    if len >= 1 {
        copy_one::<u8>(&mut src, &mut dst, &mut len);
    }
}

impl crate::Memory {
    /// Consume [`self`] into a [`crate::backend::sys::memory::Memory`].
    pub fn into_sys(self) -> crate::backend::sys::memory::Memory {
        match self.0 {
            BackendMemory::Sys(s) => s,
            _ => panic!("Not a `sys` memory!"),
        }
    }

    /// Convert a reference to [`self`] into a reference to [`crate::backend::sys::memory::Memory`].
    pub fn as_sys(&self) -> &crate::backend::sys::memory::Memory {
        match self.0 {
            BackendMemory::Sys(ref s) => s,
            _ => panic!("Not a `sys` memory!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference to [`crate::backend::sys::memory::Memory`].
    pub fn as_sys_mut(&mut self) -> &mut crate::backend::sys::memory::Memory {
        match self.0 {
            BackendMemory::Sys(ref mut s) => s,
            _ => panic!("Not a `sys` memory!"),
        }
    }
}
