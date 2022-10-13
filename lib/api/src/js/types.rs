//use crate::js::externals::Function;
// use crate::js::store::{Store, StoreObject};
// use crate::js::RuntimeError;
use crate::js::store::AsStoreRef;
use crate::js::value::Value;
use wasm_bindgen::JsValue;
pub use wasmer_types::{
    ExportType, ExternType, FunctionType, GlobalType, ImportType, MemoryType, Mutability,
    TableType, Type as ValType,
};

/// WebAssembly computations manipulate values of basic value types:
/// * Integers (32 or 64 bit width)
/// * Floating-point (32 or 64 bit width)
/// * Vectors (128 bits, with 32 or 64 bit lanes)
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#values>
// pub type Value = ();
//pub type Value = Value<Function>;

pub trait AsJs {
    fn as_jsvalue(&self, store: &impl AsStoreRef) -> JsValue;
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
    fn as_jsvalue(&self, store: &impl AsStoreRef) -> JsValue {
        match self {
            Self::I32(i) => JsValue::from_f64(*i as f64),
            Self::I64(i) => JsValue::from_f64(*i as f64),
            Self::F32(f) => JsValue::from_f64(*f as f64),
            Self::F64(f) => JsValue::from_f64(*f),
            Self::V128(f) => JsValue::from_f64(*f as f64),
            Self::FuncRef(Some(func)) => func
                .handle
                .get(store.as_store_ref().objects())
                .function
                .clone()
                .into(),
            Self::FuncRef(None) => JsValue::null(),
        }
    }
}
