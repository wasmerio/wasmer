//use crate::js::externals::Function;
// use crate::store::{Store, StoreObject};
// use crate::js::RuntimeError;
use crate::imports::Imports;
use crate::js::externals::Extern;
use crate::js::vm::VMExtern;
use crate::store::{AsStoreMut, AsStoreRef};
use crate::value::Value;
use crate::ValType;
use std::collections::HashMap;
use wasm_bindgen::JsValue;
use wasmer_types::ExternType;

/// Convert the given type to a [`JsValue`].
pub trait AsJs {
    /// The inner definition type from this Javascript object
    type DefinitionType;
    /// Convert the given type to a [`JsValue`].
    fn as_jsvalue(&self, store: &impl AsStoreRef) -> JsValue;
    /// Convert the given type to a [`JsValue`].
    fn from_jsvalue(
        &self,
        store: &mut impl AsStoreMut,
        type_: &Self::DefinitionType,
        value: &JsValue,
    ) -> Self;
}

#[inline]
pub fn param_from_js(ty: &ValType, js_val: &JsValue) -> Value {
    match ty {
        ValType::I32 => Value::I32(js_val.as_f64().unwrap() as _),
        ValType::I64 => Value::I64(js_val.as_f64().unwrap() as _),
        ValType::F32 => Value::F32(js_val.as_f64().unwrap() as _),
        ValType::F64 => Value::F64(js_val.as_f64().unwrap()),
        t => unimplemented!(
            "The type `{:?}` is not yet supported in the JS Function API",
            t
        ),
    }
}

impl AsJs for Value {
    type DefinitionType = ValType;

    fn as_jsvalue(&self, _store: &impl AsStoreRef) -> JsValue {
        match self {
            Self::I32(i) => JsValue::from_f64(*i as f64),
            Self::I64(i) => JsValue::from_f64(*i as f64),
            Self::F32(f) => JsValue::from_f64(*f as f64),
            Self::F64(f) => JsValue::from_f64(*f),
            Self::V128(f) => JsValue::from_f64(*f as f64),
            Self::FuncRef(Some(func)) => func.handle.function.clone().into(),
            Self::FuncRef(None) => JsValue::null(),
            Self::ExternRef(_) => unimplemented!(),
        }
    }

    fn from_jsvalue(
        &self,
        _store: &mut impl AsStoreMut,
        type_: &Self::DefinitionType,
        value: &JsValue,
    ) -> Self {
        param_from_js(type_, value)
    }
}

impl AsJs for wasmer_types::RawValue {
    type DefinitionType = ValType;

    fn as_jsvalue(&self, _store: &impl AsStoreRef) -> JsValue {
        unsafe { JsValue::from_f64(self.into()) }
    }

    fn from_jsvalue(
        &self,
        _store: &mut impl AsStoreMut,
        type_: &Self::DefinitionType,
        value: &JsValue,
    ) -> Self {
        unimplemented!();
    }
}

impl AsJs for Imports {
    type DefinitionType = crate::js::module::Module;

