//! Data types, functions and traits for `sys` runtime's `Tag` implementation.
use std::any::Any;

use wasmer_types::{TagType, Type};
use wasmer_vm::{StoreHandle, StoreId};

use crate::{sys::vm::VMExceptionRef, AsStoreMut, AsStoreRef, BackendTag, Tag, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
/// A WebAssembly `exnref` in the `sys` runtime.
pub(crate) struct Exception {
    exnref: wasmer_vm::VMExceptionRef,
}

impl Exception {
    /// Creates a new exception with the given tag and payload, and also creates
    /// a reference to it, returning the reference.
    #[allow(irrefutable_let_patterns)]
    pub fn new(store: &mut impl AsStoreMut, tag: Tag, payload: &[Value]) -> Self {
        if !tag.is_from_store(store) {
            panic!("cannot create Exception with Tag from another Store");
        }

        let BackendTag::Sys(tag) = &tag.0 else {
            panic!("cannot create Exception with Tag from another backend");
        };

        let store_id = store.objects_mut().id();

        let values = payload
            .iter()
            .map(|v| v.as_raw(store))
            .collect::<Vec<_>>()
            .into_boxed_slice();

        let ctx = store.objects_mut().as_sys_mut();
        let exception = wasmer_vm::VMExceptionObj::new(tag.handle.internal_handle(), values);
        let exn_handle = wasmer_vm::StoreHandle::new(ctx, exception);
        let exnref = wasmer_vm::VMExceptionRef(exn_handle);

        Self { exnref }
    }

    pub fn from_exnref(exnref: wasmer_vm::VMExceptionRef) -> Self {
        Self { exnref }
    }

    /// Checks whether this `Exception` can be used with the given context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.exnref.0.store_id() == store.as_store_ref().objects().id()
    }

    pub fn exnref(&self) -> wasmer_vm::VMExceptionRef {
        self.exnref.clone()
    }
}

#[cfg(feature = "artifact-size")]
impl loupe::MemoryUsage for Exception {
    fn size_of_val(&self, tracker: &mut dyn loupe::MemoryUsageTracker) -> usize {
        std::mem::size_of::<Self>()
    }
}
