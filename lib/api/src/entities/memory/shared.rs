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

/// Shared memory operations that do not hold the underlying memory alive.
///
/// This handle is intended for operations, such as waking atomic waiters, that
/// may be attempted after the original memory owner has started shutting down.
#[derive(Clone)]
pub struct MemoryOps {
    ops: Option<Arc<dyn SharedMemoryOps + Send + Sync>>,
}

impl std::fmt::Debug for SharedMemory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedMemory").finish()
    }
}

impl std::fmt::Debug for MemoryOps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryOps").finish()
    }
}

impl Clone for SharedMemory {
    fn clone(&self) -> Self {
        let Ok(memory) = self.memory.try_clone() else {
            unreachable!("Internal error: shared memory is always cloneable");
        };
        Self {
            memory,
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

    /// Create an operations handle that does not keep the underlying memory alive.
    pub fn ops(&self) -> MemoryOps {
        MemoryOps {
            ops: self.ops.clone(),
        }
    }

    #[inline]
    fn shared_ops(&self) -> Result<&(dyn SharedMemoryOps + Send + Sync), AtomicsError> {
        self.ops
            .as_ref()
            .map(|ops| ops.as_ref())
            .ok_or(AtomicsError::Unimplemented)
    }

    /// Notify up to `count` waiters waiting for the memory location.
    pub fn notify(&self, location: MemoryLocation, count: u32) -> Result<u32, AtomicsError> {
        self.shared_ops()?.notify(location, count)
    }

    /// Wait for the memory location to be notified.
    pub fn wait(
        &self,
        location: MemoryLocation,
        timeout: Option<std::time::Duration>,
    ) -> Result<u32, AtomicsError> {
        self.shared_ops()?.wait(location, timeout)
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
        self.shared_ops()?.disable_atomics()
    }

    /// Wake up all atomic waiters.
    ///
    /// This can be used to force-resume waiting execution.
    ///
    /// NOTE: this operation might not be supported by all memory implementations.
    /// In that case, this function will return an error.
    pub fn wake_all_atomic_waiters(&self) -> Result<(), AtomicsError> {
        self.shared_ops()?.wake_all_atomic_waiters()
    }
}

impl MemoryOps {
    #[inline]
    fn shared_ops(&self) -> Result<&(dyn SharedMemoryOps + Send + Sync), AtomicsError> {
        self.ops
            .as_ref()
            .map(|ops| ops.as_ref())
            .ok_or(AtomicsError::Unimplemented)
    }

    /// Notify up to `count` waiters waiting for the memory location.
    pub fn notify(&self, location: MemoryLocation, count: u32) -> Result<u32, AtomicsError> {
        self.shared_ops()?.notify(location, count)
    }

    /// Wait for the memory location to be notified.
    pub fn wait(
        &self,
        location: MemoryLocation,
        timeout: Option<std::time::Duration>,
    ) -> Result<u32, AtomicsError> {
        self.shared_ops()?.wait(location, timeout)
    }

    /// Disable atomics for this memory if it is still alive.
    ///
    /// All subsequent atomic wait calls will produce a trap.
    pub fn disable_atomics(&self) -> Result<(), AtomicsError> {
        self.shared_ops()?.disable_atomics()
    }

    /// Wake up all atomic waiters if the memory is still alive.
    pub fn wake_all_atomic_waiters(&self) -> Result<(), AtomicsError> {
        self.shared_ops()?.wake_all_atomic_waiters()
    }
}

impl From<SharedMemory> for MemoryOps {
    fn from(memory: SharedMemory) -> Self {
        memory.ops()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    pub fn ensure_shared_memory_handles_are_send_and_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<super::SharedMemory>();
        assert_sync::<super::SharedMemory>();
        assert_send::<super::MemoryOps>();
        assert_sync::<super::MemoryOps>();
    }
}
