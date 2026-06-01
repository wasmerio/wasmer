use crate::{AsStoreMut, macros::backend::match_rt};

use super::*;

impl VMExternToExtern for VMExtern {
    fn to_extern(self, store: &mut impl crate::AsStoreMut) -> crate::Extern {
        match_rt!(on self => s {
            s.to_extern(store)
        })
    }
}

impl VMFunctionEnvironment {
    #[allow(clippy::should_implement_trait)]
    /// Returns a reference to the underlying value.
    pub fn as_ref(&self) -> &(dyn std::any::Any + Send + 'static) {
        match_rt!(on self => s {
            s.as_ref()
        })
    }

    #[allow(clippy::should_implement_trait)]
    /// Returns a mutable reference to the underlying value.
    pub fn as_mut(&mut self) -> &mut (dyn std::any::Any + Send + 'static) {
        match_rt!(on self => s {
            s.as_mut()
        })
    }

    pub fn contents(self) -> Box<(dyn std::any::Any + Send + 'static)> {
        match_rt!(on self => s {
            s.contents
        })
    }
}

impl VMFuncRef {
    /// Converts the `VMFuncRef` into a `RawValue`.
    pub fn into_raw(self) -> RawValue {
        match_rt!(on self => s {
            s.into_raw()
        })
    }
}

impl VMExternRef {
    /// Converts the `VMExternRef` into a `RawValue`.
    pub fn into_raw(self) -> RawValue {
        match_rt!(on self => s {
            s.into_raw()
        })
    }
}

impl VMMemory {
    /// Attempts to share this memory and return a shared detached memory.
    pub(crate) fn as_shared(&self) -> Result<VMSharedMemory, wasmer_types::MemoryError> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.0.as_shared().map(VMSharedMemory::Sys),
            #[cfg(feature = "v8")]
            Self::V8(s) => s.as_shared().map(VMSharedMemory::V8),
            #[cfg(feature = "js")]
            Self::Js(s) => s.try_clone().map(VMSharedMemory::Js),
        }
    }
}

impl VMSharedMemory {
    /// Clones this shared memory handle.
    pub(crate) fn clone(&self) -> Self {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => Self::Sys(s.clone()),
            #[cfg(feature = "v8")]
            Self::V8(s) => Self::V8(s.clone()),
            #[cfg(feature = "js")]
            Self::Js(s) => Self::Js(
                s.try_clone()
                    .expect("cloning JavaScript shared memory should not fail"),
            ),
        }
    }

    pub(crate) fn into_vm_memory(self, store: &mut impl AsStoreMut) -> VMMemory {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => VMMemory::Sys(s.into()),
            #[cfg(feature = "v8")]
            Self::V8(s) => {
                let mut store = store.as_store_mut();
                VMMemory::V8(s.into_vm_memory(store.inner.store.as_v8_mut()))
            }
            #[cfg(feature = "js")]
            Self::Js(s) => VMMemory::Js(s),
        }
    }
}

impl VMExceptionRef {
    /// Converts the `VMExternRef` into a `RawValue`.
    pub fn into_raw(self) -> RawValue {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.into_raw(),

            _ => unimplemented!("VMExceptionRef::into_raw is only implemented for the sys backend"),
        }
    }
}
