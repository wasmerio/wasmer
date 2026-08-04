use crate::{
    AsStoreMut, AsStoreRef, BackendTable, RuntimeError, Value,
    js::vm::{VMFunction, VMTable},
    vm::{VMExtern, VMExternTable},
};
use wasm_bindgen::{JsCast, JsValue};
use wasmer_types::{FunctionType, TableType, Type};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub(crate) handle: VMTable,
}

// Table can't be Send in js because it doesn't support `structuredClone`
// https://developer.mozilla.org/en-US/docs/Web/API/structuredClone
// unsafe impl Send for Table {}

fn set_table_item(table: &VMTable, item_index: u32, item: &JsValue) -> Result<(), RuntimeError> {
    table
        .table
        .set_raw(item_index, item)
        .map_err(|e| e.into())
}

fn get_table_item(store: &mut impl AsStoreMut, val: Value) -> Result<JsValue, RuntimeError> {
    if !val.is_from_store(store) {
        return Err(RuntimeError::new("cannot pass Value across contexts"));
    }
    match val {
        Value::FuncRef(Some(ref func)) => Ok(func.as_js().handle.function.clone().into_inner().into()),
        Value::FuncRef(None) | Value::ExternRef(None) => Ok(JsValue::null()),
        Value::ExternRef(Some(ref reference)) => match &reference.0 {
            crate::BackendExternRef::Js(reference) => Ok(reference.as_js_value()),
            #[allow(unreachable_patterns)]
            _ => Err(RuntimeError::new(
                "cannot pass an externref across backends",
            )),
        },
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
        let element = match ty.ty {
            Type::FuncRef => "anyfunc",
            Type::ExternRef => "externref",
            other => {
                return Err(RuntimeError::new(format!(
                    "{other:?} is not a valid JavaScript table element type"
                )));
            }
        };
        js_sys::Reflect::set(&descriptor, &"element".into(), &element.into())?;

        let initial_value = get_table_item(&mut store, init)?;
        let js_table = js_sys::WebAssembly::Table::new_with_value(&descriptor, initial_value)?;
        let table = VMTable::new(js_table, ty);

        Ok(Self { handle: table })
    }

    pub fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Js(crate::js::vm::VMExtern::Table(self.handle.clone()))
    }

    pub fn ty(&self, _store: &impl AsStoreRef) -> TableType {
        self.handle.ty
    }

    pub fn get(&self, store: &mut impl AsStoreMut, index: u32) -> Option<Value> {
        let value = self.handle.table.get_raw(index).ok()?;
        if value.is_null() {
            return Some(match self.handle.ty.ty {
                Type::FuncRef => Value::FuncRef(None),
                Type::ExternRef => Value::ExternRef(None),
                _ => return None,
            });
        }
        match self.handle.ty.ty {
            Type::FuncRef => {
                let func = value.dyn_into::<js_sys::Function>().ok()?;
                let ty = VMFunction::type_from_js(&func)
                    .unwrap_or_else(|| FunctionType::new(vec![], vec![]));
                let vm_function = VMFunction::new(func, ty);
                let function = crate::Function::from_vm_extern(
                    store,
                    crate::vm::VMExternFunction::Js(vm_function),
                );
                Some(Value::FuncRef(Some(function)))
            }
            Type::ExternRef => Some(Value::ExternRef(Some(crate::ExternRef(
                crate::BackendExternRef::Js(
                    crate::backend::js::entities::external::ExternRef::from_js_value(store, value),
                ),
            )))),
            _ => None,
        }
    }

    pub fn set(
        &self,
        store: &mut impl AsStoreMut,
        index: u32,
        val: Value,
    ) -> Result<(), RuntimeError> {
        let item = get_table_item(store, val)?;
        set_table_item(&self.handle, index, &item)
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
        let initial_value = get_table_item(store, init)?;
        self.handle
            .table
            .grow_with_value(delta, initial_value)
            .map_err(Into::into)
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
            handle: vm_extern.unwrap_js(),
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
