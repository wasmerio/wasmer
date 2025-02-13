use rusty_jsc::{JSObject, JSValue};
use wasmer_types::TableType;

use crate::{
    jsc::vm::{VMExternTable, VMTable},
    vm::VMExtern,
    AsStoreMut, AsStoreRef, BackendTable, RuntimeError, Value,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub(crate) handle: VMTable,
}

// Table can't be Send in js because it dosen't support `structuredClone`
// https://developer.mozilla.org/en-US/docs/Web/API/structuredClone
// unsafe impl Send for Table {}

fn set_table_item(table: &VMTable, item_index: u32, item: &JSObject) -> Result<(), RuntimeError> {
    unimplemented!();
    // table.table.set(item_index, item).map_err(|e| e.into())
}

fn get_function(store: &mut impl AsStoreMut, val: Value) -> Result<JSObject, RuntimeError> {
    unimplemented!();
}

impl Table {
    pub fn new(
        store: &mut impl AsStoreMut,
        ty: TableType,
        init: Value,
    ) -> Result<Self, RuntimeError> {
        let store_mut = store.as_store_mut();
        let engine = store_mut.engine();
        let context = engine.as_jsc().context();

        let mut descriptor = JSObject::new(&context);
        descriptor.set_property(
            &context,
            "initial".to_string(),
            JSValue::number(&context, ty.minimum.into()),
        );
        if let Some(max) = ty.maximum {
            descriptor.set_property(
                &context,
                "maximum".to_string(),
                JSValue::number(&context, max.into()),
            );
        }
        descriptor.set_property(
            &context,
            "element".to_string(),
            JSValue::string(&context, "anyfunc".to_string()),
        );

        let js_table = engine
            .as_jsc()
            .wasm_table_type()
            .construct(&context, &[descriptor.to_jsvalue()])
            .map_err(|e| <JSValue as Into<RuntimeError>>::into(e))?;
        let vm_table = VMTable::new(js_table, ty);
        Ok(Self { handle: vm_table })
    }

    pub fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Jsc(crate::backend::jsc::vm::VMExtern::Table(
            self.handle.clone(),
        ))
    }

    pub fn ty(&self, _store: &impl AsStoreRef) -> TableType {
        self.handle.ty
    }

    pub fn get(&self, store: &mut impl AsStoreMut, index: u32) -> Option<Value> {
        unimplemented!();
    }

    pub fn set(
        &self,
        store: &mut impl AsStoreMut,
        index: u32,
        val: Value,
    ) -> Result<(), RuntimeError> {
        unimplemented!();
        // let item = get_function(store, val)?;
        // set_table_item(&self.handle, index, &item)
    }

    pub fn size(&self, store: &impl AsStoreRef) -> u32 {
        let store_mut = store.as_store_ref();
        let engine = store_mut.engine();
        let context = engine.as_jsc().context();
        self.handle
            .table
            .get_property(&context, "length".to_string())
            .to_number(&context)
            .unwrap() as _
    }

    pub fn grow(
        &self,
        store: &mut impl AsStoreMut,
        delta: u32,
        _init: Value,
    ) -> Result<u32, RuntimeError> {
        let store_mut = store.as_store_mut();
        let engine = store_mut.engine();
        let context = engine.as_jsc().context();
        let func = self
            .handle
            .table
            .get_property(&context, "grow".to_string())
            .to_object(&context)
            .unwrap();
        match func.call(
            &context,
            Some(&self.handle.table),
            &[JSValue::number(&context, delta as _)],
        ) {
            Ok(val) => Ok(val.to_number(&context).unwrap() as _),
            Err(e) => Err(<JSValue as Into<RuntimeError>>::into(e)),
        }
    }

    pub fn copy(
        _store: &mut impl AsStoreMut,
        _dst_table: &Self,
        _dst_index: u32,
        _src_table: &Self,
        _src_index: u32,
        _len: u32,
    ) -> Result<(), RuntimeError> {
        unimplemented!("Table.copy is not natively supported in Javascript");
    }

    pub(crate) fn from_vm_extern(
        _store: &mut impl AsStoreMut,
        vm_extern: crate::vm::VMExternTable,
    ) -> Self {
        Self {
            handle: vm_extern.into_jsc(),
        }
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }
}

impl crate::Table {
    /// Consume [`self`] into [`crate::backend::jsc::table::Table`].
    pub fn into_jsc(self) -> crate::backend::jsc::table::Table {
        match self.0 {
            BackendTable::Jsc(s) => s,
            _ => panic!("Not a `jsc` table!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::jsc::table::Table`].
    pub fn as_jsc(&self) -> &crate::backend::jsc::table::Table {
        match self.0 {
            BackendTable::Jsc(ref s) => s,
            _ => panic!("Not a `jsc` table!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::jsc::table::Table`].
    pub fn as_jsc_mut(&mut self) -> &mut crate::backend::jsc::table::Table {
        match self.0 {
            BackendTable::Jsc(ref mut s) => s,
            _ => panic!("Not a `jsc` table!"),
        }
    }
}

impl crate::BackendTable {
    /// Consume [`self`] into [`crate::backend::jsc::table::Table`].
    pub fn into_jsc(self) -> crate::backend::jsc::table::Table {
        match self {
            Self::Jsc(s) => s,
            _ => panic!("Not a `jsc` table!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::jsc::table::Table`].
    pub fn as_jsc(&self) -> &crate::backend::jsc::table::Table {
        match self {
            Self::Jsc(ref s) => s,
            _ => panic!("Not a `jsc` table!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::jsc::table::Table`].
    pub fn as_jsc_mut(&mut self) -> &mut crate::backend::jsc::table::Table {
        match self {
            Self::Jsc(ref mut s) => s,
            _ => panic!("Not a `jsc` table!"),
        }
    }
}
