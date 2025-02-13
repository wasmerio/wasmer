use wasmer_types::{TagType, Type};

use crate::{
    jsc::vm::VMTag,
    vm::{VMExtern, VMExternTag},
    AsStoreMut, AsStoreRef,
};

#[derive(Debug, Clone, PartialEq, Eq)]
/// A WebAssembly `tag` in the `v8` runtime.
pub(crate) struct Tag {
    pub(crate) handle: VMTag,
}

unsafe impl Send for Tag {}
unsafe impl Sync for Tag {}

// Tag can't be Send in js because it dosen't support `structuredClone`
// https://developer.mozilla.org/en-US/docs/Web/API/structuredClone
// unsafe impl Send for Tag {}

impl Tag {
    pub fn new<P: Into<Box<[Type]>>>(store: &mut impl AsStoreMut, params: P) -> Self {
        panic!("EH not supported yet!")
    }

    pub fn ty(&self, store: &impl AsStoreRef) -> TagType {
        panic!("EH not supported yet!")
    }

    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, vm_extern: VMExternTag) -> Self {
        panic!("EH not supported yet!")
    }

    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        panic!("EH not supported yet!")
    }

    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        panic!("EH not supported yet!")
    }
}
