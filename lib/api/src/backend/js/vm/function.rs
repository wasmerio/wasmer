use std::any::Any;

use crate::js::utils::js_handle::JsHandle;
use js_sys::{Array, Function as JsFunction, Reflect, Symbol};
use wasm_bindgen::{JsCast, JsValue};
use wasmer_types::{FunctionType, RawValue, Type};

fn type_key() -> Symbol {
    Symbol::for_("wasmer.function-type")
}

fn encode_types(types: &[Type]) -> Array {
    Array::from_iter(types.iter().map(|ty| {
        JsValue::from_f64(match ty {
            Type::I32 => 0.0,
            Type::I64 => 1.0,
            Type::F32 => 2.0,
            Type::F64 => 3.0,
            Type::V128 => 4.0,
            Type::ExternRef => 5.0,
            Type::FuncRef => 6.0,
            Type::ExceptionRef => 7.0,
        })
    }))
}

fn decode_types(types: &Array) -> Option<Vec<Type>> {
    types
        .iter()
        .map(|value| match value.as_f64()? as u8 {
            0 => Some(Type::I32),
            1 => Some(Type::I64),
            2 => Some(Type::F32),
            3 => Some(Type::F64),
            4 => Some(Type::V128),
            5 => Some(Type::ExternRef),
            6 => Some(Type::FuncRef),
            7 => Some(Type::ExceptionRef),
            _ => None,
        })
        .collect()
}

/// The VM Function type
#[derive(Clone, Eq)]
pub struct VMFunction {
    pub(crate) function: JsHandle<JsFunction>,
    pub(crate) ty: FunctionType,
}

unsafe impl Send for VMFunction {}
unsafe impl Sync for VMFunction {}

impl VMFunction {
    pub(crate) fn new(function: JsFunction, ty: FunctionType) -> Self {
        Self::annotate_type(&function, &ty);
        Self {
            function: JsHandle::new(function),
            ty,
        }
    }

    pub(crate) fn annotate_type(function: &JsFunction, ty: &FunctionType) {
        let encoded = Array::of2(
            &encode_types(ty.params()).into(),
            &encode_types(ty.results()).into(),
        );
        let _ = Reflect::set(&function, type_key().as_ref(), &encoded);
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
