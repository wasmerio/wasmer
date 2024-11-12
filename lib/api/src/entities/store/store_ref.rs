use std::ops::{Deref, DerefMut};

use super::inner::StoreInner;
use wasmer_types::ExternType;
use wasmer_vm::{StoreObjects, TrapHandlerFn};

use crate::{
    entities::engine::{AsEngineRef, Engine, EngineRef},
    error::RuntimeError,
    view::MemoryViewCreator,
    vm::{VMExternRefCreator, VMExternRefResolver, VMFuncRefCreator, VMFuncRefResolver},
    ExternRefCreator, ExternRefResolver, GlobalCreator, MemoryCreator, StoreHandleCreator,
    StoreLike, TableCreator, VMFunctionEnvCreator,
};

/// A temporary handle to a [`Store`].
#[derive(Debug)]
pub struct StoreRef<'a> {
    pub(crate) inner: &'a StoreInner,
}

impl<'a> StoreRef<'a> {
    pub(crate) fn objects(&self) -> &'a StoreObjects {
        &self.inner.objects
    }

    /// Returns the underlying [`Engine`].
    pub fn engine(&self) -> &Engine {
        self.inner.store.engine()
    }

    /// Checks whether two stores are identical. A store is considered
    /// equal to another store if both have the same engine.
    pub fn same(a: &Self, b: &Self) -> bool {
        a.inner.objects.id() == b.inner.objects.id()
    }

    /// The signal handler
    #[inline]
    pub fn signal_handler(&self) -> Option<*const TrapHandlerFn<'static>> {
        self.inner.store.signal_handler()
    }
}

impl AsEngineRef for StoreRef<'_> {
    fn as_engine_ref(&self) -> EngineRef<'_> {
        self.inner.store.as_engine_ref()
    }
}

/// Helper trait for a value that is convertible to a [`StoreRef`].
pub trait AsStoreRef {
    /// Returns a `StoreRef` pointing to the underlying context.
    fn as_store_ref(&self) -> StoreRef<'_>;
}

impl<'a> ExternRefResolver for StoreRef<'a> {
    fn downcast_extern_ref<'b, T>(&self, extref: &dyn crate::ExternRefLike) -> Option<&'b T>
    where
        T: std::any::Any + Send + Sync + 'static + Sized,
        Self: Sized,
    {
        todo!()
    }
}

impl<'a> VMExternRefResolver for StoreRef<'a> {
    fn extern_ref_into_raw(&self, value: crate::vm::VMExternRef) -> wasmer_types::RawValue {
        self.inner.store.extern_ref_into_raw(value)
    }
}

impl<'a> VMFuncRefResolver for StoreRef<'a> {
    fn func_ref_into_raw(&self, value: crate::vm::VMFuncRef) -> wasmer_types::RawValue {
        self.inner.store.func_ref_into_raw(value)
    }
}

impl<'a> MemoryViewCreator for StoreRef<'a> {
    fn memory_view_from_memory(
        &self,
        memory: &crate::Memory,
    ) -> Box<dyn crate::view::MemoryViewLike> {
        self.inner.store.memory_view_from_memory(memory)
    }
}

impl AsStoreRef for StoreRef<'_> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        StoreRef { inner: self.inner }
    }
}

impl<P> AsStoreRef for P
where
    P: Deref,
    P::Target: AsStoreRef,
{
    fn as_store_ref(&self) -> StoreRef<'_> {
        (**self).as_store_ref()
    }
}

/// A mutable temporary handle to a [`Store`].
pub struct StoreMut<'a> {
    pub(crate) inner: &'a mut StoreInner,
}

impl AsStoreRef for StoreMut<'_> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        StoreRef { inner: self.inner }
    }
}

/// Helper trait for a value that is convertible to a [`StoreMut`].
pub trait AsStoreMut: AsStoreRef {
    /// Returns a `StoreMut` pointing to the underlying context.
    fn as_store_mut(&mut self) -> StoreMut<'_>;

    /// Returns the ObjectMutable
    fn objects_mut(&mut self) -> &mut StoreObjects;
}

impl AsStoreMut for StoreMut<'_> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        StoreMut { inner: self.inner }
    }
    fn objects_mut(&mut self) -> &mut StoreObjects {
        &mut self.inner.objects
    }
}

impl<P> AsStoreMut for P
where
    P: DerefMut,
    P::Target: AsStoreMut,
{
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        (**self).as_store_mut()
    }

    fn objects_mut(&mut self) -> &mut StoreObjects {
        (**self).objects_mut()
    }
}

