pub use shared::SharedMemory;
use wasmer_types::{MemoryError, MemoryType, Pages};

use crate::{
    AsStoreMut, AsStoreRef, ExportError, Exportable, Extern, StoreMut, StoreRef,
    vm::{VMExtern, VMExternMemory, VMMemory},
};

pub(crate) mod buffer;
pub(crate) mod inner;
pub(crate) mod location;
pub(crate) mod shared;
pub(crate) mod view;

pub(crate) use inner::*;
pub use view::*;

/// A memory handle that has been shallow-cloned or copied from an existing memory but
/// has not yet been attached to a store.
#[derive(Debug)]
pub struct DetachedMemory {
    kind: DetachedMemoryKind,
    memory: VMMemory,
}

// The whole point of DetachedMemory is to be thread-safe, and indeed it is
// in every backend we support.
unsafe impl Send for DetachedMemory {}
unsafe impl Sync for DetachedMemory {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetachedMemoryKind {
    Shared,
    Copied,
}

impl DetachedMemory {
    pub(crate) fn from_shared_vm_memory(memory: VMMemory) -> Self {
        Self {
            kind: DetachedMemoryKind::Shared,
            memory,
        }
    }

    pub(crate) fn from_copied_vm_memory(memory: VMMemory) -> Self {
        Self {
            kind: DetachedMemoryKind::Copied,
            memory,
        }
    }

    /// Returns true if this detached memory will be attached as a shared
    /// memory reference.
    pub fn is_shared(&self) -> bool {
        self.kind == DetachedMemoryKind::Shared
    }

    /// Returns true if this detached memory will be attached as a copied
    /// memory.
    pub fn is_copied(&self) -> bool {
        self.kind == DetachedMemoryKind::Copied
    }

    /// Attaches this detached memory to the provided store.
    pub fn attach(self, new_store: &mut impl AsStoreMut) -> Memory {
        Memory::new_from_existing(new_store, self.memory)
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
    #[deprecated(
        since = "8.0.0",
        note = "Since `Store` is no longer `Send + Sync`, this method cannot be used meaningfully. \
                Use `copy_and_detach`, then `attach` on the thread owning the other `Store` instead."
    )]
    pub fn copy_to_store(
        &self,
        store: &impl AsStoreRef,
        new_store: &mut impl AsStoreMut,
    ) -> Result<Self, MemoryError> {
        self.copy_and_detach(store)
            .map(|memory| memory.attach(new_store))
    }

    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, vm_extern: VMExternMemory) -> Self {
        Self(BackendMemory::from_vm_extern(store, vm_extern))
    }

    /// Checks whether this `Memory` can be used with the given context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.0.is_from_store(store)
    }

    /// Attempt to create a new reference to the underlying memory; this new reference can then be
    /// used within a different store (from the same implementer).
    ///
    /// # Errors
    ///
    /// Fails if the underlying memory is not cloneable.
    pub fn try_clone(&self, store: &impl AsStoreRef) -> Result<VMMemory, MemoryError> {
        self.0.try_clone(store)
    }

    /// Attempts to create a detached shared memory handle that can
    /// later be attached to a different store.
    pub fn share_and_detach(&self, store: &impl AsStoreRef) -> Result<DetachedMemory, MemoryError> {
        self.0
            .share_and_detach(store)
            .map(DetachedMemory::from_shared_vm_memory)
    }

    /// Attempts to create a detached copied memory handle that can later be
    /// attached to a different store.
    ///
    /// Unlike [`Memory::share_and_detach`], this operation is not limited to
    /// shared memories because it creates an independent copy.
    pub fn copy_and_detach(&self, store: &impl AsStoreRef) -> Result<DetachedMemory, MemoryError> {
        self.0
            .copy_and_detach(store)
            .map(DetachedMemory::from_copied_vm_memory)
    }

    /// Attempts to clone this memory (if its cloneable) in a new store
    /// (cloned memory will be shared between those that clone it)
    #[deprecated(
        since = "8.0.0",
        note = "Since `Store` is no longer `Send + Sync`, this method cannot be used meaningfully. \
                Use `share_and_detach`, then `attach` on the thread owning the other `Store` instead."
    )]
    pub fn share_in_store(
        &self,
        store: &impl AsStoreRef,
        new_store: &mut impl AsStoreMut,
    ) -> Result<Self, MemoryError> {
        self.share_and_detach(store)
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
