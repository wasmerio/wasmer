use std::any::Any;

use crate::js::utils::js_handle::JsHandle;
use js_sys::{Array, Function as JsFunction, Reflect, Symbol};
use wasm_bindgen::{JsCast, JsValue, prelude::wasm_bindgen};
use wasmer_types::{FunctionType, RawValue, Type};

#[wasm_bindgen(inline_js = r#"
let probe;
export function isWasmFunction(value) {
  probe ??= new WebAssembly.Table({ element: "anyfunc", initial: 1 });
  try { probe.set(0, value); } catch { return false; }
  probe.set(0, null);
  return true;
}
"#)]
extern "C" {
    /// Whether `value` is a WebAssembly function: one a table of functions
    /// takes.
    #[wasm_bindgen(js_name = isWasmFunction)]
    fn is_wasm_function(value: &JsValue) -> bool;
}

fn type_key() -> Symbol {
    Symbol::for_("wasmer.function-type")
}

fn encode_types(types: &[Type]) -> Array {
    Array::from_iter(types.iter().map(|ty| JsValue::from_f64(*ty as u8 as f64)))
}

fn decode_types(types: &Array) -> Option<Vec<Type>> {
    types
        .iter()
        .map(|value| match value.as_f64()? as u8 {
            value if value == Type::I32 as u8 => Some(Type::I32),
            value if value == Type::I64 as u8 => Some(Type::I64),
            value if value == Type::F32 as u8 => Some(Type::F32),
            value if value == Type::F64 as u8 => Some(Type::F64),
            value if value == Type::V128 as u8 => Some(Type::V128),
            value if value == Type::ExternRef as u8 => Some(Type::ExternRef),
            value if value == Type::FuncRef as u8 => Some(Type::FuncRef),
            value if value == Type::ExceptionRef as u8 => Some(Type::ExceptionRef),
            _ => None,
        })
        .collect()
}

/// The VM Function type
///
/// What a function keeps besides its type, it keeps on its JavaScript
/// object, which belongs to the worker that made it, rather than in the
/// handle, which any thread may clone and drop.
#[derive(Clone)]
pub struct VMFunction {
    pub(crate) function: JsHandle<JsFunction>,
    pub(crate) ty: FunctionType,
    /// Whether calls of the function take the numeric lane: worked out once
    /// from `ty`, which does not change, instead of on every call.
    pub(crate) numeric: bool,
    /// Whether the function may see the receiver it is called with: false
    /// for a WebAssembly function and for a host function of this crate,
    /// which do not look at it. `Function::call` gives the others `null`, and
    /// these none, which the guards of the numeric lane then call directly,
    /// faster than through `Reflect.apply` (see `numeric`).
    pub(crate) sees_receiver: bool,
    /// The store of a host function of this crate, for
    /// `Function::is_from_store` without `experimental-async`. With it,
    /// `jspi` keeps the store on the JavaScript function, where a function
    /// taken back from JavaScript finds it too; that costs a `WeakMap` entry
    /// per function, about a fifth of making one.
    #[cfg(not(feature = "experimental-async"))]
    pub(crate) store: Option<wasmer_types::StoreId>,
}

impl Eq for VMFunction {}

unsafe impl Send for VMFunction {}
unsafe impl Sync for VMFunction {}

impl VMFunction {
    pub(crate) fn new(function: JsFunction, ty: FunctionType) -> Self {
        Self::annotate_type(&function, &ty);
        Self {
            function: JsHandle::new(function),
            numeric: crate::js::function::numeric::is_numeric(&ty),
            ty,
            sees_receiver: true,
            #[cfg(not(feature = "experimental-async"))]
            store: None,
        }
    }

    /// The function as a host function of this crate made for the store
    /// `id`, which belongs to that store (see `Function::is_from_store`)
    /// and does not look at its receiver.
    pub(crate) fn of_store(self, id: wasmer_types::StoreId) -> Self {
        #[cfg(feature = "experimental-async")]
        crate::js::jspi::own(&self.function, id);
        Self {
            #[cfg(not(feature = "experimental-async"))]
            store: Some(id),
            ..self.without_receiver()
        }
    }

    /// The function as one that does not look at its receiver: a host
    /// function of this crate, or a WebAssembly function.
    pub(crate) fn without_receiver(self) -> Self {
        Self {
            sees_receiver: false,
            ..self
        }
    }

    /// The function, as one that does not look at its receiver if it is a
    /// WebAssembly function.
    pub(crate) fn probing_receiver(self) -> Self {
        match is_wasm_function(&self.function) {
            true => self.without_receiver(),
            false => self,
        }
    }

