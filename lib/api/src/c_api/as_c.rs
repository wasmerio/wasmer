use crate::bindings::{
    wasm_val_t, wasm_val_t__bindgen_ty_1, wasm_valkind_enum_WASM_F32, wasm_valkind_enum_WASM_F64,
    wasm_valkind_enum_WASM_I32, wasm_valkind_enum_WASM_I64, wasm_valkind_t, wasm_valtype_kind,
    wasm_valtype_t,
};
//use crate::js::externals::Function;
// use crate::store::{Store, StoreObject};
// use crate::js::RuntimeError;
use crate::imports::Imports;
use crate::instance::Instance;
use crate::store::{AsStoreMut, AsStoreRef};
use crate::trap::Trap;
use crate::value::Value;
use crate::Type;
use crate::{Extern, Function, Global, Memory, Table};
use std::collections::HashMap;
use std::convert::TryInto;
use wasmer_types::ExternType;

/// Convert the given type to a [`JsValue`].
pub trait AsC: Sized {
    /// The inner definition type from this Javascript object
    type DefinitionType;
    type OutputType;
    /// Convert the given type to a [`Self::OutputType`].
    fn as_cvalue(&self, store: &impl AsStoreRef) -> Self::OutputType;
    /// Convert the given type to a [`Self::DefinitionType`].
    fn from_cvalue(
        store: &mut impl AsStoreMut,
        type_: &Self::DefinitionType,
        value: &Self::OutputType,
    ) -> Result<Self, Trap>;
}

#[inline]
pub fn param_from_c(value: &wasm_val_t) -> Value {
    match value.kind as u32 {
        wasm_valkind_enum_WASM_I32 => Value::I32(unsafe { value.of.i32_ }),
        wasm_valkind_enum_WASM_I64 => Value::I64(unsafe { value.of.i64_ }),
        wasm_valkind_enum_WASM_F32 => Value::F32(unsafe { value.of.f32_ }),
        wasm_valkind_enum_WASM_F64 => Value::F64(unsafe { value.of.f64_ }),
        _ => unimplemented!(),
    }
}

#[inline]
pub fn result_to_value(param: &Value) -> wasm_val_t {
    match param {
        Value::I32(val) => wasm_val_t {
            kind: wasm_valkind_enum_WASM_I32 as _,
            of: wasm_val_t__bindgen_ty_1 { i32_: *val },
        },
        Value::I64(val) => wasm_val_t {
            kind: wasm_valkind_enum_WASM_I64 as _,
            of: wasm_val_t__bindgen_ty_1 { i64_: *val },
        },
        Value::F32(val) => wasm_val_t {
            kind: wasm_valkind_enum_WASM_F32 as _,
            of: wasm_val_t__bindgen_ty_1 { f32_: *val },
        },
        Value::F64(val) => wasm_val_t {
            kind: wasm_valkind_enum_WASM_F64 as _,
            of: wasm_val_t__bindgen_ty_1 { f64_: *val },
        },
        _ => unimplemented!(),
    }
}

#[inline]
pub fn type_to_c(type_: &Type) -> wasm_valkind_t {
    match type_ {
        Type::I32 => wasm_valkind_enum_WASM_I32 as _,
        Type::I64 => wasm_valkind_enum_WASM_I64 as _,
        Type::F32 => wasm_valkind_enum_WASM_F32 as _,
        Type::F64 => wasm_valkind_enum_WASM_F64 as _,
        _ => unimplemented!(),
    }
}

#[inline]
pub fn valtype_to_type(type_: *const wasm_valtype_t) -> Type {
    let type_ = unsafe { wasm_valtype_kind(type_) };
    match type_ as u32 {
        wasm_valkind_enum_WASM_I32 => Type::I32,
        wasm_valkind_enum_WASM_I64 => Type::I64,
        wasm_valkind_enum_WASM_F32 => Type::F32,
        wasm_valkind_enum_WASM_F64 => Type::F64,
        _ => unimplemented!(),
    }
}

// impl AsC for Value {
//     type DefinitionType = Type;

//     fn as_cvalue(&self, _store: &impl AsStoreRef) -> JsValue {
//         match self {
//             Self::I32(i) => JsValue::from(*i),
//             Self::I64(i) => JsValue::from(*i),
//             Self::F32(f) => JsValue::from(*f),
//             Self::F64(f) => JsValue::from(*f),
//             Self::V128(v) => JsValue::from(*v),
//             Self::FuncRef(Some(func)) => func.0.handle.function.clone().into(),
//             Self::FuncRef(None) => JsValue::null(),
//             Self::ExternRef(_) => unimplemented!(),
//         }
//     }

