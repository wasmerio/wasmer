use crate::js::exports::{ExportError, Exportable};
use crate::js::externals::Extern;
use crate::js::store::{AsStoreMut, AsStoreRef, StoreObjects};
use crate::js::vm::{VMExtern, VMMemory};
use crate::js::{MemoryAccessError, MemoryType};
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::slice;
#[cfg(feature = "tracing")]
use tracing::warn;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasmer_types::{Pages, WASM_PAGE_SIZE};

use super::MemoryView;

pub use wasmer_types::MemoryError;

#[wasm_bindgen]
extern "C" {
    /// [MDN documentation](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/WebAssembly/Memory)
    #[wasm_bindgen(js_namespace = WebAssembly, extends = js_sys::Object, typescript_type = "WebAssembly.Memory")]
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub type JSMemory;

    /// The `grow()` protoype method of the `Memory` object increases the
    /// size of the memory instance by a specified number of WebAssembly
    /// pages.
    ///
    /// Takes the number of pages to grow (64KiB in size) and returns the
    /// previous size of memory, in pages.
    ///
    /// # Reimplementation
    ///
    /// We re-implement `WebAssembly.Memory.grow` because it is
    /// different from what `wasm-bindgen` declares. It marks the function
    /// as `catch`, which means it can throw an exception.
    ///
    /// See [the opened patch](https://github.com/rustwasm/wasm-bindgen/pull/2599).
    ///
    /// # Exceptions
    ///
    /// A `RangeError` is thrown if adding pages would exceed the maximum
    /// memory.
    ///
    /// [MDN documentation](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/WebAssembly/Memory/grow)
    #[wasm_bindgen(catch, method, js_namespace = WebAssembly)]
    pub fn grow(this: &JSMemory, pages: u32) -> Result<u32, JsValue>;
}

/// A WebAssembly `memory` instance.
///
/// A memory instance is the runtime representation of a linear memory.
/// It consists of a vector of bytes and an optional maximum size.
///
/// The length of the vector always is a multiple of the WebAssembly
/// page size, which is defined to be the constant 65536 – abbreviated 64Ki.
/// Like in a memory type, the maximum size in a memory instance is
/// given in units of this page size.
///
/// A memory created by the host or in WebAssembly code will be accessible and
/// mutable from both host and WebAssembly.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#memory-instances>
#[derive(Debug, Clone)]
pub struct Memory {
    pub(crate) handle: VMMemory,
}

unsafe impl Send for Memory {}
unsafe impl Sync for Memory {}

impl Memory {
    /// Creates a new host `Memory` from the provided [`MemoryType`].
    ///
    /// This function will construct the `Memory` using the store
    /// [`BaseTunables`][crate::js::tunables::BaseTunables].
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Memory, MemoryType, Pages, Store, Type, Value};
    /// # let mut store = Store::default();
    /// #
    /// let m = Memory::new(&store, MemoryType::new(1, None, false)).unwrap();
    /// ```
    pub fn new(store: &mut impl AsStoreMut, ty: MemoryType) -> Result<Self, MemoryError> {
        let vm_memory = VMMemory::new(Self::new_internal(ty.clone())?, ty);
        Ok(Self::from_vm_extern(store, vm_memory))
    }

    pub(crate) fn new_internal(ty: MemoryType) -> Result<js_sys::WebAssembly::Memory, MemoryError> {
        let descriptor = js_sys::Object::new();
        // Annotation is here to prevent spurious IDE warnings.
        #[allow(unused_unsafe)]
        unsafe {
            js_sys::Reflect::set(&descriptor, &"initial".into(), &ty.minimum.0.into()).unwrap();
            if let Some(max) = ty.maximum {
                js_sys::Reflect::set(&descriptor, &"maximum".into(), &max.0.into()).unwrap();
            }
            js_sys::Reflect::set(&descriptor, &"shared".into(), &ty.shared.into()).unwrap();
        }

        let js_memory = js_sys::WebAssembly::Memory::new(&descriptor)
            .map_err(|_e| MemoryError::Generic("Error while creating the memory".to_owned()))?;

        Ok(js_memory)
    }

    /// Creates a new host `Memory` from provided JavaScript memory.
    pub fn new_raw(
        store: &mut impl AsStoreMut,
        js_memory: js_sys::WebAssembly::Memory,
        ty: MemoryType,
    ) -> Result<Self, MemoryError> {
        let vm_memory = VMMemory::new(js_memory, ty);
        Ok(Self::from_vm_extern(store, vm_memory))
    }

    /// Create a memory object from an existing memory and attaches it to the store
    pub fn new_from_existing(new_store: &mut impl AsStoreMut, memory: VMMemory) -> Self {
        Self::from_vm_extern(new_store, memory)
    }

    /// To `VMExtern`.
    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Memory(self.handle.clone())
    }

    /// Returns the [`MemoryType`] of the `Memory`.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Memory, MemoryType, Pages, Store, Type, Value};
    /// # let mut store = Store::default();
    /// #
    /// let mt = MemoryType::new(1, None, false);
    /// let m = Memory::new(&store, mt).unwrap();
    ///
    /// assert_eq!(m.ty(), mt);
    /// ```
    pub fn ty(&self, _store: &impl AsStoreRef) -> MemoryType {
        self.handle.ty
    }

    /// Creates a view into the memory that then allows for
    /// read and write
    pub fn view(&self, store: &impl AsStoreRef) -> MemoryView {
        MemoryView::new(self, store)
    }

    /// Grow memory by the specified amount of WebAssembly [`Pages`] and return
    /// the previous memory size.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Memory, MemoryType, Pages, Store, Type, Value, WASM_MAX_PAGES};
    /// # let mut store = Store::default();
    /// #
    /// let m = Memory::new(&store, MemoryType::new(1, Some(3), false)).unwrap();
    /// let p = m.grow(2).unwrap();
    ///
    /// assert_eq!(p, Pages(1));
    /// assert_eq!(m.size(), Pages(3));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if memory can't be grown by the specified amount
    /// of pages.
    ///
    /// ```should_panic
    /// # use wasmer::{Memory, MemoryType, Pages, Store, Type, Value, WASM_MAX_PAGES};
    /// # let mut store = Store::default();
    /// #
    /// let m = Memory::new(&store, MemoryType::new(1, Some(1), false)).unwrap();
    ///
    /// // This results in an error: `MemoryError::CouldNotGrow`.
    /// let s = m.grow(1).unwrap();
    /// ```
    pub fn grow<IntoPages>(
        &self,
        store: &mut impl AsStoreMut,
        delta: IntoPages,
    ) -> Result<Pages, MemoryError>
    where
        IntoPages: Into<Pages>,
    {
        let pages = delta.into();
        let js_memory = &self.handle.memory;
        let our_js_memory: &JSMemory = JsCast::unchecked_from_js_ref(js_memory);
        let new_pages = our_js_memory.grow(pages.0).map_err(|err| {
            if err.is_instance_of::<js_sys::RangeError>() {
                MemoryError::CouldNotGrow {
                    current: self.view(&store.as_store_ref()).size(),
                    attempted_delta: pages,
                }
            } else {
                MemoryError::Generic(err.as_string().unwrap())
            }
        })?;
        Ok(Pages(new_pages))
    }

    /// Copies the memory to a new store and returns a memory reference to it
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
            let pages = ((delta - 1) / WASM_PAGE_SIZE) + 1;
            new_memory.grow(new_store, Pages(pages as u32))?;
            new_view = new_memory.view(&new_store);
        }

        // Copy the bytes
        view.copy_to_memory(amount as u64, &new_view)
            .map_err(|err| MemoryError::Generic(err.to_string()))?;

        // Return the new memory
        Ok(new_memory)
    }

    pub(crate) fn from_vm_extern(_store: &mut impl AsStoreMut, internal: VMMemory) -> Self {
        Self { handle: internal }
    }

    /// Attempts to clone this memory (if its clonable)
    pub fn try_clone(&self, _store: &impl AsStoreRef) -> Option<VMMemory> {
        self.handle.try_clone()
    }

    /// Checks whether this `Global` can be used with the given context.
    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }

    /// Copies this memory to a new memory
    pub fn duplicate(&mut self, _store: &impl AsStoreRef) -> Result<VMMemory, MemoryError> {
        self.handle.duplicate()
    }
}

