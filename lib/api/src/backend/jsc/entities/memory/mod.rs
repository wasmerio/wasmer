pub(crate) mod view;
pub(crate) use view::*;

pub(crate) mod buffer;
pub(crate) use buffer::*;

use crate::{jsc::vm::VMMemory, vm::VMExtern, AsStoreMut, AsStoreRef, BackendMemory};
use rusty_jsc::{JSObject, JSValue};
use wasmer_types::{MemoryType, Pages};
use wasmer_vm::MemoryError;

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
        let handle = VMMemory::new(Self::js_memory_from_type(store, &ty)?, ty);
        Ok(Self { handle })
    }

    pub(crate) fn js_memory_from_type(
        store: &impl AsStoreRef,
        ty: &MemoryType,
    ) -> Result<JSObject, MemoryError> {
        let store_ref = store.as_store_ref();
        let engine = store_ref.engine();
        let context = engine.as_jsc().context();

        let mut descriptor = JSObject::new(&context);
        descriptor.set_property(
            &context,
            "initial".to_string(),
            JSValue::number(&context, ty.minimum.0.into()),
        );
        if let Some(max) = ty.maximum {
            descriptor.set_property(
                &context,
                "maximum".to_string(),
                JSValue::number(&context, max.0.into()),
            );
        }
        descriptor.set_property(
            &context,
            "shared".to_string(),
            JSValue::boolean(&context, ty.shared),
        );

        engine
            .as_jsc()
            .wasm_memory_type()
            .construct(&context, &[descriptor.to_jsvalue()])
            .map_err(|e| MemoryError::Generic(format!("{:?}", e)))
    }

    pub fn new_from_existing(new_store: &mut impl AsStoreMut, memory: VMMemory) -> Self {
        Self {
            handle: memory.clone(),
        }
    }

    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Jsc(crate::backend::jsc::vm::VMExtern::Memory(
            self.handle.clone(),
        ))
    }

    pub fn ty(&self, _store: &impl AsStoreRef) -> MemoryType {
        self.handle.ty
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
        let pages = delta.into();

        let store_mut = store.as_store_mut();
        let engine = store_mut.engine();
        let context = engine.as_jsc().context();
        let func = self
            .handle
            .memory
            .get_property(&context, "grow".to_string())
            .to_object(&context)
            .unwrap();
        match func.call(
            &context,
            Some(&self.handle.memory),
            &[JSValue::number(&context, pages.0 as _)],
        ) {
            Ok(val) => Ok(Pages(val.to_number(&context).unwrap() as _)),
            Err(e) => {
                let old_pages = pages;
                Err(MemoryError::CouldNotGrow {
                    current: old_pages,
                    attempted_delta: pages,
                })
            }
        }
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

    pub fn copy_to_store(
        &self,
        store: &impl AsStoreRef,
        new_store: &mut impl AsStoreMut,
    ) -> Result<Self, MemoryError> {
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
        // // Return the new memory
        Ok(new_memory)
    }

    pub(crate) fn from_vm_extern(
        _store: &mut impl AsStoreMut,
        internal: crate::vm::VMExternMemory,
    ) -> Self {
        Self {
            handle: internal.into_jsc(),
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
        cloned.copy(store)
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }

    #[allow(unused)]
    pub fn duplicate(&mut self, store: &impl AsStoreRef) -> Result<VMMemory, MemoryError> {
        self.handle.copy(store)
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

impl std::cmp::Eq for Memory {}

impl crate::Memory {
    /// Consume [`self`] into a [`crate::backend::jsc::mem::Memory`].
    pub fn into_jsc(self) -> crate::backend::jsc::memory::Memory {
        match self.0 {
            BackendMemory::Jsc(s) => s,
            _ => panic!("Not a `jsc` memory!"),
        }
    }

    /// Convert a reference to [`self`] into a reference to [`crate::backend::jsc::mem::Memory`].
    pub fn as_jsc(&self) -> &crate::backend::jsc::memory::Memory {
        match self.0 {
            BackendMemory::Jsc(ref s) => s,
            _ => panic!("Not a `jsc` memory!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference to [`crate::backend::jsc::mem::Memory`].
    pub fn as_jsc_mut(&mut self) -> &mut crate::backend::jsc::memory::Memory {
        match self.0 {
            BackendMemory::Jsc(ref mut s) => s,
            _ => panic!("Not a `jsc` memory!"),
        }
    }
}
