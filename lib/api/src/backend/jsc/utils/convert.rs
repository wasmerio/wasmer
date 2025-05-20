use rusty_jsc::{JSContext, JSValue};
use wasmer_types::{ExternType, Type};

use crate::{jsc::engine::IntoJSC, AsStoreMut, AsStoreRef, Extern, Function, Value};

/// Convert the given type to a [`JsValue`].
pub trait AsJsc: Sized {
    /// The inner definition type from this Javascript object
    type DefinitionType;
    /// Convert the given type to a [`JsValue`].
    fn as_jsc_value(&self, store: &impl AsStoreRef) -> JSValue;
    /// Convert the given type to a [`JsValue`].
    fn from_jsc_value(
        store: &mut impl AsStoreMut,
        type_: &Self::DefinitionType,
        value: &JSValue,
    ) -> Result<Self, JSValue>;
}

#[inline]
pub fn jsc_value_to_wasmer(context: &JSContext, ty: &Type, js_val: &JSValue) -> Value {
    match ty {
        Type::I32 => Value::I32(js_val.to_number(&context).unwrap() as _),
        Type::I64 => {
            let number = if js_val.is_number(&context) {
                js_val.to_number(&context).unwrap() as _
            } else {
                js_val
                    .to_string(&context)
                    .unwrap()
                    .to_string()
                    .parse()
                    .unwrap()
            };
            Value::I64(number)
        }
        Type::F32 => Value::F32(js_val.to_number(&context).unwrap() as _),
        Type::F64 => Value::F64(js_val.to_number(&context).unwrap()),
        Type::V128 => {
            let number = if js_val.is_number(&context) {
                js_val.to_number(&context).unwrap() as _
            } else {
                js_val
                    .to_string(&context)
                    .unwrap()
                    .to_string()
                    .parse()
                    .unwrap()
            };
            Value::V128(number)
        }
        Type::ExternRef | Type::FuncRef | Type::ExceptionRef => unimplemented!(
            "The type `{:?}` is not yet supported in the JSC Function API",
            ty
        ),
    }
}

impl AsJsc for Value {
    type DefinitionType = Type;

    fn as_jsc_value(&self, store: &impl AsStoreRef) -> JSValue {
        let engine = store.as_store_ref();
        let context = engine.jsc().context();
        match self {
            Self::I32(i) => JSValue::number(&context, *i as _),
            // JavascriptCore will fail with:
            // new WebAssembly.Global({value: "i64", mutable: false}, 3);
            // But will succeed with
            // new WebAssembly.Global({value: "i64", mutable: false}, "3");
            Self::I64(i) => JSValue::string(&context, (*i).to_string()),
            Self::F32(f) => JSValue::number(&context, *f as _),
            Self::F64(f) => JSValue::number(&context, *f),
            Self::V128(v) => JSValue::number(&context, *v as _),
            Self::FuncRef(Some(func)) => func.as_jsc().handle.function.clone().to_jsvalue(),
            Self::FuncRef(None) => JSValue::null(&context),
            Self::ExternRef(_) => {
                unimplemented!("ExternRefs are not yet supported in the JSC Function API",)
            }
            Self::ExceptionRef(_) => {
                unimplemented!("ExceptionRefs are not yet supported in the JSC Function API",)
            }
        }
    }

    fn from_jsc_value(
        store: &mut impl AsStoreMut,
        type_: &Self::DefinitionType,
        value: &JSValue,
    ) -> Result<Self, JSValue> {
        let engine = store.as_store_ref();
        let context = engine.jsc().context();
        Ok(jsc_value_to_wasmer(context, type_, value))
    }
}

impl AsJsc for Extern {
    type DefinitionType = ExternType;

    fn as_jsc_value(&self, _store: &impl AsStoreRef) -> JSValue {
        match self {
            Self::Memory(memory) => memory.as_jsc().handle.memory.clone().to_jsvalue(),
            Self::Function(function) => function.as_jsc().handle.function.clone().to_jsvalue(),
            Self::Table(table) => table.as_jsc().handle.table.clone().to_jsvalue(),
            Self::Global(global) => global.as_jsc().handle.global.clone().to_jsvalue(),
            Self::Tag(_) => unimplemented!("Tags are not yet supported in the JS Function API"),
        }
    }

    fn from_jsc_value(
        store: &mut impl AsStoreMut,
        extern_type: &Self::DefinitionType,
        val: &JSValue,
    ) -> Result<Self, JSValue> {
        // Note: this function do a soft check over the type
        // We only check the "kind" of Extern, but nothing else
        // unimplemented!();
        let engine = store.as_store_mut();
        let context = engine.jsc().context();
        match extern_type {
            ExternType::Function(function_type) => {
                let obj_val = val.to_object(&context).unwrap();
                Ok(Self::Function(Function::from_vm_extern(
                    store,
                    crate::vm::VMExternFunction::Jsc(
                        crate::backend::jsc::vm::VMExternFunction::new(
                            obj_val,
                            function_type.clone(),
                        ),
                    ),
                )))
            }
            ExternType::Global(global_type) => {
                let obj_val = val.to_object(&context).unwrap();
                Ok(Self::Global(crate::Global::from_vm_extern(
                    store,
                    crate::vm::VMExternGlobal::Jsc(crate::backend::jsc::vm::VMExternGlobal::new(
                        obj_val,
                        global_type.clone(),
                    )),
                )))
            }
            ExternType::Memory(memory_type) => {
                let obj_val = val.to_object(&context).unwrap();
                Ok(Self::Memory(crate::Memory::from_vm_extern(
                    store,
                    crate::vm::VMExternMemory::Jsc(crate::backend::jsc::vm::VMExternMemory::new(
                        obj_val,
                        memory_type.clone(),
                    )),
                )))
            }
            ExternType::Table(table_type) => {
                let obj_val = val.to_object(&context).unwrap();
                Ok(Self::Table(crate::Table::from_vm_extern(
                    store,
                    crate::vm::VMExternTable::Jsc(crate::backend::jsc::vm::VMExternTable::new(
                        obj_val,
                        table_type.clone(),
                    )),
                )))
            }
            ExternType::Tag(tag) => {
                panic!("EH not supported in `jsc` rt")
            }
        }
    }
}
