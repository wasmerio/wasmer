use wasmer_types::MemoryError;

use crate::{
    error::AtomicsError,
    location::{MemoryLocation, SharedMemoryOps},
    Memory,
};

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
