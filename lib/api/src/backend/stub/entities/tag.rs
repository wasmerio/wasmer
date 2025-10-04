use crate::store::{AsStoreMut, AsStoreRef};
use crate::vm::VMExternTag;
use wasmer_types::TagType;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tag;

impl Tag {
    pub fn new<P: Into<Box<[crate::Type]>>>(
        _store: &mut impl AsStoreMut,
        _params: P,
    ) -> Self {
        panic!("stub backend cannot create tags")
    }

    pub fn ty(&self, _store: &impl AsStoreRef) -> TagType {
        panic!("stub backend does not expose tag types")
    }

    pub fn from_vm_extern(
        _store: &mut impl AsStoreMut,
        _vm_extern: VMExternTag,
    ) -> Self {
        panic!("stub backend cannot import tags")
    }

    pub fn to_vm_extern(&self) -> VMExternTag {
        panic!("stub backend cannot expose VM tags")
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        panic!("stub backend cannot verify tag origins")
    }
}
