use std::sync::Arc;

use crate::{
    AsStoreMut, Memory,
    error::AtomicsError,
    location::{MemoryLocation, SharedMemoryOps},
    vm::VMMemory,
};

/// A shared memory instance that can be shared across multiple stores and threads,
/// not attached to any specific store.
pub struct SharedMemory {
    memory: VMMemory,
    ops: Option<Arc<dyn SharedMemoryOps + Send + Sync>>,
}

unsafe impl Send for SharedMemory {}
unsafe impl Sync for SharedMemory {}

impl std::fmt::Debug for SharedMemory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedMemory").finish()
    }
}

impl Clone for SharedMemory {
    fn clone(&self) -> Self {
        Self {
            memory: self
                .memory
                .try_clone()
                .expect("Internal error: shared memory should be cloneable"),
            ops: self.ops.clone(),
        }
    }
}

impl SharedMemory {
    /// Create a new shared memory from an existing VMMemory.
    pub(crate) fn from_vm_memory(memory: VMMemory) -> Self {
        Self { memory, ops: None }
    }

    /// Create a new shared memory from an existing VMMemory.
    pub(crate) fn from_vm_memory_and_ops(
        memory: VMMemory,
        ops: Arc<dyn SharedMemoryOps + Send + Sync>,
    ) -> Self {
        Self {
            memory,
            ops: Some(ops),
        }
    }

    /// Attach this shared memory to the provided store.
    pub fn attach(self, store: &mut impl AsStoreMut) -> Memory {
        Memory::new_from_existing(store, self.memory)
    }

    #[inline]
    fn ops(&self) -> Result<&(dyn SharedMemoryOps + Send + Sync), AtomicsError> {
        self.ops
            .as_ref()
            .map(|ops| ops.as_ref())
            .ok_or(AtomicsError::Unimplemented)
    }

    /// Notify up to `count` waiters waiting for the memory location.
    pub fn notify(&self, location: MemoryLocation, count: u32) -> Result<u32, AtomicsError> {
        self.ops()?.notify(location, count)
    }

    /// Wait for the memory location to be notified.
    pub fn wait(
        &self,
        location: MemoryLocation,
        timeout: Option<std::time::Duration>,
    ) -> Result<u32, AtomicsError> {
        self.ops()?.wait(location, timeout)
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
    pub fn disable_atomics(&self) -> Result<(), AtomicsError> {
        self.ops()?.disable_atomics()
    }

    /// Wake up all atomic waiters.
    ///
    /// This can be used to force-resume waiting execution.
    ///
    /// NOTE: this operation might not be supported by all memory implementations.
    /// In that case, this function will return an error.
    pub fn wake_all_atomic_waiters(&self) -> Result<(), AtomicsError> {
        self.ops()?.wake_all_atomic_waiters()
    }
}