impl std::cmp::PartialEq for Memory {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
    }
}

impl<'a> Exportable<'a> for Memory {
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Memory(memory) => Ok(memory),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}

/// Underlying buffer for a memory.
#[derive(Copy, Clone)]
pub(crate) struct MemoryBuffer<'a> {
    pub(crate) base: *mut js_sys::Uint8Array,
    pub(crate) marker: PhantomData<(&'a Memory, &'a StoreObjects)>,
}

impl<'a> MemoryBuffer<'a> {
    pub(crate) fn read(&self, offset: u64, buf: &mut [u8]) -> Result<(), MemoryAccessError> {
        let end = offset
            .checked_add(buf.len() as u64)
            .ok_or(MemoryAccessError::Overflow)?;
        let view = unsafe { &*(self.base) };
        if end > view.length().into() {
            #[cfg(feature = "tracing")]
            warn!(
                "attempted to read ({} bytes) beyond the bounds of the memory view ({} > {})",
                buf.len(),
                end,
                view.length()
            );
            return Err(MemoryAccessError::HeapOutOfBounds);
        }
        view.subarray(offset as _, end as _)
            .copy_to(unsafe { &mut slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len()) });
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
        let view = unsafe { &*(self.base) };
        if end > view.length().into() {
            #[cfg(feature = "tracing")]
            warn!(
                "attempted to read ({} bytes) beyond the bounds of the memory view ({} > {})",
                buf.len(),
                end,
                view.length()
            );
            return Err(MemoryAccessError::HeapOutOfBounds);
        }
        let buf_ptr = buf.as_mut_ptr() as *mut u8;
        view.subarray(offset as _, end as _)
            .copy_to(unsafe { &mut slice::from_raw_parts_mut(buf_ptr, buf.len()) });

        Ok(unsafe { slice::from_raw_parts_mut(buf_ptr, buf.len()) })
    }

    pub(crate) fn write(&self, offset: u64, data: &[u8]) -> Result<(), MemoryAccessError> {
        let end = offset
            .checked_add(data.len() as u64)
            .ok_or(MemoryAccessError::Overflow)?;
        let view = unsafe { &mut *(self.base) };
        if end > view.length().into() {
            #[cfg(feature = "tracing")]
            warn!(
                "attempted to write ({} bytes) beyond the bounds of the memory view ({} > {})",
                data.len(),
                end,
                view.length()
            );
            return Err(MemoryAccessError::HeapOutOfBounds);
        }
        view.subarray(offset as _, end as _).copy_from(data);

        Ok(())
    }
}
