//! Data types, functions and traits for `sys` runtime's `Tag` implementation.
use std::any::Any;

use wasmer_types::{TagType, Type};
use wasmer_vm::{StoreHandle, StoreId};

use crate::{
    AsStoreMut, AsStoreRef, BackendTag, Tag, Value,
    sys::vm::{VMExceptionObj, VMExceptionRef},
};

#[derive(Debug, Clone, PartialEq, Eq)]
/// A WebAssembly `exnref` in the `sys` runtime.
pub(crate) struct Exception {
    exnref: wasmer_vm::VMExceptionRef,
}

impl Exception {
    /// Creates a new exception with the given tag and payload, and also creates
    /// a reference to it, returning the reference.
    #[allow(irrefutable_let_patterns)]
    pub fn new(store: &mut impl AsStoreMut, tag: &crate::sys::tag::Tag, payload: &[Value]) -> Self {
        if !tag.is_from_store(store) {
            panic!("cannot create Exception with Tag from another Store");
        }

        let store_objects = store.objects().as_sys();
        let store_id = store_objects.id();

        let tag_ty = tag.handle.get(store_objects).signature.params();

        if tag_ty.len() != payload.len() {
            panic!("payload length mismatch");
        }

        let values = payload
            .iter()
            .enumerate()
            .map(|(i, v)| {
                if v.ty() != tag_ty[i] {
                    panic!("payload type mismatch");
                }
                v.as_raw(store)
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();

        let ctx = store.objects_mut().as_sys_mut();
        let exception = wasmer_vm::VMExceptionObj::new(tag.handle.internal_handle(), values);
        let exn_handle = wasmer_vm::StoreHandle::new(ctx, exception);
        let exnref = wasmer_vm::VMExceptionRef(exn_handle);

        Self { exnref }
    }

    pub fn exnref(&self) -> wasmer_vm::VMExceptionRef {
        self.exnref.clone()
    }

    pub fn from_exnref(exnref: wasmer_vm::VMExceptionRef) -> Self {
        Self { exnref }
    }

    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.exnref.0.store_id() == store.objects().id()
    }

    pub fn tag(&self, store: &impl AsStoreRef) -> crate::sys::tag::Tag {
        if !self.is_from_store(store) {
            panic!("Exception is from another Store");
        }
        let ctx = store.objects().as_sys();
        let exception = self.exnref.0.get(ctx);
        let tag_handle = exception.tag();
        crate::sys::tag::Tag {
            handle: unsafe { StoreHandle::from_internal(ctx.id(), tag_handle) },
        }
    }

    pub fn payload(&self, store: &mut impl AsStoreMut) -> Vec<Value> {
        if !self.is_from_store(store) {
            panic!("Exception is from another Store");
        }
        let ctx = store.objects().as_sys();
        let exception = self.exnref.0.get(ctx);
        let params_ty = exception.tag().get(ctx).signature.params().to_vec();
        let payload_ptr = exception.payload();

        assert_eq!(params_ty.len(), payload_ptr.len());

        params_ty
            .iter()
            .zip(unsafe { payload_ptr.as_ref().iter() })
            .map(|(ty, raw)| unsafe { Value::from_raw(store, *ty, *raw) })
            .collect()
    }
}

#[cfg(feature = "artifact-size")]
impl loupe::MemoryUsage for Exception {
    fn size_of_val(&self, tracker: &mut dyn loupe::MemoryUsageTracker) -> usize {
        std::mem::size_of::<Self>()
    }
}