//     fn from_cvalue(
//         _store: &mut impl AsStoreMut,
//         type_: &Self::DefinitionType,
//         value: &JsValue,
//     ) -> Result<Self, JsError> {
//         Ok(param_from_js(type_, value))
//     }
// }

// impl AsC for Imports {
//     type DefinitionType = crate::module::Module;

//     // Annotation is here to prevent spurious IDE warnings.
//     #[allow(unused_unsafe)]
//     fn as_cvalue(&self, store: &impl AsStoreRef) -> wasm_bindgen::JsValue {
//         let imports_object = js_sys::Object::new();
//         for (namespace, name, extern_) in self.iter() {
//             let val = unsafe { js_sys::Reflect::get(&imports_object, &namespace.into()).unwrap() };
//             if !val.is_undefined() {
//                 // If the namespace is already set

//                 // Annotation is here to prevent spurious IDE warnings.
//                 #[allow(unused_unsafe)]
//                 unsafe {
//                     js_sys::Reflect::set(
//                         &val,
//                         &name.into(),
//                         &extern_.as_cvalue(&store.as_store_ref()),
//                     )
//                     .unwrap();
//                 }
//             } else {
//                 // If the namespace doesn't exist
//                 let import_namespace = js_sys::Object::new();
//                 #[allow(unused_unsafe)]
//                 unsafe {
//                     js_sys::Reflect::set(
//                         &import_namespace,
//                         &name.into(),
//                         &extern_.as_cvalue(&store.as_store_ref()),
//                     )
//                     .unwrap();
//                     js_sys::Reflect::set(
//                         &imports_object,
//                         &namespace.into(),
//                         &import_namespace.into(),
//                     )
//                     .unwrap();
//                 }
//             }
//         }
//         imports_object.into()
//     }

//     fn from_cvalue(
//         store: &mut impl AsStoreMut,
//         module: &Self::DefinitionType,
//         value: &JsValue,
//     ) -> Result<Self, JsError> {
//         let module_imports: HashMap<(String, String), ExternType> = module
//             .imports()
//             .map(|import| {
//                 (
//                     (import.module().to_string(), import.name().to_string()),
//                     import.ty().clone(),
//                 )
//             })
//             .collect::<HashMap<(String, String), ExternType>>();

//         let mut map: HashMap<(String, String), Extern> = HashMap::new();
//         let object: js_sys::Object = value.clone().into();
//         for module_entry in js_sys::Object::entries(&object).iter() {
//             let module_entry: js_sys::Array = module_entry.into();
//             let module_name = module_entry.get(0).as_string().unwrap().to_string();
//             let module_import_object: js_sys::Object = module_entry.get(1).into();
//             for import_entry in js_sys::Object::entries(&module_import_object).iter() {
//                 let import_entry: js_sys::Array = import_entry.into();
//                 let import_name = import_entry.get(0).as_string().unwrap().to_string();
//                 let import_js: wasm_bindgen::JsValue = import_entry.get(1);
//                 let key = (module_name.clone(), import_name);
//                 let extern_type = module_imports.get(&key).unwrap();
//                 let extern_ = Extern::from_cvalue(store, extern_type, &import_js)?;
//                 map.insert(key, extern_);
//             }
//         }

//         Ok(Self { map })
//     }
// }

// impl AsC for Extern {
//     type DefinitionType = ExternType;

//     fn as_cvalue(&self, _store: &impl AsStoreRef) -> wasm_bindgen::JsValue {
//         match self {
//             Self::Memory(memory) => memory.0.handle.memory.clone().into(),
//             Self::Function(function) => function.0.handle.function.clone().into(),
//             Self::Table(table) => table.0.handle.table.clone().into(),
//             Self::Global(global) => global.0.handle.global.clone().into(),
//         }
//     }

//     fn from_cvalue(
//         store: &mut impl AsStoreMut,
//         extern_type: &Self::DefinitionType,
//         val: &JsValue,
//     ) -> Result<Self, JsError> {
//         // Note: this function do a soft check over the type
//         // We only check the "kind" of Extern, but nothing else
//         match extern_type {
//             ExternType::Memory(memory_type) => {
//                 Ok(Self::Memory(Memory::from_cvalue(store, memory_type, val)?))
//             }
//             ExternType::Global(global_type) => {
//                 Ok(Self::Global(Global::from_cvalue(store, global_type, val)?))
//             }
//             ExternType::Function(function_type) => Ok(Self::Function(Function::from_cvalue(
//                 store,
//                 function_type,
//                 val,
//             )?)),
//             ExternType::Table(table_type) => {
//                 Ok(Self::Table(Table::from_cvalue(store, table_type, val)?))
//             }
//         }
//     }
// }