impl AsEngineRef for StoreMut<'_> {
    fn as_engine_ref(&self) -> EngineRef<'_> {
        self.inner.store.as_engine_ref()
    }
}

impl<'a> ExternRefCreator for StoreMut<'a> {
    fn extern_ref_new<T>(&mut self, value: T) -> Box<dyn crate::ExternRefLike>
    where
        T: std::any::Any + Send + Sync + 'static + Sized,
        Self: Sized,
    {
        let embedder = self.inner.store.get_embedder();

        #[cfg(feature = "sys")]
        {
            if matches!(crate::embedders::Embedder::Sys, embedder) {
                match self.inner.store.as_sys_mut() {
                    Some(store) => {
                        return Box::new(
                            crate::embedders::sys::entitites::extern_ref::ExternRef::new(
                                store, value,
                            ),
                        )
                    }
                    None => todo!(),
                }
            }
        }

        panic!()
    }

    unsafe fn extern_ref_from_vm(
        &mut self,
        vm_externref: crate::vm::VMExternRef,
    ) -> Box<dyn crate::ExternRefLike> {
        todo!()
    }
}

impl<'a> VMExternRefCreator for StoreMut<'a> {
    unsafe fn extern_ref_from_raw(
        &self,
        raw: wasmer_types::RawValue,
    ) -> Option<crate::vm::VMExternRef> {
        self.inner.store.extern_ref_from_raw(raw)
    }
}

impl<'a> VMFuncRefCreator for StoreMut<'a> {
    unsafe fn func_ref_from_raw(
        &self,
        raw: wasmer_types::RawValue,
    ) -> Option<crate::vm::VMFuncRef> {
        self.inner.store.func_ref_from_raw(raw)
    }
}

impl<'a> GlobalCreator for StoreMut<'a> {
    fn global_from_value(
        &mut self,
        val: crate::Value,
        mutability: wasmer_types::Mutability,
    ) -> Result<Box<dyn crate::GlobalLike>, RuntimeError> {
        self.inner.store.global_from_value(val, mutability)
    }

    fn global_from_vm_extern(
        &mut self,
        vm_extern: crate::vm::VMExternGlobal,
    ) -> Box<dyn crate::GlobalLike> {
        self.inner.store.global_from_vm_extern(vm_extern)
    }
}

impl<'a> TableCreator for StoreMut<'a> {
    fn table_from_value(
        &mut self,
        ty: wasmer_types::TableType,
        init: crate::Value,
    ) -> Result<Box<dyn crate::TableLike>, RuntimeError> {
        self.inner.store.table_from_value(ty, init)
    }

    fn copy(
        &mut self,
        dst_table: &dyn crate::TableLike,
        dst_index: u32,
        src_table: &dyn crate::TableLike,
        src_index: u32,
        len: u32,
    ) -> Result<(), RuntimeError> {
        self.inner
            .store
            .copy(dst_table, dst_index, src_table, src_index, len)
    }

    fn table_from_vm_extern(&mut self, ext: crate::vm::VMExternTable) -> Box<dyn crate::TableLike> {
        self.inner.store.table_from_vm_extern(ext)
    }
}

impl<'a> MemoryCreator for StoreMut<'a> {
    fn memory_new(
        &mut self,
        ty: wasmer_types::MemoryType,
    ) -> Result<Box<dyn crate::MemoryLike>, wasmer_vm::MemoryError> {
        todo!()
    }

    fn memory_from_existing(&mut self, memory: crate::vm::VMMemory) -> Box<dyn crate::MemoryLike> {
        todo!()
    }

    fn memory_from_vm_extern(
        &mut self,
        vm_extern: crate::vm::VMExternMemory,
    ) -> Box<dyn crate::MemoryLike> {
        todo!()
    }
}

impl<'a> StoreHandleCreator for StoreMut<'a> {
    fn store_handle_from_value<T>(&mut self, val: T) -> crate::StoreHandle<T>
    where
        Self: Sized,
    {
        let embedder = self.inner.store.get_embedder();

        #[cfg(feature = "sys")]
        {
            if matches!(crate::embedders::Embedder::Sys, embedder) {
                todo!()
            }
        }

        panic!()
    }
}

impl<'a> VMFunctionEnvCreator for StoreMut<'a> {
    fn vm_env_from_value<T>(&mut self, value: T) -> crate::vm::VMFunctionEnvironment {
        todo!()
    }
}
