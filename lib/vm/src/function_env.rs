use std::any::Any;

/// Underlying FunctionEnvironment used by a `VMFunction`.
pub struct VMFunctionEnvironment<Object = wasmer_types::BoxStoreObject> {
    /// The contents of the environment.
    pub contents: Object,
}

impl<Object> std::fmt::Debug for VMFunctionEnvironment<Object> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VMFunctionEnvironment")
            .field("contents", &"...")
            .finish()
    }
}

impl VMFunctionEnvironment {
    /// Wraps the given value to expose it to Wasm code as a function context.
    pub fn new(val: impl Any + Send + 'static) -> Self {
        Self {
            contents: Box::new(val),
        }
    }
}

impl<Object> VMFunctionEnvironment<Object> {
    #[allow(clippy::should_implement_trait)]
    /// Returns a reference to the underlying value.
    pub fn as_ref(&self) -> &Object {
        &self.contents
    }

    #[allow(clippy::should_implement_trait)]
    /// Returns a mutable reference to the underlying value.
    pub fn as_mut(&mut self) -> &mut Object {
        &mut self.contents
    }
}
