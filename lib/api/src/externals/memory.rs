#[cfg(feature = "wasm-c-api")]
use crate::c_api::externals::memory as memory_impl;
#[cfg(feature = "js")]
use crate::js::externals::memory as memory_impl;
#[cfg(feature = "jsc")]
use crate::jsc::externals::memory as memory_impl;
#[cfg(feature = "sys")]
use crate::sys::externals::memory as memory_impl;

use super::memory_view::MemoryView;
use crate::exports::{ExportError, Exportable};
use crate::store::{AsStoreMut, AsStoreRef};
use crate::vm::{VMExtern, VMExternMemory, VMMemory};
use crate::MemoryAccessError;
use crate::MemoryType;
use crate::{AtomicsError, Extern};
use std::mem::MaybeUninit;
use wasmer_types::{MemoryError, Pages};

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
#[derive(Debug, Clone, PartialEq)]
pub struct Memory(pub(crate) memory_impl::Memory);

impl Memory {
    /// Creates a new host `Memory` from the provided [`MemoryType`].
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
        Ok(Self(memory_impl::Memory::new(store, ty)?))
    }

    /// Create a memory object from an existing memory and attaches it to the store
    pub fn new_from_existing(new_store: &mut impl AsStoreMut, memory: VMMemory) -> Self {
        Self(memory_impl::Memory::new_from_existing(new_store, memory))
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

    /// Grows the memory to at least a minimum size. If the memory is already big enough
    /// for the min size then this function does nothing
    pub fn grow_at_least(
        &self,
        store: &mut impl AsStoreMut,
        min_size: u64,
    ) -> Result<(), MemoryError> {
        self.0.grow_at_least(store, min_size)
    }

    /// Resets the memory back to zero length
    pub fn reset(&self, store: &mut impl AsStoreMut) -> Result<(), MemoryError> {
        self.0.reset(store)?;
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
            .try_copy(&store)
            .map(|new_memory| Self::new_from_existing(new_store, new_memory.into()))
    }

    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, vm_extern: VMExternMemory) -> Self {
        Self(memory_impl::Memory::from_vm_extern(store, vm_extern))
    }

    /// Checks whether this `Memory` can be used with the given context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.0.is_from_store(store)
    }

    /// Attempts to clone this memory (if its clonable)
    pub fn try_clone(&self, store: &impl AsStoreRef) -> Result<VMMemory, MemoryError> {
        self.0.try_clone(store)
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
            .try_clone(&store)
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
        self.0.as_shared(store)
    }

    /// To `VMExtern`.
    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        self.0.to_vm_extern()
    }
}

impl std::cmp::Eq for Memory {}

impl<'a> Exportable<'a> for Memory {
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Memory(memory) => Ok(memory),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}

/// Location in a WebAssembly memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MemoryLocation {
    // NOTE: must be expanded to an enum that also supports 64bit memory in
    // the future
    // That's why this is private.
    pub(crate) address: u32,
}

impl MemoryLocation {
    /// Create a new memory location for a 32bit memory.
    pub fn new_32(address: u32) -> Self {
        Self { address }
    }
}

impl From<u32> for MemoryLocation {
    fn from(value: u32) -> Self {
        Self::new_32(value)
    }
}

/// See [`SharedMemory`].
pub(crate) trait SharedMemoryOps {
    /// See [`SharedMemory::disable_atomics`].
    fn disable_atomics(&self) -> Result<(), MemoryError> {
        Err(MemoryError::AtomicsNotSupported)
    }

    /// See [`SharedMemory::wake_all_atomic_waiters`].
    fn wake_all_atomic_waiters(&self) -> Result<(), MemoryError> {
        Err(MemoryError::AtomicsNotSupported)
    }

    /// See [`SharedMemory::notify`].
    fn notify(&self, _dst: MemoryLocation, _count: u32) -> Result<u32, AtomicsError> {
        Err(AtomicsError::Unimplemented)
    }

    /// See [`SharedMemory::wait`].
    fn wait(
        &self,
        _dst: MemoryLocation,
        _timeout: Option<std::time::Duration>,
    ) -> Result<u32, AtomicsError> {
        Err(AtomicsError::Unimplemented)
    }
}

/// A handle that exposes operations only relevant for shared memories.
///
/// Enables interaction independent from the [`crate::Store`], and thus allows calling
/// some methods an instane is running.
///
/// **NOTE**: Not all methods are supported by all backends.
#[derive(Clone)]
pub struct SharedMemory {
    memory: Memory,
    ops: std::sync::Arc<dyn SharedMemoryOps + Send + Sync>,
}

impl std::fmt::Debug for SharedMemory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedMemory").finish()
    }
}

impl SharedMemory {
    /// Get the underlying memory.
    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    /// Create a new handle from ops.
    #[allow(unused)]
    pub(crate) fn new(memory: Memory, ops: impl SharedMemoryOps + Send + Sync + 'static) -> Self {
        Self {
            memory,
            ops: std::sync::Arc::new(ops),
        }
    }

    /// Notify up to `count` waiters waiting for the memory location.
    pub fn notify(&self, location: MemoryLocation, count: u32) -> Result<u32, AtomicsError> {
        self.ops.notify(location, count)
    }

    /// Wait for the memory location to be notified.
    pub fn wait(
        &self,
        location: MemoryLocation,
        timeout: Option<std::time::Duration>,
    ) -> Result<u32, AtomicsError> {
        self.ops.wait(location, timeout)
    }

    /// Disable atomics for this memory.
    ///
    /// All subsequent atomic wait calls will produce a trap.
    ///
    /// This can be used or forced shutdown of instances that continuously try
    /// to wait on atomics.
    ///
    /// NOTE: this operation might not be supported by all memory implementations.
    /// In that case, this function will return an error.
    pub fn disable_atomics(&self) -> Result<(), MemoryError> {
        self.ops.disable_atomics()
    }

    /// Wake up all atomic waiters.
    ///
    /// This can be used to force-resume waiting execution.
    ///
    /// NOTE: this operation might not be supported by all memory implementations.
    /// In that case, this function will return an error.
    pub fn wake_all_atomic_waiters(&self) -> Result<(), MemoryError> {
        self.ops.wake_all_atomic_waiters()
    }
}

/// Underlying buffer for a memory.
#[derive(Debug, Copy, Clone)]
pub(crate) struct MemoryBuffer<'a>(pub(crate) memory_impl::MemoryBuffer<'a>);

impl<'a> MemoryBuffer<'a> {
    #[allow(unused)]
    pub(crate) fn read(&self, offset: u64, buf: &mut [u8]) -> Result<(), MemoryAccessError> {
        self.0.read(offset, buf)
    }

    #[allow(unused)]
    pub(crate) fn read_uninit<'b>(
        &self,
        offset: u64,
        buf: &'b mut [MaybeUninit<u8>],
    ) -> Result<&'b mut [u8], MemoryAccessError> {
        self.0.read_uninit(offset, buf)
    }

    #[allow(unused)]
    pub(crate) fn write(&self, offset: u64, data: &[u8]) -> Result<(), MemoryAccessError> {
        self.0.write(offset, data)
    }
}
