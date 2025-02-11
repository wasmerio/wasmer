use wasmer_types::{TagType, Type};

use crate::{
    macros::rt::{gen_rt_ty, match_rt},
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

impl RuntimeTag {
    /// Create a new tag with event of type P -> [], that is a function that takes parameters `P`
    /// and has no return value.
    pub fn new<P: Into<Box<[Type]>>>(store: &mut impl AsStoreMut, params: P) -> Self {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::RuntimeStore::Sys(_) => Self::Sys(crate::rt::sys::tag::Tag::new(store, params)),
            #[cfg(feature = "wamr")]
            crate::RuntimeStore::Wamr(_) => {
                Self::Wamr(crate::rt::wamr::tag::Tag::new(store, params))
            }
            #[cfg(feature = "wasmi")]
            crate::RuntimeStore::Wasmi(_) => {
                Self::Wasmi(crate::rt::wasmi::tag::Tag::new(store, params))
            }
            #[cfg(feature = "v8")]
            crate::RuntimeStore::V8(_) => {
                Self::V8(crate::rt::v8::entities::tag::Tag::new(store, params))
            }
            #[cfg(feature = "js")]
            crate::RuntimeStore::Js(_) => Self::Js(crate::rt::js::tag::Tag::new(store, params)),
            #[cfg(feature = "jsc")]
            crate::RuntimeStore::Jsc(_) => Self::Jsc(crate::rt::jsc::tag::Tag::new(store, params)),
        }
    }

    /// Returns the [`TagType`] of the tag.
    pub fn ty(&self, store: &impl AsStoreRef) -> TagType {
        match_rt!(on self => f {
            f.ty(store)
        })
    }

    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, vm_extern: VMExternTag) -> Self {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::RuntimeStore::Sys(_) => {
                Self::Sys(crate::rt::sys::tag::Tag::from_vm_extern(store, vm_extern))
            }
            #[cfg(feature = "wamr")]
            crate::RuntimeStore::Wamr(_) => {
                Self::Wamr(crate::rt::wamr::tag::Tag::from_vm_extern(store, vm_extern))
            }
            #[cfg(feature = "wasmi")]
            crate::RuntimeStore::Wasmi(_) => {
                Self::Wasmi(crate::rt::wasmi::tag::Tag::from_vm_extern(store, vm_extern))
            }
            #[cfg(feature = "v8")]
            crate::RuntimeStore::V8(_) => Self::V8(
                crate::rt::v8::entities::tag::Tag::from_vm_extern(store, vm_extern),
            ),
            #[cfg(feature = "js")]
            crate::RuntimeStore::Js(_) => Self::Js(
                crate::rt::js::entities::tag::Tag::from_vm_extern(store, vm_extern),
            ),
            #[cfg(feature = "jsc")]
            crate::RuntimeStore::Jsc(_) => Self::Jsc(
                crate::rt::jsc::entities::tag::Tag::from_vm_extern(store, vm_extern),
            ),
        }
    }

    /// Checks whether this tag can be used with the given context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        match_rt!(on self => f {
            f.is_from_store(store)
        })
    }

    /// Create a [`VMExtern`] from self.
    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        match_rt!(on self => f {
            f.to_vm_extern()
        })
    }
}

impl<'a> Exportable<'a> for RuntimeTag {
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Tag(func) => Ok(&func.0),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
