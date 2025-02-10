//! Data types, functions and traits for `sys` runtime's `Tag` implementation.
use wasmer_types::{FunctionType, TagType, Type};
use wasmer_vm::StoreHandle;

use crate::{
    sys::vm::VMTag,
    vm::{VMExtern, VMExternTag},
    AsStoreMut, AsStoreRef,
};

#[derive(Debug, Clone, PartialEq, Eq)]
/// A WebAssembly `tag` in the `v8` runtime.
pub(crate) struct Tag {
    pub(crate) handle: StoreHandle<wasmer_vm::VMTag>,
}

unsafe impl Send for Tag {}
unsafe impl Sync for Tag {}

// Tag can't be Send in js because it dosen't support `structuredClone`
// https://developer.mozilla.org/en-US/docs/Web/API/structuredClone
// unsafe impl Send for Tag {}

impl Tag {
    /// Create a new [`Tag`].
    pub fn new<P: Into<Box<[Type]>>>(store: &mut impl AsStoreMut, params: P) -> Self {
        Self {
            handle: StoreHandle::new(
                store.objects_mut().as_sys_mut(),
                VMTag::new(
                    wasmer_types::TagKind::Exception,
                    FunctionType::new(params, []),
                ),
            ),
        }
    }

    /// Get the [`Tag`]'s type.
    pub fn ty(&self, store: &impl AsStoreRef) -> TagType {
        TagType {
            kind: wasmer_types::TagKind::Exception,
            ty: self
                .handle
                .get(store.as_store_ref().objects().as_sys())
                .signature
                .clone(),
        }
    }

    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, vm_extern: VMExternTag) -> Self {
        Self {
            handle: unsafe {
                StoreHandle::from_internal(store.objects_mut().id(), vm_extern.into_sys())
            },
        }
    }

    /// Check whether or not the [`Tag`] is from the given store.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.handle.store_id() == store.as_store_ref().objects().id()
    }

    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Sys(wasmer_vm::VMExtern::Tag(self.handle.internal_handle()))
    }
}
