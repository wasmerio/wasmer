use crate::vm::VMExceptionRef;
use crate::{
    macros::backend::{gen_rt_ty, match_rt},
    vm::{VMExtern, VMExternTag},
    AsStoreMut, AsStoreRef, ExportError, Exportable, Extern, Tag, Value,
};

/// A WebAssembly `global` instance.
///
/// A global instance is the runtime representation of a global variable.
/// It consists of an individual value and a flag indicating whether it is mutable.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#global-instances>
gen_rt_ty!(Exception
    @cfg feature = "artifact-size" => derive(loupe::MemoryUsage)
    @derives Debug, Clone, PartialEq, Eq, derive_more::From
);

impl BackendException {
    /// Create a new exception with the given tag type and payload.
    #[inline]
    pub fn new(store: &mut impl AsStoreMut, tag: &Tag, payload: &[Value]) -> Self {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(_) => Self::Sys(
                crate::backend::sys::exception::Exception::new(store, tag, payload),
            ),
            _ => unimplemented!("new is only implemented for the sys backend"),
        }
    }

    /// Checks whether this `Exception` can be used with the given store.
    #[inline]
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.is_from_store(store),
            _ => unimplemented!("is_from_store is only implemented for the sys backend"),
        }
    }

    /// Gets the exception tag.
    #[inline]
    pub fn tag(&self, store: &impl AsStoreRef) -> Tag {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.tag(store),
            _ => unimplemented!("tag is only implemented for the sys backend"),
        }
    }

    /// Gets the exception payload values.
    #[inline]
    pub fn payload(&self, store: &mut impl AsStoreMut) -> Vec<Value> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.payload(store),
            _ => unimplemented!("payload is only implemented for the sys backend"),
        }
    }

    /// Get the `VMExceptionRef` corresponding to this `Exception`.
    #[inline]
    pub fn vm_exceptionref(&self) -> VMExceptionRef {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => VMExceptionRef::Sys(s.exnref()),
            _ => unimplemented!("vm_exceptionref is only implemented for the sys backend"),
        }
    }

    /// Creates a new `Exception` from a `VMExceptionRef`.
    #[inline]
    pub fn from_vm_exceptionref(exnref: VMExceptionRef) -> Self {
        match exnref {
            #[cfg(feature = "sys")]
            VMExceptionRef::Sys(s) => {
                Self::Sys(crate::backend::sys::exception::Exception::from_exnref(s))
            }
            _ => unimplemented!("from_vm_exceptionref is only implemented for the sys backend"),
        }
    }
}
