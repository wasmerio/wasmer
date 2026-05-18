pub use owned::OwnedMemory;
pub use shared::SharedMemory;
use wasmer_types::{MemoryError, MemoryType, Pages};

use crate::{
    AsStoreMut, AsStoreRef, ExportError, Exportable, Extern, StoreMut, StoreRef,
    vm::{VMExtern, VMExternMemory, VMMemory},
};

pub(crate) mod buffer;
pub(crate) mod inner;
pub(crate) mod location;
pub(crate) mod owned;
pub(crate) mod shared;
pub(crate) mod view;

pub(crate) use inner::*;
pub use view::*;

#[inline]
pub(crate) fn shared_memory_detach_error() -> MemoryError {
    MemoryError::Generic(
        "could not detach shared WebAssembly memory for use outside the store: duplicating the \
         backing handle failed, synchronization support may be unavailable, or the backend does \
         not support exposing this shared memory independently of its store"
            .into(),
    )
}

/// A detached memory handle that is either an owned non-shared
/// memory or a shared memory reference.
pub enum OwnedOrSharedMemory {
    /// A shared memory reference.
    Shared(SharedMemory),
    /// A detached owned memory.
    Owned(OwnedMemory),
}

impl OwnedOrSharedMemory {
    /// Attach this memory handle to the provided store.
    pub fn attach(self, store: &mut impl AsStoreMut) -> Memory {
        match self {
            OwnedOrSharedMemory::Shared(shared) => shared.attach(store),
            OwnedOrSharedMemory::Owned(owned) => owned.attach(store),
        }
    }
}

/// A WebAssembly `memory` instance.
///
/// A memory instance is the runtime representation of a linear memory.
/// It consists of a vector of bytes and an optional maximum size.
///
/// The length of the vector always is a multiple of the WebAssembly
/// page size, which is defined to be the constant 65536 – abbreviated 64Ki.
/// Like in a memory type, the maximum size in a memory instance is
/// given in units of this page size.
///
/// A memory created by the host or in WebAssembly code will be accessible and
/// mutable from both host and WebAssembly.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#memory-instances>
#[derive(Debug, Clone, PartialEq, Eq, derive_more::From)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct Memory(pub(crate) BackendMemory);

impl Memory {
    /// Creates a new host [`Memory`] from the provided [`MemoryType`].
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
    pub fn new(store: &mut impl AsStoreMut, ty: MemoryType) -> Result<Self, MemoryError> {
        BackendMemory::new(store, ty).map(Self)
    }

    /// Create a memory object from an existing memory and attaches it to the store
    pub fn new_from_existing<IntoVMMemory>(
        new_store: &mut impl AsStoreMut,
        memory: IntoVMMemory,
    ) -> Self
    where
        IntoVMMemory: Into<VMMemory>,
    {
        Self(BackendMemory::new_from_existing(new_store, memory.into()))
    }

    /// Returns the [`MemoryType`] of the `Memory`.
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
    pub fn ty(&self, store: &impl AsStoreRef) -> MemoryType {
        self.0.ty(store)
    }

    /// Creates a view into the memory that then allows for
    /// read and write
    pub fn view<'a>(&self, store: &'a (impl AsStoreRef + ?Sized)) -> MemoryView<'a> {
        MemoryView::new(self, store)
    }

    /// Retrieve the size of the memory in pages.
    pub fn size(&self, store: &impl AsStoreRef) -> Pages {
        self.0.size(store)
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
    pub fn grow<IntoPages>(
        &self,
        store: &mut impl AsStoreMut,
        delta: IntoPages,
    ) -> Result<Pages, MemoryError>
    where
        IntoPages: Into<Pages>,
    {
        self.0.grow(store, delta)
    }

    /// Grows the memory to at least a minimum size.
    ///
    /// # Note
    ///
    /// If the memory is already big enough for the min size this function does nothing.
    pub fn grow_at_least(
        &self,
        store: &mut impl AsStoreMut,
        min_size: u64,
    ) -> Result<(), MemoryError> {
        self.0.grow_at_least(store, min_size)
    }

    /// Resets the memory back to zero length
    pub fn reset(&self, store: &mut impl AsStoreMut) -> Result<(), MemoryError> {
        self.0.reset(store)
    }

    /// Attempts to duplicate this memory in a new store with a byte-for-byte copy
    ///
    /// Since Wasmer 8.0, this function can no longer be used for stores
    /// in different threads; for that, use `copy`, and then `attach`
    /// on the thread owning the other `Store`.
    pub fn copy_to_store(
        &self,
        store: &impl AsStoreRef,
        new_store: &mut impl AsStoreMut,
    ) -> Result<Self, MemoryError> {
        self.copy(store).map(|memory| memory.attach(new_store))
    }

    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, vm_extern: VMExternMemory) -> Self {
        Self(BackendMemory::from_vm_extern(store, vm_extern))
    }

    /// Checks whether this `Memory` can be used with the given context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.0.is_from_store(store)
    }

    /// Attempts to create a detached copied memory handle that can later be
    /// attached to a different store.
    ///
    /// If the memory is shared, this returns a shared handle. Otherwise, it
    /// creates an independent byte-for-byte copy.
    pub fn copy(&self, store: &impl AsStoreRef) -> Result<OwnedOrSharedMemory, MemoryError> {
        self.0.copy(store)
    }

    /// Attempts to clone this memory (if its cloneable) in a new store
    /// (cloned memory will be shared between those that clone it)
    ///
    /// Since Wasmer 8.0, this function can no longer be used for stores
    /// in different threads; for that, use `as_shared`, and then `attach`
    /// on the thread owning the other `Store`.
    pub fn share_in_store(
        &self,
        store: &impl AsStoreRef,
        new_store: &mut impl AsStoreMut,
    ) -> Result<Self, MemoryError> {
        if !self.ty(store).shared {
            return Err(MemoryError::MemoryNotShared);
        }

        self.as_shared(store)
            .ok_or_else(shared_memory_detach_error)
            .map(|memory| memory.attach(new_store))
    }

    /// Get a [`SharedMemory`].
    ///
    /// Only returns `Some(_)` if the memory is shared, and if the target
    /// backend supports shared memory operations.
    ///
    /// See [`SharedMemory`] and its methods for more information.
    pub fn as_shared(&self, store: &impl AsStoreRef) -> Option<SharedMemory> {
        self.0.as_shared(store)
    }

    /// Create a [`VMExtern`] from self.
    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        self.0.to_vm_extern()
    }
}

impl<'a> Exportable<'a> for Memory {
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Memory(memory) => Ok(memory),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
