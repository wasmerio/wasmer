use crate::store::{AsStoreMut, AsStoreRef};
use crate::vm::VMExternGlobal;
use crate::{RuntimeError, Value};
use wasmer_types::{GlobalType, Mutability};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
        panic!("stub backend does not expose global types")
    }

    pub fn get(&self, _store: &mut impl AsStoreMut) -> Value {
        panic!("stub backend cannot read globals")
    }

    pub fn set(&self, _store: &mut impl AsStoreMut, _val: Value) -> Result<(), RuntimeError> {
        Err(RuntimeError::new("stub backend cannot mutate globals"))
    }

    pub fn from_vm_extern(
        _store: &mut impl AsStoreMut,
        _vm_extern: VMExternGlobal,
    ) -> Self {
        panic!("stub backend cannot import globals")
    }

    pub fn to_vm_extern(&self) -> VMExternGlobal {
        panic!("stub backend cannot expose VM globals")
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        panic!("stub backend cannot verify global origins")
    }
}
