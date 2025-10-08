//! Data types, functions and traits for `sys` runtime's `Tag` implementation.

use super::store::StoreHandle;
use crate::{
    js::vm::{VMException, VMExceptionRef},
    AsStoreMut, AsStoreRef, Tag, Value,
};
use std::any::Any;
use wasmer_types::{TagType, Type};

#[derive(Debug, Clone, PartialEq, Eq)]
/// A WebAssembly `tag` in the `v8` runtime.
pub(crate) struct Exception {
    pub(crate) handle: VMException,
}

unsafe impl Send for Exception {}
unsafe impl Sync for Exception {}

impl Exception {
    /// Create a new [`Exception`].
    pub fn new(store: &mut impl AsStoreMut, tag: Tag, payload: &[Value]) -> Self {
        unimplemented!("Exception handling is not yet supported in js");
    }
}
