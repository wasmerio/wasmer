use crate::backend::stub::panic_stub;
use crate::entities::store::{AsStoreMut, AsStoreRef};
use crate::vm::VMExternGlobal;
use crate::{RuntimeError, Value};
use wasmer_types::{GlobalType, Mutability};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Global;

impl Global {
    pub fn from_value(
        _store: &mut impl AsStoreMut,
        _value: Value,
        _mutability: Mutability,
    ) -> Result<Self, RuntimeError> {
        Err(RuntimeError::new("stub backend cannot create globals"))
    }

    pub fn ty(&self, _store: &impl AsStoreRef) -> GlobalType {
        panic_stub("does not expose global types")
    }

    pub fn get(&self, _store: &mut impl AsStoreMut) -> Value {
        panic_stub("cannot read globals")
    }

    pub fn set(&self, _store: &mut impl AsStoreMut, _val: Value) -> Result<(), RuntimeError> {
        Err(RuntimeError::new("stub backend cannot mutate globals"))
    }

    pub fn from_vm_extern(_store: &mut impl AsStoreMut, _vm_extern: VMExternGlobal) -> Self {
        panic_stub("cannot import globals")
    }

    pub fn to_vm_extern(&self) -> VMExternGlobal {
        VMExternGlobal::Stub(crate::backend::stub::vm::VMExternGlobal::stub())
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        panic_stub("cannot verify global origins")
    }
}
