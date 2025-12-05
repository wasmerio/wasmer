use crate::{
    AsStoreMut, AsStoreRef, BackendTable, RuntimeError, Value,
    js::vm::{VMFunction, VMTable},
    vm::{VMExtern, VMExternTable},
};
use js_sys::Function;
use tracing::trace;
use wasm_bindgen::{JsCast, JsValue};
use wasmer_types::{FunctionType, TableType, Type};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub(crate) handle: VMTable,
}

// Table can't be Send in js because it dosen't support `structuredClone`
// https://developer.mozilla.org/en-US/docs/Web/API/structuredClone
// unsafe impl Send for Table {}

fn set_table_item(table: &VMTable, item_index: u32, item: &Function) -> Result<(), RuntimeError> {
    Ok(table.table.set(item_index, item)?)
}
fn set_table_null(table: &VMTable, item_index: u32) -> Result<(), RuntimeError> {
    // Get the method `table.set`
    let set = js_sys::Reflect::get(&table.table, &JsValue::from_str("set"))?
        .dyn_into::<Function>()
        .unwrap();

    // Call: table.set(index, null)
    set.call2(
        &table.table.as_ref(),      // this
        &JsValue::from(item_index), // first arg
        &JsValue::NULL,             // second arg
    )?;
    Ok(())
}

fn get_function(store: &mut impl AsStoreMut, val: Value) -> Result<Option<Function>, RuntimeError> {
    if !val.is_from_store(store) {
        return Err(RuntimeError::new("cannot pass Value across contexts"));
    }
    match val {
        Value::FuncRef(Some(ref func)) => {
            Ok(Some(func.as_js().handle.function.clone().into_inner()))
        }
        Value::FuncRef(None) => Ok(None),
        // Only funcrefs is supported by the spec atm
        _ => unimplemented!("The {val:?} is not yet supported"),
    }
}

impl Table {
    pub fn new(
        store: &mut impl AsStoreMut,
        ty: TableType,
        init: Value,
    ) -> Result<Self, RuntimeError> {
        let mut store = store;
        let descriptor = js_sys::Object::new();
        js_sys::Reflect::set(&descriptor, &"initial".into(), &ty.minimum.into())?;
        if let Some(max) = ty.maximum {
            js_sys::Reflect::set(&descriptor, &"maximum".into(), &max.into())?;
        }
        js_sys::Reflect::set(&descriptor, &"element".into(), &"anyfunc".into())?;

        let js_table = js_sys::WebAssembly::Table::new(&descriptor)?;
        // TODO: use `Table.new_with_value` method from wasm-bindgen
        // https://github.com/wasm-bindgen/wasm-bindgen/pull/4698
        let table = VMTable::new(js_table, ty);
        let num_elements = table.table.length();
        let func = get_function(&mut store, init)?;
        if let Some(func) = func {
            for i in 0..num_elements {
                set_table_item(&table, i, &func)?;
            }
        }

        Ok(Self { handle: table })
    }

    pub fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Js(crate::js::vm::VMExtern::Table(self.handle.clone()))
    }

    pub fn ty(&self, _store: &impl AsStoreRef) -> TableType {
        self.handle.ty
    }

    pub fn get(&self, store: &mut impl AsStoreMut, index: u32) -> Option<Value> {
        match self.handle.table.get(index) {
            Ok(value) => {
                if value.is_null() || value.is_undefined() {
                    return Some(Value::FuncRef(None));
                }

                let ty = FunctionType::new(vec![], vec![]);
                let vm_function = VMFunction::new(value, ty);
                let function = crate::Function::from_vm_extern(
                    store,
                    crate::vm::VMExternFunction::Js(vm_function),
                );
                Some(Value::FuncRef(Some(function)))
            }
            Err(_) => None,
        }
    }

    pub fn set(
        &self,
        store: &mut impl AsStoreMut,
        index: u32,
        val: Value,
    ) -> Result<(), RuntimeError> {
        let item = get_function(store, val)?;
        if let Some(func) = item {
            set_table_item(&self.handle, index, &func)?;
        } else {
            set_table_null(&self.handle, index)?;
        }
        Ok(())
    }

    pub fn size(&self, _store: &impl AsStoreRef) -> u32 {
        self.handle.table.length()
    }

    pub fn grow(
        &self,
        store: &mut impl AsStoreMut,
        delta: u32,
        init: Value,
    ) -> Result<u32, RuntimeError> {
        // TODO: use `Table.grow_with_value` method from wasm-bindgen
        // https://github.com/wasm-bindgen/wasm-bindgen/pull/4698
        let old_size = self.handle.table.grow(delta)?;
        if let Some(func) = get_function(store, init)? {
            for i in old_size..(old_size + delta) {
                set_table_item(&self.handle, i, &func)?;
            }
        } else {
            for i in old_size..(old_size + delta) {
                set_table_null(&self.handle, i)?;
            }
        }
        Ok(old_size)
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

    pub(crate) fn from_vm_extern(_store: &mut impl AsStoreMut, vm_extern: VMExternTable) -> Self {
        Self {
            handle: vm_extern.into_js(),
        }
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }
}

impl crate::Table {
    /// Consume [`self`] into [`crate::backend::js::table::Table`].
    pub fn into_js(self) -> crate::backend::js::table::Table {
        match self.0 {
            BackendTable::Js(s) => s,
            _ => panic!("Not a `js` table!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::js::table::Table`].
    pub fn as_js(&self) -> &crate::backend::js::table::Table {
        match &self.0 {
            BackendTable::Js(s) => s,
            _ => panic!("Not a `js` table!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::js::table::Table`].
    pub fn as_js_mut(&mut self) -> &mut crate::backend::js::table::Table {
        match &mut self.0 {
            BackendTable::Js(s) => s,
            _ => panic!("Not a `js` table!"),
        }
    }
}

impl crate::BackendTable {
    /// Consume [`self`] into [`crate::backend::js::table::Table`].
    pub fn into_js(self) -> crate::backend::js::table::Table {
        match self {
            Self::Js(s) => s,
            _ => panic!("Not a `js` table!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::js::table::Table`].
    pub fn as_js(&self) -> &crate::backend::js::table::Table {
        match self {
            Self::Js(s) => s,
            _ => panic!("Not a `js` table!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::js::table::Table`].
    pub fn as_js_mut(&mut self) -> &mut crate::backend::js::table::Table {
        match self {
            Self::Js(s) => s,
            _ => panic!("Not a `js` table!"),
        }
    }
}
