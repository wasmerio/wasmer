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

    /// Grows this memory by `delta` pages, returning the previous size.
    ///
    /// Only the `sys` backend can grow a shared memory without a store; the
    /// others report [`MemoryError::UnsupportedOperation`].
    pub(crate) fn grow(
        &self,
        delta: wasmer_types::Pages,
    ) -> Result<wasmer_types::Pages, wasmer_types::MemoryError> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => {
                use wasmer_vm::LinearMemory;
                // `LinearMemory::grow` wants `&mut self`, but a shared memory
                // is an `Arc<RwLock<..>>` internally, so every clone addresses
                // the same allocation and growing through one is growing them
                // all. Taking a clone here is what lets the public API keep a
                // `&self` receiver, which is the honest signature for a handle
                // several threads may hold at once.
                let mut memory = s.clone();
                memory.grow(delta)
            }
            #[cfg(feature = "v8")]
            Self::V8(_) => Err(wasmer_types::MemoryError::UnsupportedOperation {
                message: "growing a shared memory without a store is not supported by the v8 \
                          backend"
                    .into(),
            }),
            #[cfg(feature = "js")]
            Self::Js(_) => Err(wasmer_types::MemoryError::UnsupportedOperation {
                message: "growing a shared memory without a store is not supported by the js \
                          backend"
                    .into(),
            }),
        }
    }

    /// The host address of guest offset 0, or `None` on backends that cannot
    /// report it without a store.
    pub(crate) fn data_ptr(&self) -> Option<*mut u8> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => {
                use wasmer_vm::LinearMemory;
                // SAFETY: the definition pointer is valid for as long as the
                // memory is, and this handle keeps it alive.
                Some(unsafe { s.vmmemory().as_ref().base })
            }
            #[cfg(feature = "v8")]
            Self::V8(_) => None,
            #[cfg(feature = "js")]
            Self::Js(_) => None,
        }
    }

    /// How this memory's host mapping is laid out, or `None` on backends that
    /// do not model a style.
    pub(crate) fn style(&self) -> Option<wasmer_types::MemoryStyle> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => {
                use wasmer_vm::LinearMemory;
                Some(s.style())
            }
            #[cfg(feature = "v8")]
            Self::V8(_) => None,
            #[cfg(feature = "js")]
            Self::Js(_) => None,
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
