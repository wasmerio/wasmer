use crate::imports::Imports;
use crate::instance::Instance;
use crate::jsc::instance::Instance as JsInstance;
use crate::jsc::vm::{VMFunction, VMGlobal, VMMemory, VMTable};
use crate::store::{AsStoreMut, AsStoreRef};
use crate::value::Value;
use crate::Type;
use crate::{Extern, Function, Global, Memory, Table};
use rusty_jsc::{JSContext, JSObject, JSValue};
use std::collections::HashMap;
use std::convert::TryInto;
use wasmer_types::ExternType;

/// Convert the given type to a [`JsValue`].
pub trait AsJs: Sized {
    /// The inner definition type from this Javascript object
    type DefinitionType;
    /// Convert the given type to a [`JsValue`].
    fn as_jsvalue(&self, store: &impl AsStoreRef) -> JSValue;
    /// Convert the given type to a [`JsValue`].
    fn from_jsvalue(
        store: &mut impl AsStoreMut,
        type_: &Self::DefinitionType,
        value: &JSValue,
    ) -> Result<Self, JSValue>;
}

#[inline]
pub fn param_from_js(context: &JSContext, ty: &Type, js_val: &JSValue) -> Value {
    match ty {
        Type::I32 => Value::I32(js_val.to_number(&context) as _),
        Type::I64 => {
            // TODO: use to_number as error
            // let number = js_val.as_f64().map(|f| f as i64).unwrap_or_else(|| {
            //     if js_val.is_bigint() {
            //         // To support BigInt
            //         let big_num: u128 = js_sys::BigInt::from(js_val.clone()).try_into().unwrap();
            //         big_num as i64
            //     } else {
            //         (js_sys::Number::from(js_val.clone()).as_f64().unwrap()) as i64
            //     }
            // });
            // println!("Param from js: {}, {}", ty, js_val.to_string(&context));
            let number = if js_val.is_number(&context) {
                js_val.to_number(&context) as _
            } else {
                js_val.to_string(&context).parse().unwrap()
            };
            Value::I64(number)
        }
        Type::F32 => Value::F32(js_val.to_number(&context) as _),
        Type::F64 => Value::F64(js_val.to_number(&context)),
        Type::V128 => {
            let number = if js_val.is_number(&context) {
                js_val.to_number(&context) as _
            } else {
                js_val.to_string(&context).parse().unwrap()
            };
            Value::V128(number)
        }
        Type::ExternRef | Type::FuncRef => unimplemented!(
            "The type `{:?}` is not yet supported in the JS Function API",
            ty
        ),
    }
}

impl AsJs for Value {
    type DefinitionType = Type;

    fn as_jsvalue(&self, store: &impl AsStoreRef) -> JSValue {
        let engine = store.as_store_ref();
        let context = engine.engine().0.context();
        match self {
            Self::I32(i) => JSValue::number(&context, *i as _),
            // JavascriptCore will fail with:
            // new WebAssembly.Global({value: "i64", mutable: false}, 3);
            // But will succeed with
            // new WebAssembly.Global({value: "i64", mutable: false}, "3");
            Self::I64(i) => JSValue::string(&context, (*i).to_string()).unwrap(),
            Self::F32(f) => JSValue::number(&context, *f as _),
            Self::F64(f) => JSValue::number(&context, *f),
            Self::V128(v) => JSValue::number(&context, *v as _),
            Self::FuncRef(Some(func)) => func.0.handle.function.clone().to_jsvalue(),
            Self::FuncRef(None) => JSValue::null(&context),
            Self::ExternRef(_) => unimplemented!(),
        }
    }

    fn from_jsvalue(
        store: &mut impl AsStoreMut,
        type_: &Self::DefinitionType,
        value: &JSValue,
    ) -> Result<Self, JSValue> {
        let engine = store.as_store_ref();
        let context = engine.engine().0.context();
        Ok(param_from_js(context, type_, value))
    }
}

// impl AsJs for Imports {
//     type DefinitionType = crate::module::Module;

//     // Annotation is here to prevent spurious IDE warnings.
//     #[allow(unused_unsafe)]
//     fn as_jsvalue(&self, store: &impl AsStoreRef) -> wasm_bindgen::JsValue {
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
//                         &extern_.as_jsvalue(&store.as_store_ref()),
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
//                         &extern_.as_jsvalue(&store.as_store_ref()),
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

//     fn from_jsvalue(
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
//                 let extern_ = Extern::from_jsvalue(store, extern_type, &import_js)?;
//                 map.insert(key, extern_);
//             }
//         }

//         Ok(Self { map })
//     }
// }

impl AsJs for Extern {
    type DefinitionType = ExternType;

    fn as_jsvalue(&self, _store: &impl AsStoreRef) -> JSValue {
        match self {
            Self::Memory(memory) => memory.0.handle.memory.clone().to_jsvalue(),
            Self::Function(function) => function.0.handle.function.clone().to_jsvalue(),
            Self::Table(table) => table.0.handle.table.clone().to_jsvalue(),
            Self::Global(global) => global.0.handle.global.clone().to_jsvalue(),
        }
    }

    fn from_jsvalue(
        store: &mut impl AsStoreMut,
        extern_type: &Self::DefinitionType,
        val: &JSValue,
    ) -> Result<Self, JSValue> {
        // Note: this function do a soft check over the type
        // We only check the "kind" of Extern, but nothing else
        // unimplemented!();
        let engine = store.as_store_mut();
        let context = engine.engine().0.context();
        match extern_type {
            ExternType::Function(function_type) => {
                let obj_val = val.to_object(&context);
                Ok(Self::Function(Function::from_vm_extern(
                    store,
                    VMFunction::new(obj_val, function_type.clone()),
                )))
            }
            ExternType::Global(global_type) => {
                let obj_val = val.to_object(&context);
                Ok(Self::Global(Global::from_vm_extern(
                    store,
                    VMGlobal::new(obj_val, global_type.clone()),
                )))
            }
            ExternType::Memory(memory_type) => {
                let obj_val = val.to_object(&context);
                Ok(Self::Memory(Memory::from_vm_extern(
                    store,
                    VMMemory::new(obj_val, memory_type.clone()),
                )))
            }
            ExternType::Table(table_type) => {
                let obj_val = val.to_object(&context);
                Ok(Self::Table(Table::from_vm_extern(
                    store,
                    VMTable::new(obj_val, table_type.clone()),
                )))
            }
        }
    }
}

// impl AsJs for Instance {
//     type DefinitionType = crate::module::Module;
//     fn as_jsvalue(&self, _store: &impl AsStoreRef) -> wasm_bindgen::JsValue {
//         self._inner._handle.clone().into()
//     }

//     fn from_jsvalue(
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
