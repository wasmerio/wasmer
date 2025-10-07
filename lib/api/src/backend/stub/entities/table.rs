use crate::backend::stub::panic_stub;
use crate::entities::store::{AsStoreMut, AsStoreRef};
use crate::vm::VMExternTable;
use crate::{RuntimeError, Value};
use wasmer_types::TableType;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table;

impl Table {
    pub fn new(
        _store: &mut impl AsStoreMut,
        _ty: TableType,
        _init: Value,
    ) -> Result<Self, RuntimeError> {
        Err(RuntimeError::new("stub backend cannot create tables"))
    }

    pub fn ty(&self, _store: &impl AsStoreRef) -> TableType {
        panic_stub("does not expose table types")
    }

    pub fn get(&self, _store: &mut impl AsStoreMut, _index: u32) -> Option<Value> {
        panic_stub("cannot read tables")
    }

    pub fn set(
        &self,
        _store: &mut impl AsStoreMut,
        _index: u32,
        _value: Value,
    ) -> Result<(), RuntimeError> {
        Err(RuntimeError::new("stub backend cannot mutate tables"))
    }

    pub fn size(&self, _store: &impl AsStoreRef) -> u32 {
        panic_stub("does not expose table size")
    }

    pub fn grow(
        &self,
        _store: &mut impl AsStoreMut,
        _delta: u32,
        _init: Value,
    ) -> Result<u32, RuntimeError> {
        Err(RuntimeError::new("stub backend cannot grow tables"))
    }

    pub fn copy(
        _store: &mut impl AsStoreMut,
        _dst_table: &Self,
        _dst_index: u32,
        _src_table: &Self,
        _src_index: u32,
        _len: u32,
    ) -> Result<(), RuntimeError> {
        Err(RuntimeError::new("stub backend cannot copy tables"))
    }

    pub fn fill(
        &self,
        _store: &mut impl AsStoreMut,
        _index: u32,
        _value: Value,
        _len: u32,
    ) -> Result<(), RuntimeError> {
        Err(RuntimeError::new("stub backend cannot fill tables"))
    }

    pub fn from_vm_extern(_store: &mut impl AsStoreMut, _ext: VMExternTable) -> Self {
        panic_stub("cannot import tables")
    }

    pub fn to_vm_extern(&self) -> VMExternTable {
        VMExternTable::Stub(crate::backend::stub::vm::VMExternTable::stub())
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        panic_stub("cannot verify table origins")
    }
}
