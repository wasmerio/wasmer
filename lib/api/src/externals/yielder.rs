use async_wormhole::AsyncYielder;

use crate::{RuntimeError, Val};

/// Wrapper around `async-wormhole`'s yielder
#[derive(Clone)]
pub struct Yielder {
    inner: *const std::ffi::c_void,
}

impl Yielder {
    /// Create a new instance from a raw pointer to a yielder
    pub fn new(inner: *const std::ffi::c_void) -> Self {
        Self { inner }
    }

    /// Get the `AsyncYielder`
    pub fn get(&self) -> &mut AsyncYielder<Result<Box<[Val]>, RuntimeError>> {
        let yielder: &mut AsyncYielder<Result<Box<[Val]>, RuntimeError>> =
            unsafe { std::mem::transmute(self.inner) };

        yielder
    }
}

//FIXME
unsafe impl Send for Yielder {}
unsafe impl Sync for Yielder {}
