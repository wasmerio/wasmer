use super::{shared::SharedMemory, view::*};
use wasmer_types::{MemoryError, MemoryType, Pages};

use crate::{
    macros::backend::{gen_rt_ty, match_rt},
    vm::{VMExtern, VMExternMemory, VMMemory},
    AsStoreMut, AsStoreRef, ExportError, Exportable, Extern, StoreMut, StoreRef,
};

gen_rt_ty!(Memory
    @cfg feature = "artifact-size" => derive(loupe::MemoryUsage)
    @derives Debug, Clone, PartialEq, Eq, derive_more::From
);

impl BackendMemory {
    /// Creates a new host [`BackendMemory`] from the provided [`MemoryType`].
    ///
    /// This function will construct the `Memory` using the store
    /// `BaseTunables`.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Memory, MemoryType, Pages, Store, Type, Value};
    /// # let mut store = Store::default();
    /// #
    /// let m = Memory::new(&mut store, MemoryType::new(1, None, false)).unwrap();
    /// ```
    #[inline]
    pub fn new(store: &mut impl AsStoreMut, ty: MemoryType) -> Result<Self, MemoryError> {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(s) => Ok(Self::Sys(
                crate::backend::sys::entities::memory::Memory::new(store, ty)?,
            )),
            #[cfg(feature = "wamr")]
            crate::BackendStore::Wamr(s) => Ok(Self::Wamr(
                crate::backend::wamr::entities::memory::Memory::new(store, ty)?,
            )),
            #[cfg(feature = "wasmi")]
            crate::BackendStore::Wasmi(s) => Ok(Self::Wasmi(
                crate::backend::wasmi::entities::memory::Memory::new(store, ty)?,
            )),
            #[cfg(feature = "v8")]
            crate::BackendStore::V8(s) => Ok(Self::V8(
                crate::backend::v8::entities::memory::Memory::new(store, ty)?,
            )),
            #[cfg(feature = "js")]
            crate::BackendStore::Js(s) => Ok(Self::Js(
                crate::backend::js::entities::memory::Memory::new(store, ty)?,
            )),
            #[cfg(feature = "jsc")]
            crate::BackendStore::Jsc(s) => Ok(Self::Jsc(
                crate::backend::jsc::entities::memory::Memory::new(store, ty)?,
            )),
        }
    }

    /// Create a memory object from an existing memory and attaches it to the store
    #[inline]
    pub fn new_from_existing(new_store: &mut impl AsStoreMut, memory: VMMemory) -> Self {
        match new_store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(_) => Self::Sys(
                crate::backend::sys::entities::memory::Memory::new_from_existing(
                    new_store,
                    memory.into_sys(),
                ),
            ),
            #[cfg(feature = "wamr")]
            crate::BackendStore::Wamr(_) => Self::Wamr(
                crate::backend::wamr::entities::memory::Memory::new_from_existing(
                    new_store,
                    memory.into_wamr(),
                ),
            ),
            #[cfg(feature = "wasmi")]
            crate::BackendStore::Wasmi(_) => Self::Wasmi(
                crate::backend::wasmi::entities::memory::Memory::new_from_existing(
                    new_store,
                    memory.into_wasmi(),
                ),
            ),
            #[cfg(feature = "v8")]
            crate::BackendStore::V8(_) => Self::V8(
                crate::backend::v8::entities::memory::Memory::new_from_existing(
                    new_store,
                    memory.into_v8(),
                ),
            ),
            #[cfg(feature = "js")]
            crate::BackendStore::Js(_) => Self::Js(
                crate::backend::js::entities::memory::Memory::new_from_existing(
                    new_store,
                    memory.into_js(),
                ),
            ),
            #[cfg(feature = "jsc")]
            crate::BackendStore::Jsc(_) => Self::Jsc(
                crate::backend::jsc::entities::memory::Memory::new_from_existing(
                    new_store,
                    memory.into_jsc(),
                ),
            ),
        }
    }

    /// Returns the [`MemoryType`] of the [`BackendMemory`].
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Memory, MemoryType, Pages, Store, Type, Value};
    /// # let mut store = Store::default();
    /// #
    /// let mt = MemoryType::new(1, None, false);
    /// let m = Memory::new(&mut store, mt).unwrap();
    ///
    /// assert_eq!(m.ty(&mut store), mt);
    /// ```
    #[inline]
    pub fn ty(&self, store: &impl AsStoreRef) -> MemoryType {
        match_rt!(on self => s {
            s.ty(store)
        })
    }

    /// Grow memory by the specified amount of WebAssembly [`Pages`] and return
    /// the previous memory size.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Memory, MemoryType, Pages, Store, Type, Value, WASM_MAX_PAGES};
    /// # let mut store = Store::default();
    /// #
    /// let m = Memory::new(&mut store, MemoryType::new(1, Some(3), false)).unwrap();
    /// let p = m.grow(&mut store, 2).unwrap();
    ///
    /// assert_eq!(p, Pages(1));
    /// assert_eq!(m.view(&mut store).size(), Pages(3));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if memory can't be grown by the specified amount
    /// of pages.
    ///
    /// ```should_panic
    /// # use wasmer::{Memory, MemoryType, Pages, Store, Type, Value, WASM_MAX_PAGES};
    /// # use wasmer::FunctionEnv;
    /// # let mut store = Store::default();
    /// # let env = FunctionEnv::new(&mut store, ());
    /// #
    /// let m = Memory::new(&mut store, MemoryType::new(1, Some(1), false)).unwrap();
    ///
    /// // This results in an error: `MemoryError::CouldNotGrow`.
    /// let s = m.grow(&mut store, 1).unwrap();
    /// ```
    #[inline]
    pub fn grow<IntoPages>(
        &self,
        store: &mut impl AsStoreMut,
        delta: IntoPages,
    ) -> Result<Pages, MemoryError>
    where
        IntoPages: Into<Pages>,
    {
        match_rt!(on self => s {
            s.grow(store, delta)
        })
    }

    /// Grows the memory to at least a minimum size.
    ///
    /// # Note
    ///
    /// If the memory is already big enough for the min size this function does nothing.
    #[inline]
    pub fn grow_at_least(
        &self,
        store: &mut impl AsStoreMut,
        min_size: u64,
    ) -> Result<(), MemoryError> {
        match_rt!(on self => s {
            s.grow_at_least(store, min_size)
        })
    }

    /// Resets the memory back to zero length
    #[inline]
    pub fn reset(&self, store: &mut impl AsStoreMut) -> Result<(), MemoryError> {
        match_rt!(on self => s {
            s.reset(store)
        })
    }

    /// Attempts to duplicate this memory (if its clonable) in a new store
    /// (copied memory)
    #[inline]
    pub fn copy_to_store(
        &self,
        store: &impl AsStoreRef,
        new_store: &mut impl AsStoreMut,
    ) -> Result<Self, MemoryError> {
        if !self.ty(store).shared {
            // We should only be able to duplicate in a new store if the memory is shared
            return Err(MemoryError::InvalidMemory {
                reason: "memory is not a shared memory type".to_string(),
            });
        }

        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.try_copy(store).map(|new_memory| {
                Self::new_from_existing(
                    new_store,
                    VMMemory::Sys(crate::backend::sys::vm::VMMemory(new_memory)),
                )
            }),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s
                .try_copy(store)
                .map(|new_memory| Self::new_from_existing(new_store, VMMemory::Wamr(new_memory))),
            #[cfg(feature = "wasmi")]
            Self::Wasmi(s) => s
                .try_copy(store)
                .map(|new_memory| Self::new_from_existing(new_store, VMMemory::Wasmi(new_memory))),

            #[cfg(feature = "v8")]
            Self::V8(s) => s
                .try_copy(store)
                .map(|new_memory| Self::new_from_existing(new_store, VMMemory::V8(new_memory))),
            #[cfg(feature = "js")]
            Self::Js(s) => s
                .try_copy(store)
                .map(|new_memory| Self::new_from_existing(new_store, VMMemory::Js(new_memory))),
            #[cfg(feature = "jsc")]
            Self::Jsc(s) => s
                .try_copy(store)
                .map(|new_memory| Self::new_from_existing(new_store, VMMemory::Jsc(new_memory))),
        }
    }

    #[inline]
    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, vm_extern: VMExternMemory) -> Self {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(s) => Self::Sys(
                crate::backend::sys::entities::memory::Memory::from_vm_extern(store, vm_extern),
            ),
            #[cfg(feature = "wamr")]
            crate::BackendStore::Wamr(s) => Self::Wamr(
                crate::backend::wamr::entities::memory::Memory::from_vm_extern(store, vm_extern),
            ),
            #[cfg(feature = "wasmi")]
            crate::BackendStore::Wasmi(s) => Self::Wasmi(
                crate::backend::wasmi::entities::memory::Memory::from_vm_extern(store, vm_extern),
            ),
            #[cfg(feature = "v8")]
            crate::BackendStore::V8(s) => Self::V8(
                crate::backend::v8::entities::memory::Memory::from_vm_extern(store, vm_extern),
            ),
            #[cfg(feature = "js")]
            crate::BackendStore::Js(s) => Self::Js(
                crate::backend::js::entities::memory::Memory::from_vm_extern(store, vm_extern),
            ),
            #[cfg(feature = "jsc")]
            crate::BackendStore::Jsc(s) => Self::Jsc(
                crate::backend::jsc::entities::memory::Memory::from_vm_extern(store, vm_extern),
            ),
        }
    }

    /// Checks whether this `Memory` can be used with the given context.
    #[inline]
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        match_rt!(on self => s {
            s.is_from_store(store)
        })
    }

    /// Attempt to create a new reference to the underlying memory; this new reference can then be
    /// used within a different store (from the same implementer).
    ///
    /// # Errors
    ///
    /// Fails if the underlying memory is not clonable.
    #[inline]
    pub fn try_clone(&self, store: &impl AsStoreRef) -> Result<VMMemory, MemoryError> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.try_clone(store).map(VMMemory::Sys),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.try_clone(store).map(VMMemory::Wamr),
            #[cfg(feature = "wasmi")]
            Self::Wasmi(s) => s.try_clone(store).map(VMMemory::Wasmi),
            #[cfg(feature = "v8")]
            Self::V8(s) => s.try_clone(store).map(VMMemory::V8),
            #[cfg(feature = "js")]
            Self::Js(s) => s.try_clone(store).map(VMMemory::Js),
            #[cfg(feature = "jsc")]
            Self::Jsc(s) => s.try_clone(store).map(VMMemory::Jsc),
        }
    }

    /// Attempts to clone this memory (if its clonable) in a new store
    /// (cloned memory will be shared between those that clone it)
    #[inline]
    pub fn share_in_store(
        &self,
        store: &impl AsStoreRef,
        new_store: &mut impl AsStoreMut,
    ) -> Result<Self, MemoryError> {
        if !self.ty(store).shared {
            // We should only be able to duplicate in a new store if the memory is shared
            return Err(MemoryError::InvalidMemory {
                reason: "memory is not a shared memory type".to_string(),
            });
        }

        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s
                .try_clone(store)
                .map(|new_memory| Self::new_from_existing(new_store, VMMemory::Sys(new_memory))),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s
                .try_clone(store)
                .map(|new_memory| Self::new_from_existing(new_store, VMMemory::Wamr(new_memory))),
            #[cfg(feature = "wasmi")]
            Self::Wasmi(s) => s
                .try_clone(store)
                .map(|new_memory| Self::new_from_existing(new_store, VMMemory::Wasmi(new_memory))),
            #[cfg(feature = "v8")]
            Self::V8(s) => s
                .try_clone(store)
                .map(|new_memory| Self::new_from_existing(new_store, VMMemory::V8(new_memory))),
            #[cfg(feature = "js")]
            Self::Js(s) => s
                .try_clone(store)
                .map(|new_memory| Self::new_from_existing(new_store, VMMemory::Js(new_memory))),
            #[cfg(feature = "jsc")]
            Self::Jsc(s) => s
                .try_clone(store)
                .map(|new_memory| Self::new_from_existing(new_store, VMMemory::Jsc(new_memory))),
        }
    }

    /// Get a [`SharedMemory`].
    ///
    /// Only returns `Some(_)` if the memory is shared, and if the target
    /// backend supports shared memory operations.
    ///
    /// See [`SharedMemory`] and its methods for more information.
    #[inline]
    pub fn as_shared(&self, store: &impl AsStoreRef) -> Option<SharedMemory> {
        if !self.ty(store).shared {
            return None;
        }

        match_rt!(on self => s {
            s.as_shared(store)
        })
    }

    /// Create a [`VMExtern`] from self.
    #[inline]
    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        match_rt!(on self => s {
            s.to_vm_extern()
        })
    }
}
