pub(crate) mod js;
pub(crate) use js::*;

pub(crate) mod view;
pub(crate) use view::*;

pub(crate) mod buffer;
pub(crate) use buffer::*;

use wasm_bindgen::JsCast;
use wasmer_types::{MemoryError, MemoryType, Pages};

use crate::{
    js::vm::memory::VMMemory,
    vm::{VMExtern, VMExternMemory},
    AsStoreMut, AsStoreRef, BackendMemory,
};

#[derive(Debug, Clone, Eq)]
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
        let vm_memory = VMMemory::new(Self::js_memory_from_type(&ty)?, ty);
        Ok(Self::from_vm_extern(store, VMExternMemory::Js(vm_memory)))
    }

    pub(crate) fn js_memory_from_type(
        ty: &MemoryType,
    ) -> Result<js_sys::WebAssembly::Memory, MemoryError> {
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

        let js_memory = js_sys::WebAssembly::Memory::new(&descriptor).map_err(|e| {
            let error_message = if let Some(s) = e.as_string() {
                s
            } else if let Some(obj) = e.dyn_ref::<js_sys::Object>() {
                obj.to_string().into()
            } else {
                "Error while creating the memory".to_string()
            };
            MemoryError::Generic(error_message)
        })?;

        Ok(js_memory)
    }

    pub fn new_from_existing(new_store: &mut impl AsStoreMut, memory: VMMemory) -> Self {
        Self::from_vm_extern(new_store, VMExternMemory::Js(memory))
    }

    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Js(crate::js::vm::VMExtern::Memory(self.handle.clone()))
    }

    pub fn ty(&self, _store: &impl AsStoreRef) -> MemoryType {
        self.handle.ty
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

    pub fn grow_at_least(
        &self,
        store: &mut impl AsStoreMut,
        min_size: u64,
    ) -> Result<(), MemoryError> {
        let cur_size = self.view(store).data_size();
        if min_size > cur_size {
            let delta = min_size - cur_size;
            let pages = ((delta - 1) / wasmer_types::WASM_PAGE_SIZE as u64) + 1;

            self.grow(store, Pages(pages as u32))?;
        }
        Ok(())
    }

    pub fn reset(&self, _store: &mut impl AsStoreMut) -> Result<(), MemoryError> {
        Ok(())
    }

    pub(crate) fn from_vm_extern(_store: &mut impl AsStoreMut, internal: VMExternMemory) -> Self {
        Self {
            handle: internal.into_js(),
        }
    }

    /// Cloning memory will create another reference to the same memory that
    /// can be put into a new store
    pub fn try_clone(&self, _store: &impl AsStoreRef) -> Result<VMMemory, MemoryError> {
        self.handle.try_clone()
    }

    /// Copying the memory will actually copy all the bytes in the memory to
    /// a identical byte copy of the original that can be put into a new store
    pub fn try_copy(&self, store: &impl AsStoreRef) -> Result<VMMemory, MemoryError> {
        let mut cloned = self.try_clone(store)?;
        cloned.copy()
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }

    pub fn as_shared(&self, _store: &impl AsStoreRef) -> Option<crate::shared::SharedMemory> {
        // Not supported.
        None
    }
}

impl std::cmp::PartialEq for Memory {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
    }
}

impl From<Memory> for crate::Memory {
    fn from(value: Memory) -> Self {
        crate::Memory(crate::BackendMemory::Js(value))
    }
}

impl From<crate::Memory> for Memory {
    fn from(value: crate::Memory) -> Self {
        value.into_js()
    }
}

impl crate::Memory {
    /// Consume [`self`] into a [`crate::backend::js::mem::Memory`].
    pub fn into_js(self) -> crate::backend::js::memory::Memory {
        match self.0 {
            BackendMemory::Js(s) => s,
            _ => panic!("Not a `js` memory!"),
        }
    }

    /// Convert a reference to [`self`] into a reference to [`crate::backend::js::mem::Memory`].
    pub fn as_js(&self) -> &crate::backend::js::memory::Memory {
        match self.0 {
            BackendMemory::Js(ref s) => s,
            _ => panic!("Not a `js` memory!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference to [`crate::backend::js::mem::Memory`].
    pub fn as_js_mut(&mut self) -> &mut crate::backend::js::memory::Memory {
        match self.0 {
            BackendMemory::Js(ref mut s) => s,
            _ => panic!("Not a `js` memory!"),
        }
    }
}
