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
    pub fn new(store: &mut impl AsStoreMut, tag: Tag, payload: &[Value]) -> Self {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(_) => Self::Sys(
                crate::backend::sys::exception::Exception::new(store, tag, payload),
            ),
            #[cfg(feature = "wamr")]
            crate::BackendStore::Wamr(_) => Self::Wamr(
                crate::backend::wamr::exception::Exception::new(store, tag, payload),
            ),
            #[cfg(feature = "wasmi")]
            crate::BackendStore::Wasmi(_) => Self::Wasmi(
                crate::backend::wasmi::exception::Exception::new(store, tag, payload),
            ),
            #[cfg(feature = "v8")]
            crate::BackendStore::V8(_) => Self::V8(crate::backend::v8::exception::Exception::new(
                store, tag, payload,
            )),
            #[cfg(feature = "js")]
            crate::BackendStore::Js(_) => Self::Js(crate::backend::js::exception::Exception::new(
                store, tag, payload,
            )),
            #[cfg(feature = "jsc")]
            crate::BackendStore::Jsc(_) => Self::Jsc(
                crate::backend::jsc::exception::Exception::new(store, tag, payload),
            ),
        }
    }

    /// Checks whether this `Exception` can be used with the given store.
    #[inline]
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        todo!()
    }
}
