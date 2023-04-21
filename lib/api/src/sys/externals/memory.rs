use super::memory_view::MemoryView;
use crate::store::{AsStoreMut, AsStoreRef};
use crate::vm::VMExternMemory;
use crate::MemoryAccessError;
use crate::MemoryType;
use std::convert::TryInto;
use std::marker::PhantomData;
use std::mem;
use std::mem::MaybeUninit;
use std::slice;
#[cfg(feature = "tracing")]
use tracing::warn;
use wasmer_types::Pages;
use wasmer_vm::{LinearMemory, MemoryError, StoreHandle, VMExtern, VMMemory};

#[derive(Debug, Clone)]
pub struct Memory {
    pub(crate) handle: StoreHandle<VMMemory>,
}

impl Memory {
    pub fn new(store: &mut impl AsStoreMut, ty: MemoryType) -> Result<Self, MemoryError> {
        let mut store = store.as_store_mut();
        let tunables = store.engine().tunables();
        let style = tunables.memory_style(&ty);
        let memory = tunables.create_host_memory(&ty, &style)?;

        Ok(Self {
            handle: StoreHandle::new(store.objects_mut(), memory),
        })
    }

    pub fn new_from_existing(new_store: &mut impl AsStoreMut, memory: VMMemory) -> Self {
        let handle = StoreHandle::new(new_store.objects_mut(), memory);
        Self::from_vm_extern(new_store, handle.internal_handle())
    }

    pub fn ty(&self, store: &impl AsStoreRef) -> MemoryType {
        self.handle.get(store.as_store_ref().objects()).ty()
    }

    /// Creates a view into the memory that then allows for
    /// read and write
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
        self.handle.get_mut(store.objects_mut()).grow(delta.into())
    }

    /// Makes all the memory inaccessible to any reads or writes
    pub fn make_inaccessible(&self, store: &impl AsStoreRef) -> Result<(), MemoryError> {
        self.handle
            .get(store.as_store_ref().objects())
            .make_inaccessible()
    }

    pub fn copy_to_store(
        &self,
        store: &impl AsStoreRef,
        new_store: &mut impl AsStoreMut,
    ) -> Result<Self, MemoryError> {
        // Create the new memory using the parameters of the existing memory
        let view = self.view(store);
        let ty = self.ty(store);
        let amount = view.data_size() as usize;

        let new_memory = Self::new(new_store, ty)?;
        let mut new_view = new_memory.view(&new_store);
        let new_view_size = new_view.data_size() as usize;
        if amount > new_view_size {
            let delta = amount - new_view_size;
            let pages = ((delta - 1) / wasmer_types::WASM_PAGE_SIZE) + 1;
            new_memory.grow(new_store, Pages(pages as u32))?;
            new_view = new_memory.view(&new_store);
        }

        // Copy the bytes
        view.copy_to_memory(amount as u64, &new_view)
            .map_err(|err| MemoryError::Generic(err.to_string()))?;

        // Return the new memory
        Ok(new_memory)
    }

    pub(crate) fn from_vm_extern(store: &impl AsStoreRef, vm_extern: VMExternMemory) -> Self {
        Self {
            handle: unsafe {
                StoreHandle::from_internal(store.as_store_ref().objects().id(), vm_extern)
            },
        }
    }

    /// Checks whether this `Memory` can be used with the given context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.handle.store_id() == store.as_store_ref().objects().id()
    }

    pub fn try_clone(&self, store: &impl AsStoreRef) -> Option<VMMemory> {
        let mem = self.handle.get(store.as_store_ref().objects());
        mem.try_clone().map(|mem| mem.into())
    }

    /// To `VMExtern`.
    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Memory(self.handle.internal_handle())
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
            #[cfg(feature = "tracing")]
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
            #[cfg(feature = "tracing")]
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
            #[cfg(feature = "tracing")]
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