    // Annotation is here to prevent spurious IDE warnings.
    #[allow(unused_unsafe)]
    fn as_jsvalue(&self, store: &impl AsStoreRef) -> wasm_bindgen::JsValue {
        // /// Returns the `Imports` as a Javascript `Object`
        // pub fn as_jsobject(&self, store: &impl AsStoreRef) -> js_sys::Object {
        //     let imports = js_sys::Object::new();
        //     let namespaces: HashMap<&str, Vec<(&str, &Extern)>> =
        //         self.map
        //             .iter()
        //             .fold(HashMap::default(), |mut acc, ((ns, name), ext)| {
        //                 acc.entry(ns.as_str())
        //                     .or_default()
        //                     .push((name.as_str(), ext));
        //                 acc
        //             });

        //     for (ns, exports) in namespaces.into_iter() {
        //         let import_namespace = js_sys::Object::new();
        //         for (name, ext) in exports {
        //             // Annotation is here to prevent spurious IDE warnings.
        //             #[allow(unused_unsafe)]
        //             unsafe {
        //                 js_sys::Reflect::set(&import_namespace, &name.into(), &ext.as_jsvalue(store))
        //                     .expect("Error while setting into the js namespace object");
        //             }
        //         }
        //         // Annotation is here to prevent spurious IDE warnings.
        //         #[allow(unused_unsafe)]
        //         unsafe {
        //             js_sys::Reflect::set(&imports, &ns.into(), &import_namespace.into())
        //                 .expect("Error while setting into the js imports object");
        //         }
        //     }
        //     imports
        // }

        let imports_object = js_sys::Object::new();
        for (namespace, name, extern_) in self.iter() {
            let val = unsafe { js_sys::Reflect::get(&imports_object, &namespace.into()).unwrap() };
            if !val.is_undefined() {
                // If the namespace is already set

                // Annotation is here to prevent spurious IDE warnings.
                #[allow(unused_unsafe)]
                unsafe {
                    js_sys::Reflect::set(
                        &val,
                        &name.into(),
                        &extern_.as_jsvalue(&store.as_store_ref()),
                    )
                    .unwrap();
                }
            } else {
                // If the namespace doesn't exist
                let import_namespace = js_sys::Object::new();
                #[allow(unused_unsafe)]
                unsafe {
                    js_sys::Reflect::set(
                        &import_namespace,
                        &name.into(),
                        &extern_.as_jsvalue(&store.as_store_ref()),
                    )
                    .unwrap();
                    js_sys::Reflect::set(
                        &imports_object,
                        &namespace.into(),
                        &import_namespace.into(),
                    )
                    .unwrap();
                }
            }
        }
        imports_object.into()
    }

    fn from_jsvalue(
        &self,
        store: &mut impl AsStoreMut,
        module: &Self::DefinitionType,
        value: &JsValue,
    ) -> Self {
        let module_imports: HashMap<(String, String), ExternType> = module
            .imports()
            .map(|import| {
                (
                    (import.module().to_string(), import.name().to_string()),
                    import.ty().clone(),
                )
            })
            .collect::<HashMap<(String, String), ExternType>>();

        let mut map: HashMap<(String, String), Extern> = HashMap::new();
        let object: js_sys::Object = value.clone().into();
        for module_entry in js_sys::Object::entries(&object).iter() {
            let module_entry: js_sys::Array = module_entry.into();
            let module_name = module_entry.get(0).as_string().unwrap().to_string();
            let module_import_object: js_sys::Object = module_entry.get(1).into();
            for import_entry in js_sys::Object::entries(&module_import_object).iter() {
                let import_entry: js_sys::Array = import_entry.into();
                let import_name = import_entry.get(0).as_string().unwrap().to_string();
                let import_js: wasm_bindgen::JsValue = import_entry.get(1);
                let key = (module_name.clone(), import_name);
                let extern_type = module_imports.get(&key).unwrap();
                let export =
                    VMExtern::from_js_value(import_js, store, extern_type.clone()).unwrap();
                let extern_ = Extern::from_vm_extern(store, export);
                map.insert(key, extern_);
            }
        }

        Self { map }
    }
}

impl AsJs for Extern {
    type DefinitionType = ExternType;

    fn as_jsvalue(&self, store: &impl AsStoreRef) -> wasm_bindgen::JsValue {
        match self {
            Self::Function(_) => self.to_vm_extern().as_jsvalue(store),
            Self::Global(_) => self.to_vm_extern().as_jsvalue(store),
            Self::Table(_) => self.to_vm_extern().as_jsvalue(store),
            Self::Memory(_) => self.to_vm_extern().as_jsvalue(store),
        }
        .clone()
    }
    fn from_jsvalue(
        &self,
        _store: &mut impl AsStoreMut,
        type_: &Self::DefinitionType,
        value: &JsValue,
    ) -> Self {
        unimplemented!();
    }
}