// impl AsC for Instance {
//     type DefinitionType = crate::module::Module;
//     fn as_cvalue(&self, _store: &impl AsStoreRef) -> wasm_bindgen::JsValue {
//         self._inner._handle.clone().into()
//     }

//     fn from_cvalue(
//         store: &mut impl AsStoreMut,
//         module: &Self::DefinitionType,
//         value: &JsValue,
//     ) -> Result<Self, JsError> {
//         let js_instance: js_sys::WebAssembly::Instance = value.clone().into();
//         let (instance, exports) = JsInstance::from_module_and_instance(store, module, js_instance)
//             .map_err(|e| JsError::new(&format!("Can't get the instance: {:?}", e)))?;
//         Ok(Instance {
//             _inner: instance,
//             module: module.clone(),
//             exports,
//         })
//     }
// }

// impl AsC for Memory {
//     type DefinitionType = crate::MemoryType;

//     fn as_cvalue(&self, _store: &impl AsStoreRef) -> wasm_bindgen::JsValue {
//         self.0.handle.memory.clone().into()
//     }

//     fn from_cvalue(
//         store: &mut impl AsStoreMut,
//         memory_type: &Self::DefinitionType,
//         value: &JsValue,
//     ) -> Result<Self, JsError> {
//         if let Some(memory) = value.dyn_ref::<JsMemory>() {
//             Ok(Memory::from_vm_extern(
//                 store,
//                 VMMemory::new(memory.clone(), memory_type.clone()),
//             ))
//         } else {
//             Err(JsError::new(&format!(
//                 "Extern expect to be of type Memory, but received {:?}",
//                 value
//             )))
//         }
//     }
// }

// impl AsC for Function {
//     type DefinitionType = crate::FunctionType;

//     fn as_cvalue(&self, _store: &impl AsStoreRef) -> wasm_bindgen::JsValue {
//         self.0.handle.function.clone().into()
//     }

//     fn from_cvalue(
//         store: &mut impl AsStoreMut,
//         function_type: &Self::DefinitionType,
//         value: &JsValue,
//     ) -> Result<Self, JsError> {
//         if value.is_instance_of::<JsFunction>() {
//             Ok(Function::from_vm_extern(
//                 store,
//                 VMFunction::new(
//                     value.clone().unchecked_into::<JsFunction>(),
//                     function_type.clone(),
//                 ),
//             ))
//         } else {
//             Err(JsError::new(&format!(
//                 "Extern expect to be of type Function, but received {:?}",
//                 value
//             )))
//         }
//     }
// }

// impl AsC for Global {
//     type DefinitionType = crate::GlobalType;

//     fn as_cvalue(&self, _store: &impl AsStoreRef) -> wasm_bindgen::JsValue {
//         self.0.handle.global.clone().into()
//     }

//     fn from_cvalue(
//         store: &mut impl AsStoreMut,
//         global_type: &Self::DefinitionType,
//         value: &JsValue,
//     ) -> Result<Self, JsError> {
//         if value.is_instance_of::<JsGlobal>() {
//             Ok(Global::from_vm_extern(
//                 store,
//                 VMGlobal::new(
//                     value.clone().unchecked_into::<JsGlobal>(),
//                     global_type.clone(),
//                 ),
//             ))
//         } else {
//             Err(JsError::new(&format!(
//                 "Extern expect to be of type Global, but received {:?}",
//                 value
//             )))
//         }
//     }
// }

// impl AsC for Table {
//     type DefinitionType = crate::TableType;

//     fn as_cvalue(&self, _store: &impl AsStoreRef) -> wasm_bindgen::JsValue {
//         self.0.handle.table.clone().into()
//     }

//     fn from_cvalue(
//         store: &mut impl AsStoreMut,
//         table_type: &Self::DefinitionType,
//         value: &JsValue,
//     ) -> Result<Self, JsError> {
//         if value.is_instance_of::<JsTable>() {
//             Ok(Table::from_vm_extern(
//                 store,
//                 VMTable::new(
//                     value.clone().unchecked_into::<JsTable>(),
//                     table_type.clone(),
//                 ),
//             ))
//         } else {
//             Err(JsError::new(&format!(
//                 "Extern expect to be of type Table, but received {:?}",
//                 value
//             )))
//         }
//     }
// }
