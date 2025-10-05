use crate::backend::stub::panic_stub;
use crate::entities::store::{AsStoreMut, AsStoreRef};
use crate::vm::VMExternTag;
use wasmer_types::TagType;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag;

impl Tag {
    pub fn new<P: Into<Box<[crate::Type]>>>(_store: &mut impl AsStoreMut, _params: P) -> Self {
        panic_stub("cannot create tags")
    }

    pub fn ty(&self, _store: &impl AsStoreRef) -> TagType {
        panic_stub("does not expose tag types")
    }

    pub fn from_vm_extern(_store: &mut impl AsStoreMut, _vm_extern: VMExternTag) -> Self {
        panic_stub("cannot import tags")
    }

    pub fn to_vm_extern(&self) -> VMExternTag {
        VMExternTag::Stub(crate::backend::stub::vm::VMExternTag::stub())
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        panic_stub("cannot verify tag origins")
    }
}
