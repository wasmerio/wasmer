use std::any::Any;
use wasmer_types::RawValue;

use crate::store::InternalStoreHandle;

/// Underlying object referenced by a `VMExceptionRef`.
#[derive(Debug)]
pub struct VMExceptionObj {
    contents: Box<dyn Any + Send + Sync + 'static>,
}

impl VMExceptionObj {
    /// Wraps the given value to expose it to Wasm code as an externref.
    pub fn new(val: impl Any + Send + Sync + 'static) -> Self {
        Self {
            contents: Box::new(val),
        }
    }

    #[allow(clippy::should_implement_trait)]
    /// Returns a reference to the underlying value.
    pub fn as_ref(&self) -> &(dyn Any + Send + Sync + 'static) {
        &*self.contents
    }
}

/// Represents an opaque reference to any data within WebAssembly.
#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct VMExceptionRef(pub InternalStoreHandle<VMExceptionObj>);

impl VMExceptionRef {
    /// Converts the [`VMExceptionRef`] into a `RawValue`.
    pub fn into_raw(self) -> RawValue {
        RawValue {
            funcref: self.0.index(),
        }
    }

    /// Extracts a `VMExceptionRef` from a `RawValue`.
    ///
    /// # Safety
    /// `raw` must be a valid `VMExceptionRef` instance.
    pub unsafe fn from_raw(raw: RawValue) -> Option<Self> {
        InternalStoreHandle::from_index(raw.externref).map(Self)
    }
}
