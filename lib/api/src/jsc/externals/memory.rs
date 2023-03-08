use crate::jsc::vm::{VMExtern, VMMemory};
use crate::mem_access::MemoryAccessError;
use crate::store::{AsStoreMut, AsStoreRef, StoreObjects};
use crate::MemoryType;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::slice;
#[cfg(feature = "tracing")]
use tracing::warn;

use wasmer_types::{Pages, WASM_PAGE_SIZE};

use super::memory_view::MemoryView;

pub use wasmer_types::MemoryError;

#[derive(Debug, Clone)]
pub struct Memory {
    pub(crate) handle: VMMemory,
}

// Only SharedMemories can be Send in js, becuase they support `structuredClone`.
// Normal memories will fail while doing structuredClone.
// In this case, we implement Send just in case as it can be a shared memory.
// https://developer.mozilla.org/en-US/docs/Web/API/structuredClone
// ```js
// const memory = new WebAssembly.Memory({
//   initial: 10,
//   maximum: 100,
//   shared: true // <--- It must be shared, otherwise structuredClone will fail
// });
// structuredClone(memory)
// ```
unsafe impl Send for Memory {}
unsafe impl Sync for Memory {}

impl Memory {
    pub fn new(store: &mut impl AsStoreMut, ty: MemoryType) -> Result<Self, MemoryError> {
        unimplemented!();
        // let vm_memory = VMMemory::new(Self::js_memory_from_type(&ty)?, ty);
        // Ok(Self::from_vm_extern(store, vm_memory))
    }

    pub fn new_from_existing(new_store: &mut impl AsStoreMut, memory: VMMemory) -> Self {
        unimplemented!();
        // Self::from_vm_extern(new_store, memory)
    }

    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        unimplemented!();
        // VMExtern::Memory(self.handle.clone())
    }

    pub fn ty(&self, _store: &impl AsStoreRef) -> MemoryType {
        self.handle.ty
    }

    pub fn view(&self, store: &impl AsStoreRef) -> MemoryView {
        unimplemented!();
        // MemoryView::new(self, store)
    }

    pub fn grow<IntoPages>(
        &self,
        store: &mut impl AsStoreMut,
        delta: IntoPages,
    ) -> Result<Pages, MemoryError>
    where
        IntoPages: Into<Pages>,
    {
        unimplemented!();
        // let pages = delta.into();
        // let js_memory = &self.handle.memory;
        // let our_js_memory: &JSMemory = JsCast::unchecked_from_js_ref(js_memory);
        // let new_pages = our_js_memory.grow(pages.0).map_err(|err| {
        //     if err.is_instance_of::<js_sys::RangeError>() {
        //         MemoryError::CouldNotGrow {
        //             current: self.view(&store.as_store_ref()).size(),
        //             attempted_delta: pages,
        //         }
        //     } else {
        //         MemoryError::Generic(err.as_string().unwrap())
        //     }
        // })?;
        // Ok(Pages(new_pages))
    }

    pub fn copy_to_store(
        &self,
        store: &impl AsStoreRef,
        new_store: &mut impl AsStoreMut,
    ) -> Result<Self, MemoryError> {
        unimplemented!();
        // // Create the new memory using the parameters of the existing memory
        // let view = self.view(store);
        // let ty = self.ty(store);
        // let amount = view.data_size() as usize;

        // let new_memory = Self::new(new_store, ty)?;
        // let mut new_view = new_memory.view(&new_store);
        // let new_view_size = new_view.data_size() as usize;
        // if amount > new_view_size {
        //     let delta = amount - new_view_size;
        //     let pages = ((delta - 1) / WASM_PAGE_SIZE) + 1;
        //     new_memory.grow(new_store, Pages(pages as u32))?;
        //     new_view = new_memory.view(&new_store);
        // }

        // // Copy the bytes
        // view.copy_to_memory(amount as u64, &new_view)
        //     .map_err(|err| MemoryError::Generic(err.to_string()))?;

        // // Return the new memory
        // Ok(new_memory)
    }

    pub(crate) fn from_vm_extern(_store: &mut impl AsStoreMut, internal: VMMemory) -> Self {
        Self { handle: internal }
    }

    pub fn try_clone(&self, _store: &impl AsStoreRef) -> Option<VMMemory> {
        self.handle.try_clone()
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }

    #[allow(unused)]
    pub fn duplicate(&mut self, _store: &impl AsStoreRef) -> Result<VMMemory, MemoryError> {
        self.handle.duplicate()
    }
}

impl std::cmp::PartialEq for Memory {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
    }
}

/// Underlying buffer for a memory.
#[derive(Copy, Clone, Debug)]
pub(crate) struct MemoryBuffer<'a> {
    // pub(crate) base: *mut js_sys::Uint8Array,
    pub(crate) marker: PhantomData<(&'a Memory, &'a StoreObjects)>,
}

impl<'a> MemoryBuffer<'a> {
    pub(crate) fn read(&self, offset: u64, buf: &mut [u8]) -> Result<(), MemoryAccessError> {
        unimplemented!();
        // let end = offset
        //     .checked_add(buf.len() as u64)
        //     .ok_or(MemoryAccessError::Overflow)?;
        // let view = unsafe { &*(self.base) };
        // if end > view.length().into() {
        //     #[cfg(feature = "tracing")]
        //     warn!(
        //         "attempted to read ({} bytes) beyond the bounds of the memory view ({} > {})",
        //         buf.len(),
        //         end,
        //         view.length()
        //     );
        //     return Err(MemoryAccessError::HeapOutOfBounds);
        // }
        // view.subarray(offset as _, end as _)
        //     .copy_to(unsafe { &mut slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len()) });
        // Ok(())
    }

    pub(crate) fn read_uninit<'b>(
        &self,
        offset: u64,
        buf: &'b mut [MaybeUninit<u8>],
    ) -> Result<&'b mut [u8], MemoryAccessError> {
        unimplemented!();
        // let end = offset
        //     .checked_add(buf.len() as u64)
        //     .ok_or(MemoryAccessError::Overflow)?;
        // let view = unsafe { &*(self.base) };
        // if end > view.length().into() {
        //     #[cfg(feature = "tracing")]
        //     warn!(
        //         "attempted to read ({} bytes) beyond the bounds of the memory view ({} > {})",
        //         buf.len(),
        //         end,
        //         view.length()
        //     );
        //     return Err(MemoryAccessError::HeapOutOfBounds);
        // }
        // let buf_ptr = buf.as_mut_ptr() as *mut u8;
        // view.subarray(offset as _, end as _)
        //     .copy_to(unsafe { &mut slice::from_raw_parts_mut(buf_ptr, buf.len()) });

        // Ok(unsafe { slice::from_raw_parts_mut(buf_ptr, buf.len()) })
    }

    pub(crate) fn write(&self, offset: u64, data: &[u8]) -> Result<(), MemoryAccessError> {
        unimplemented!();
        // let end = offset
        //     .checked_add(data.len() as u64)
        //     .ok_or(MemoryAccessError::Overflow)?;
        // let view = unsafe { &mut *(self.base) };
        // if end > view.length().into() {
        //     #[cfg(feature = "tracing")]
        //     warn!(
        //         "attempted to write ({} bytes) beyond the bounds of the memory view ({} > {})",
        //         data.len(),
        //         end,
        //         view.length()
        //     );
        //     return Err(MemoryAccessError::HeapOutOfBounds);
        // }
        // view.subarray(offset as _, end as _).copy_from(data);

        // Ok(())
    }
}
