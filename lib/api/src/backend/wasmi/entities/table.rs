//! Data types, functions and traits for `wasmi`'s `Table` implementation.
#![allow(missing_docs)]
use ::wasmi as wasmi_native;
use wasmer_types::TableType;

use crate::{
    AsStoreMut, AsStoreRef, BackendTable, RuntimeError, Value,
    vm::{VMExtern, VMExternTable},
    wasmi::{
        utils::convert::{IntoCApiType, IntoCApiValue, IntoWasmerType, IntoWasmerValue},
        vm::{VMTable, handle_bits},
    },
};

#[derive(Debug, Clone)]
/// A WebAssembly `table` in `wasmi`.
pub struct Table {
    pub(crate) handle: VMTable,
}

unsafe impl Send for Table {}
unsafe impl Sync for Table {}

impl PartialEq for Table {
    fn eq(&self, other: &Self) -> bool {
        handle_bits(self.handle) == handle_bits(other.handle)
    }
}

impl Eq for Table {}

fn wasmi_table_type(ty: TableType) -> wasmi_native::TableType {
    wasmi_native::TableType::new(ty.ty.into_ct(), ty.minimum, ty.maximum)
}

impl Table {
    pub fn new(
        store: &mut impl AsStoreMut,
        ty: TableType,
        init: Value,
    ) -> Result<Self, RuntimeError> {
        let mut store = store.as_store_mut();
        let handle = wasmi_native::Table::new(
            &mut store.inner.store.as_wasmi_mut().inner,
            wasmi_table_type(ty),
            init.into_cv(),
        )
        .map_err(|err| RuntimeError::new(err.to_string()))?;
        Ok(Self { handle })
    }

    pub fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Wasmi(crate::backend::wasmi::vm::VMExtern::Table(self.handle))
    }

    pub fn ty(&self, store: &impl AsStoreRef) -> TableType {
        let ty = self
            .handle
            .ty(&store.as_store_ref().inner.store.as_wasmi().inner);
        TableType {
            ty: ty.element().into_wt(),
            minimum: ty.minimum() as u32,
            maximum: ty.maximum().map(|v| v as u32),
            readonly: false,
        }
    }

    pub fn get(&self, store: &mut impl AsStoreMut, index: u32) -> Option<Value> {
        self.handle
            .get(
                &store.as_store_ref().inner.store.as_wasmi().inner,
                index as u64,
            )
            .map(IntoWasmerValue::into_wv)
    }

    pub fn set(
        &self,
        store: &mut impl AsStoreMut,
        index: u32,
        val: Value,
    ) -> Result<(), RuntimeError> {
        self.handle
            .set(
                &mut store.as_store_mut().inner.store.as_wasmi_mut().inner,
                index as u64,
                val.into_cv(),
            )
            .map_err(|err| RuntimeError::new(err.to_string()))
    }

    pub fn size(&self, store: &impl AsStoreRef) -> u32 {
        self.handle
            .size(&store.as_store_ref().inner.store.as_wasmi().inner) as u32
    }

    pub fn grow(
        &self,
        store: &mut impl AsStoreMut,
        delta: u32,
        init: Value,
    ) -> Result<u32, RuntimeError> {
        self.handle
            .grow(
                &mut store.as_store_mut().inner.store.as_wasmi_mut().inner,
                delta as u64,
                init.into_cv(),
            )
            .map(|v| v as u32)
            .map_err(|err| RuntimeError::new(err.to_string()))
    }

    pub fn copy(
        _store: &mut impl AsStoreMut,
        _dst_table: &Self,
        _dst_index: u32,
        _src_table: &Self,
        _src_index: u32,
        _len: u32,
    ) -> Result<(), RuntimeError> {
        unimplemented!("Copying tables is currently not implemented!")
    }

    pub(crate) fn from_vm_extern(_store: &mut impl AsStoreMut, vm_extern: VMExternTable) -> Self {
        let crate::vm::VMExternTable::Wasmi(handle) = vm_extern else {
            panic!("Not a `wasmi` table extern")
        };
        Self { handle }
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }
}

impl crate::Table {
    pub fn into_wasmi(self) -> crate::backend::wasmi::table::Table {
        match self.0 {
            BackendTable::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` table!"),
        }
    }

    pub fn as_wasmi(&self) -> &crate::backend::wasmi::table::Table {
        match &self.0 {
            BackendTable::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` table!"),
        }
    }

    pub fn as_wasmi_mut(&mut self) -> &mut crate::backend::wasmi::table::Table {
        match &mut self.0 {
            BackendTable::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` table!"),
        }
    }
}

impl crate::BackendTable {
    pub fn into_wasmi(self) -> crate::backend::wasmi::table::Table {
        match self {
            Self::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` table!"),
        }
    }

    pub fn as_wasmi(&self) -> &crate::backend::wasmi::table::Table {
        match self {
            Self::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` table!"),
        }
    }

    pub fn as_wasmi_mut(&mut self) -> &mut crate::backend::wasmi::table::Table {
        match self {
            Self::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` table!"),
        }
    }
}
