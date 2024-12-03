use std::any::Any;

/// Underlying FunctionEnvironment used by a `VMFunction`.
pub struct VMFunctionEnvironment {
    /// The contents of the environment.
    pub contents: Box<dyn Any + Send + 'static>,
}

impl std::fmt::Debug for VMFunctionEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VMFunctionEnvironment")
            .field("contents", &(&*self.contents as *const _))
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
