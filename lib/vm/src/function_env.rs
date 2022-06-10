use std::{any::Any, sync::Arc};

/// Underlying FunctionEnvironment used by a `VMFunction`.
#[derive(Clone)]
pub struct VMFunctionEnvironment {
    contents: Arc<dyn Any + Send + 'static>,
}

impl VMFunctionEnvironment {
    /// Wraps the given value to expose it to Wasm code as a function context.
    pub fn new(val: impl Any + Send + 'static) -> Self {
        Self {
            contents: Arc::new(val),
        }
    }

    #[allow(clippy::should_implement_trait)]
    /// Returns a reference to the underlying value.
    pub fn as_ref(&self) -> &(dyn Any + Send + 'static) {
        &*self.contents
    }

    #[allow(clippy::should_implement_trait)]
    /// Returns a reference to the underlying value.
    pub fn as_mut(&mut self) -> Option<&mut (dyn Any + Send + 'static)> {
        Arc::get_mut(&mut self.contents)
    }
}
