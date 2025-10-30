use crate::vm::VMContinuationRef;
use crate::{
    AsStoreMut, AsStoreRef, BackendTag, ExportError, Exportable, Extern, Tag, Value,
    macros::backend::{gen_rt_ty, match_rt},
    vm::{VMExtern, VMExternTag},
};

/// A WebAssembly `global` instance.
///
/// A global instance is the runtime representation of a global variable.
/// It consists of an individual value and a flag indicating whether it is mutable.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#global-instances>
gen_rt_ty!(Continuation
    @cfg feature = "artifact-size" => derive(loupe::MemoryUsage)
    @derives Debug, Clone, PartialEq, Eq, derive_more::From
);

impl BackendContinuation {
    /// Create a new continuation with the given tag type and payload.
    #[inline]
    #[allow(irrefutable_let_patterns)]
    pub fn new(store: &mut impl AsStoreMut, tag: &Tag, payload: &[Value]) -> Self {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(_) => {
                let BackendTag::Sys(tag) = &tag.0 else {
                    panic!("cannot create Continuation with Tag from another backend");
                };

                Self::Sys(crate::backend::sys::continuation::Continuation::new(
                    store, tag, payload,
                ))
            }
            _ => unimplemented!("new is only implemented for the sys backend"),
        }
    }

    /// Checks whether this `Continuation` can be used with the given store.
    #[inline]
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.is_from_store(store),
            _ => unimplemented!("is_from_store is only implemented for the sys backend"),
        }
    }

    /// Gets the continuation tag.
    #[inline]
    pub fn tag(&self, store: &impl AsStoreRef) -> Tag {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => Tag(BackendTag::Sys(s.tag(store))),
            _ => unimplemented!("tag is only implemented for the sys backend"),
        }
    }

    /// Gets the continuation payload values.
    #[inline]
    pub fn payload(&self, store: &mut impl AsStoreMut) -> Vec<Value> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.payload(store),
            _ => unimplemented!("payload is only implemented for the sys backend"),
        }
    }

    /// Get the `VMContinuationRef` corresponding to this `Continuation`.
    #[inline]
    pub fn vm_continuationref(&self) -> VMContinuationRef {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => VMContinuationRef::Sys(s.exnref()),
            _ => unimplemented!("vm_continuationref is only implemented for the sys backend"),
        }
    }

    /// Creates a new `Continuation` from a `VMContinuationRef`.
    #[inline]
    pub fn from_vm_continuationref(exnref: VMContinuationRef) -> Self {
        match exnref {
            #[cfg(feature = "sys")]
            VMContinuationRef::Sys(s) => {
                Self::Sys(crate::backend::sys::continuation::Continuation::from_exnref(s))
            }
            _ => unimplemented!("from_vm_continuationref is only implemented for the sys backend"),
        }
    }
}
