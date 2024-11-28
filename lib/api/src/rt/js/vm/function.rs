use std::any::Any;

use crate::js::utils::js_handle::JsHandle;
use js_sys::Function as JsFunction;
use wasmer_types::{FunctionType, RawValue};

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
        Self {
            function: JsHandle::new(function),
            ty,
        }
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
