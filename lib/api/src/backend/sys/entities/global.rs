//! Data types, functions and traits for `sys` runtime's `Global` implementation.

use crate::{
    error::RuntimeError,
    store::{AsStoreMut, AsStoreRef},
    value::Value,
    vm::{VMExtern, VMExternGlobal},
};
use wasmer_types::{GlobalType, Mutability};
use wasmer_vm::{StoreHandle, VMGlobal};

#[derive(Debug, Clone)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
/// A WebAssembly `global` in the `sys` runtime.
pub struct Global {
    handle: StoreHandle<VMGlobal>,
}

impl Global {
    /// Create a `Global` with the initial value [`Value`] and the provided [`Mutability`].
    pub(crate) fn from_value(
        store: &mut impl AsStoreMut,
        val: Value,
        mutability: Mutability,
    ) -> Result<Self, RuntimeError> {
        if !val.is_from_store(store) {
            return Err(RuntimeError::new("cross-`Store` values are not supported"));
        }
        let global = VMGlobal::new(GlobalType {
            mutability,
            ty: val.ty(),
        });
        unsafe {
            global.vmglobal().as_mut().val = val.as_raw(store);
        }

        Ok(Self {
            handle: StoreHandle::new(store.objects_mut().as_sys_mut(), global),
        })
    }

    pub(crate) fn ty(&self, store: &impl AsStoreRef) -> GlobalType {
        *self
            .handle
            .get(store.as_store_ref().objects().as_sys())
            .ty()
    }

    pub(crate) fn get(&self, store: &mut impl AsStoreMut) -> Value {
        unsafe {
            let raw = self
                .handle
                .get(store.as_store_ref().objects().as_sys())
                .vmglobal()
                .as_ref()
                .val;
            let ty = self
                .handle
                .get(store.as_store_ref().objects().as_sys())
                .ty()
                .ty;
            Value::from_raw(store, ty, raw)
        }
    }

    pub(crate) fn set(&self, store: &mut impl AsStoreMut, val: Value) -> Result<(), RuntimeError> {
        if !val.is_from_store(store) {
            return Err(RuntimeError::new("cross-`Store` values are not supported"));
        }
        if self.ty(store).mutability != Mutability::Var {
            return Err(RuntimeError::new("Attempted to set an immutable global"));
        }
        if val.ty() != self.ty(store).ty {
            return Err(RuntimeError::new(format!(
                "Attempted to operate on a global of type {expected} as a global of type {found}",
                expected = self.ty(store).ty,
                found = val.ty(),
            )));
        }
        unsafe {
            self.handle
                .get_mut(store.as_store_mut().objects_mut().as_sys_mut())
                .vmglobal()
                .as_mut()
                .val = val.as_raw(store);
        }
        Ok(())
    }

    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, vm_extern: VMExternGlobal) -> Self {
        Self {
            handle: unsafe {
                StoreHandle::from_internal(
                    store.as_store_ref().objects().id(),
                    vm_extern.into_sys(),
                )
            },
        }
    }

    pub(crate) fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.handle.store_id() == store.as_store_ref().objects().id()
    }

    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Sys(wasmer_vm::VMExtern::Global(self.handle.internal_handle()))
    }
}

impl std::cmp::PartialEq for Global {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
    }
}

impl std::cmp::Eq for Global {}
