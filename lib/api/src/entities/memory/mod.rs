use shared::SharedMemory;
use view::MemoryView;
use wasmer_types::{MemoryType, Pages};
use wasmer_vm::{LinearMemory, MemoryError};

use crate::{
    vm::{VMExtern, VMExternMemory, VMMemory},
    AsStoreMut, AsStoreRef, ExportError, Exportable, Extern, StoreMut, StoreRef,
};

pub(crate) mod buffer;
pub(crate) mod location;
pub(crate) mod shared;
pub(crate) mod view;

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
#[derive(Debug)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct Memory(pub(crate) Box<dyn MemoryLike>);

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
        Ok(Self(store.as_store_mut().memory_new(ty)?))
    }

    /// Create a memory object from an existing memory and attaches it to the store
    pub fn new_from_existing(new_store: &mut impl AsStoreMut, memory: VMMemory) -> Self {
        Self(new_store.as_store_mut().memory_from_existing(memory))
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
        self.0.ty(store.as_store_ref())
    }

    /// Creates a view into the memory that then allows for
    /// read and write
    pub fn view(&self, store: &(impl AsStoreRef + ?Sized)) -> MemoryView {
        MemoryView::new(self, store)
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
        self.0.grow(store.as_store_mut(), delta.into())
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
        self.0.grow_at_least(store.as_store_mut(), min_size)
    }

    /// Resets the memory back to zero length
    pub fn reset(&self, store: &mut impl AsStoreMut) -> Result<(), MemoryError> {
        self.0.reset(store.as_store_mut())?;
        Ok(())
    }

    /// Attempts to duplicate this memory (if its clonable) in a new store
    /// (copied memory)
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
        self.0
            .try_copy(store.as_store_ref())
            .map(|new_memory| Self::new_from_existing(new_store, new_memory.into()))
    }

    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, vm_extern: VMExternMemory) -> Self {
        Self(store.as_store_mut().memory_from_vm_extern(vm_extern))
    }

    /// Checks whether this `Memory` can be used with the given context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.0.is_from_store(store.as_store_ref())
    }

    /// Attempt to create a new reference to the underlying memory; this new reference can then be
    /// used within a different store (from the same implementer).
    ///
    /// # Errors
    ///
    /// Fails if the underlying memory is not clonable.
    pub fn try_clone(&self, store: &impl AsStoreRef) -> Result<VMMemory, MemoryError> {
        self.0.try_clone(store.as_store_ref())
    }

    /// Attempts to clone this memory (if its clonable) in a new store
    /// (cloned memory will be shared between those that clone it)
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
        self.0
            .try_clone(store.as_store_ref())
            .map(|new_memory| Self::new_from_existing(new_store, new_memory))
    }

    /// Get a [`SharedMemory`].
    ///
    /// Only returns `Some(_)` if the memory is shared, and if the target
    /// backend supports shared memory operations.
    ///
    /// See [`SharedMemory`] and its methods for more information.
    pub fn as_shared(&self, store: &impl AsStoreRef) -> Option<SharedMemory> {
        if !self.ty(store).shared {
            return None;
        }
        self.0.as_shared(store.as_store_ref())
    }

    /// Create a [`VMExtern`] from self.
    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        self.0.to_vm_extern()
    }
}

impl std::cmp::PartialEq for Memory {
    fn eq(&self, other: &Self) -> bool {
        todo!()
    }
}

impl std::cmp::Eq for Memory {}

impl Clone for Memory {
    fn clone(&self) -> Self {
        Self(self.0.clone_box())
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

/// The trait that every concrete memory must implement.
pub trait MemoryLike: std::fmt::Debug {
    /// Returns the [`MemoryType`] of the `Memory`.
    fn ty(&self, store: StoreRef) -> MemoryType;

    /// Grow memory by the specified amount of WebAssembly [`Pages`] and return
    /// the previous memory size.
    fn grow(&self, store: StoreMut, delta: Pages) -> Result<Pages, MemoryError>;

    /// Grows the memory to at least a minimum size.
    ///
    /// # Note
    ///
    /// If the memory is already big enough for the min size this function does nothing.
    fn grow_at_least(&self, store: StoreMut, min_size: u64) -> Result<(), MemoryError>;

    /// Create a [`VMExtern`] from self.
    fn to_vm_extern(&self) -> VMExtern;

    /// Get a [`SharedMemory`].
    ///
    /// Only returns `Some(_)` if the memory is shared, and if the target
    /// backend supports shared memory operations.
    ///
    /// See [`SharedMemory`] and its methods for more information.
    fn as_shared(&self, store: StoreRef) -> Option<SharedMemory>;

    /// Attempt to create a new reference to the underlying memory; this new reference can then be
    /// used within a different store (from the same implementer).
    ///
    /// # Errors
    ///
    /// Fails if the underlying memory is not clonable.
    fn try_clone(&self, store: StoreRef) -> Result<VMMemory, MemoryError>;

    /// Attempt to create a new deep copy of the underlying memory; this new reference can then be
    /// used within a different store (from the same implementer).
    ///
    /// # Errors
    ///
    /// Fails if the underlying memory is not clonable.
    fn try_copy(&self, store: StoreRef) -> Result<VMMemory, MemoryError>;

    /// Check whether this memory can be used with the given context.
    fn is_from_store(&self, store: StoreRef) -> bool;

    /// Reset the memory by deleting its contents and resetting its size to zero.
    fn reset(&self, store: StoreMut) -> Result<(), MemoryError>;

    /// Create a boxed clone of this implementer.
    fn clone_box(&self) -> Box<dyn MemoryLike>;
}

/// The trait implemented by all those that can create new memories.
pub trait MemoryCreator {
    /// Create a new [`MemoryLike`] from the provided [`MemoryType`].
    fn memory_new(&mut self, ty: MemoryType) -> Result<Box<dyn MemoryLike>, MemoryError>;

    /// Create a new memory from an existing one.
    fn memory_from_existing(&mut self, memory: VMMemory) -> Box<dyn MemoryLike>;

    /// Create a new memory from a [`VMExternMemory`]
    fn memory_from_vm_extern(&mut self, vm_extern: VMExternMemory) -> Box<dyn MemoryLike>;
}
