use wasmer_types::{TagType, Type};

use crate::{
    macros::backend::{gen_rt_ty, match_rt},
    vm::{VMExtern, VMExternTag},
    AsStoreMut, AsStoreRef, ExportError, Exportable, Extern,
};

/// A WebAssembly `global` instance.
///
/// A global instance is the runtime representation of a global variable.
/// It consists of an individual value and a flag indicating whether it is mutable.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#global-instances>
gen_rt_ty!(Tag
    @cfg feature = "artifact-size" => derive(loupe::MemoryUsage)
    @derives Debug, Clone, PartialEq, Eq, derive_more::From
);

impl BackendTag {
    /// Create a new tag with event of type P -> [], that is a function that takes parameters `P`
    /// and has no return value.
    #[inline]
    pub fn new<P: Into<Box<[Type]>>>(store: &mut impl AsStoreMut, params: P) -> Self {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(_) => {
                Self::Sys(crate::backend::sys::tag::Tag::new(store, params))
            }
            #[cfg(feature = "wamr")]
            crate::BackendStore::Wamr(_) => {
                Self::Wamr(crate::backend::wamr::tag::Tag::new(store, params))
            }
            #[cfg(feature = "wasmi")]
            crate::BackendStore::Wasmi(_) => {
                Self::Wasmi(crate::backend::wasmi::tag::Tag::new(store, params))
            }
            #[cfg(feature = "v8")]
            crate::BackendStore::V8(_) => {
                Self::V8(crate::backend::v8::entities::tag::Tag::new(store, params))
            }
            #[cfg(feature = "js")]
            crate::BackendStore::Js(_) => {
                Self::Js(crate::backend::js::tag::Tag::new(store, params))
            }
            #[cfg(feature = "jsc")]
            crate::BackendStore::Jsc(_) => {
                Self::Jsc(crate::backend::jsc::tag::Tag::new(store, params))
            }
        }
    }

    /// Returns the [`TagType`] of the tag.
    #[inline]
    pub fn ty(&self, store: &impl AsStoreRef) -> TagType {
        match_rt!(on self => f {
            f.ty(store)
        })
    }

    #[inline]
    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, vm_extern: VMExternTag) -> Self {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(_) => Self::Sys(
                crate::backend::sys::tag::Tag::from_vm_extern(store, vm_extern),
            ),
            #[cfg(feature = "wamr")]
            crate::BackendStore::Wamr(_) => Self::Wamr(
                crate::backend::wamr::tag::Tag::from_vm_extern(store, vm_extern),
            ),
            #[cfg(feature = "wasmi")]
            crate::BackendStore::Wasmi(_) => Self::Wasmi(
                crate::backend::wasmi::tag::Tag::from_vm_extern(store, vm_extern),
            ),
            #[cfg(feature = "v8")]
            crate::BackendStore::V8(_) => Self::V8(
                crate::backend::v8::entities::tag::Tag::from_vm_extern(store, vm_extern),
            ),
            #[cfg(feature = "js")]
            crate::BackendStore::Js(_) => Self::Js(
                crate::backend::js::entities::tag::Tag::from_vm_extern(store, vm_extern),
            ),
            #[cfg(feature = "jsc")]
            crate::BackendStore::Jsc(_) => Self::Jsc(
                crate::backend::jsc::entities::tag::Tag::from_vm_extern(store, vm_extern),
            ),
        }
    }

    /// Checks whether this tag can be used with the given context.
    #[inline]
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        match_rt!(on self => f {
            f.is_from_store(store)
        })
    }

    /// Create a [`VMExtern`] from self.
    #[inline]
    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        match_rt!(on self => f {
            f.to_vm_extern()
        })
    }
}

impl<'a> Exportable<'a> for BackendTag {
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Tag(func) => Ok(&func.0),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