    /// `WebAssembly.promising(function)`, made by the first `call_async` and
    /// kept for the next ones (see `jspi::promising`).
    #[cfg(feature = "experimental-async")]
    pub(crate) fn promising(&self) -> Result<JsFunction, JsValue> {
        crate::js::jspi::promising(&self.function)
    }

    pub(crate) fn annotate_type(function: &JsFunction, ty: &FunctionType) {
        let encoded = Array::of2(
            &encode_types(ty.params()).into(),
            &encode_types(ty.results()).into(),
        );
        let _ = Reflect::set(function, type_key().as_ref(), &encoded);
    }

    pub(crate) fn type_from_js(function: &JsFunction) -> Option<FunctionType> {
        let encoded = Reflect::get(function, type_key().as_ref())
            .ok()?
            .dyn_into::<Array>()
            .ok()?;
        let params = encoded.get(0).dyn_into::<Array>().ok()?;
        let results = encoded.get(1).dyn_into::<Array>().ok()?;
        Some(FunctionType::new(
            decode_types(&params)?,
            decode_types(&results)?,
        ))
    }
}

impl PartialEq for VMFunction {
    fn eq(&self, other: &Self) -> bool {
        self.function == other.function
    }
}

impl std::fmt::Debug for VMFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VMFunction")
            .field("function", &self.function)
            .finish()
    }
}

/// Underlying FunctionEnvironment used by a `VMFunction`.
#[derive(Debug)]
pub struct VMFunctionEnvironment {
    pub(crate) contents: Box<dyn Any + Send + 'static>,
}

impl VMFunctionEnvironment {
    /// Wraps the given value to expose it to Wasm code as a function context.
    pub fn new(val: impl Any + Send + 'static) -> Self {
        Self {
            contents: Box::new(val),
        }
    }

    #[allow(clippy::should_implement_trait)]
    /// Returns a reference to the underlying value.
    pub fn as_ref(&self) -> &(dyn Any + Send + 'static) {
        &*self.contents
    }

    #[allow(clippy::should_implement_trait)]
    /// Returns a mutable reference to the underlying value.
    pub fn as_mut(&mut self) -> &mut (dyn Any + Send + 'static) {
        &mut *self.contents
    }

    /// Returns the underlying value, consuming the environment.
    pub fn into_contents(self) -> Box<dyn Any + Send + 'static> {
        self.contents
    }
}

#[repr(C)]
/// The type of function bodies in the `js` VM.
pub struct VMFunctionBody(u8);

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// The type of function references in the `js` VM.
pub(crate) struct VMFuncRef;

impl VMFuncRef {
    /// Converts the `VMFuncRef` into a `RawValue`.
    pub fn into_raw(self) -> RawValue {
        unimplemented!();
    }

    /// Extracts a `VMFuncRef` from a `RawValue`.
    ///
    /// # Safety
    /// `raw.funcref` must be a valid pointer.
    pub unsafe fn from_raw(_raw: RawValue) -> Option<Self> {
        unimplemented!();
    }
}

/// The type of function callbacks in the `js` VM.
pub type VMFunctionCallback = *const VMFunctionBody;

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen_test::wasm_bindgen_test;

    /// A WebAssembly function is taken for one that does not see its
    /// receiver; a JavaScript function is not.
    #[wasm_bindgen_test]
    fn only_a_webassembly_function_is_taken_for_one_without_a_receiver() {
        // (module (func (export "f")))
        let bytes: [u8; 31] = [
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x04, 0x01, 0x60, 0x00, 0x00,
            0x03, 0x02, 0x01, 0x00, 0x07, 0x05, 0x01, 0x01, 0x66, 0x00, 0x00, 0x0a, 0x04, 0x01,
            0x02, 0x00, 0x0b,
        ];
        let module =
            js_sys::WebAssembly::Module::new(&js_sys::Uint8Array::from(&bytes[..])).unwrap();
        let instance = js_sys::WebAssembly::Instance::new(&module, &js_sys::Object::new()).unwrap();
        let exports = Reflect::get(&instance, &"exports".into()).unwrap();
        let wasm: JsFunction = Reflect::get(&exports, &"f".into())
            .unwrap()
            .unchecked_into();
        let ty = FunctionType::new([], []);
        assert!(
            !VMFunction::new(wasm, ty.clone())
                .probing_receiver()
                .sees_receiver
        );

        let javascript: JsFunction = Closure::<dyn Fn()>::new(|| {})
            .into_js_value()
            .unchecked_into();
        assert!(
            VMFunction::new(javascript, ty)
                .probing_receiver()
                .sees_receiver
        );
    }
}
