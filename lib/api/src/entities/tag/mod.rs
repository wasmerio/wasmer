pub(crate) mod inner;
pub(crate) use inner::*;
use wasmer_types::{TagType, Type};

use crate::{
    vm::{VMExtern, VMExternTag},
    AsStoreMut, AsStoreRef, ExportError, Exportable, Extern,
};

/// A WebAssembly `tag` instance.
///
/// A tag instance is the runtime representation of a tag variable.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#tag-instances>
#[derive(Debug, Clone, PartialEq, Eq, derive_more::From)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct Tag(pub(crate) BackendTag);

impl Tag {
    /// Create a new tag with event of type P -> [], that is a function that takes parameters `P`
    /// and has no return value.
    //
    // Note: in the future, a tag might express other kinds of events other than just exceptions.
    // This would imply that the signature of this function becomes
    //
    // `pub fn new<R: Into<Box<[Type]>>, P: Into<Box<[Type]>>>(store: &mut impl AsStoreMut, kind: TagKind, params: P, rets: R) -> Self`
    //
    // For now, since the only possible kind is `TagKind::Exception`, we decided to make the
    // external API easier to use, while having the internal types in place to allow the needed
    // changes.
    pub fn new<P: Into<Box<[Type]>>>(store: &mut impl AsStoreMut, params: P) -> Self {
        Self(BackendTag::new(store, params))
    }

    /// Returns the [`TagType`] of the tag.
    pub fn ty(&self, store: &impl AsStoreRef) -> TagType {
        self.0.ty(store)
    }

    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, vm_extern: VMExternTag) -> Self {
        Self(BackendTag::from_vm_extern(store, vm_extern))
    }

    /// Checks whether this tag can be used with the given context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.0.is_from_store(store)
    }

    /// Create a [`VMExtern`] from self.
    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        self.0.to_vm_extern()
    }
}

impl<'a> Exportable<'a> for Tag {
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Tag(tag) => Ok(tag),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
