use std::sync::Arc;

use crate::{
    AsStoreMut, Memory, MemoryError,
    error::AtomicsError,
    location::{MemoryLocation, SharedMemoryOps},
    vm::VMMemory,
    vm::VMSharedMemory,
};

/// A detached handle to shared linear memory, attachable to multiple stores and threads.
///
/// This type never represents non-shared linear memory or an owned byte snapshot.
pub struct SharedMemory {
    memory: VMSharedMemory,
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
        Self {
            memory: self.memory.clone(),
            ops: self.ops.clone(),
        }
    }
}

impl SharedMemory {
    /// Create a new shared memory.
    pub(crate) fn new(memory: VMSharedMemory) -> Self {
        Self { memory, ops: None }
    }

    /// Create a new shared memory with memory operations.
    pub(crate) fn new_with_ops(
        memory: VMSharedMemory,
        ops: Arc<dyn SharedMemoryOps + Send + Sync>,
    ) -> Self {
        Self {
            memory,
            ops: Some(ops),
        }
    }

    /// Attach this shared memory to the provided store.
    ///
    /// # Panics
    /// Panics where [`SharedMemory::try_attach`] fails.
    pub fn attach(self, store: &mut impl AsStoreMut) -> Memory {
        self.try_attach(store)
            .unwrap_or_else(|err| panic!("failed to attach shared memory: {err}"))
    }

    /// Attach this shared memory to the provided store.
    ///
    /// On the `js` backend this fails if the memory has not been delivered to
    /// the current worker (see `wasmer::js::receive_shared_object_message`).
    /// Other backends always succeed.
    pub fn try_attach(self, store: &mut impl AsStoreMut) -> Result<Memory, MemoryError> {
        let memory = self.memory.try_into_vm_memory(store)?;
        Ok(Memory::new_from_existing(store, memory))
    }

    /// The host address of the memory's first byte and its current size in bytes, read
    /// without a store, or `None` where the backend does not keep the memory in host memory
    /// that never moves. The `sys` backend returns `Some` only for a static reservation
    /// covering the full declared maximum (or the wasm32 maximum if unspecified); dynamic
    /// reservations and other backends return `None`.
    ///
    /// For a returned pointer the base never moves, so the first `size` bytes stay
    /// valid for as long as this handle keeps the memory alive. Other threads may access them
    /// at any time: reads and writes through the pointer race with the guest as its own
    /// threads do with each other.
    #[doc(hidden)]
    pub fn host_data(&self) -> Option<(*mut u8, u64)> {
        self.memory.host_data()
    }

    /// The JS Memory already registered on the calling worker, or `None` if it
    /// has not been delivered there. The returned JS value must stay on that worker.
    #[cfg(feature = "js")]
    pub fn local_js_memory(&self) -> Option<js_sys::WebAssembly::Memory> {
        match &self.memory {
            VMSharedMemory::Js(memory) => memory.local_memory(),
            #[allow(unreachable_patterns)]
            _ => None,
        }
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
    #[cfg(feature = "sys")]
    #[test]
    fn host_data_requires_a_reservation_covering_the_full_maximum() {
        use wasmer_types::{MemoryStyle, MemoryType, Pages};
        use wasmer_vm::LinearMemory as _;
        for style in [
            MemoryStyle::Static,
            MemoryStyle::Dynamic {
                offset_guard_size: 0,
            },
        ] {
            let mut raw =
                wasmer_vm::VMSharedMemory::new(&MemoryType::new(1, Some(4), true), &style).unwrap();
            let shared = super::SharedMemory::new(crate::vm::VMSharedMemory::Sys(raw.clone()));
            if matches!(style, MemoryStyle::Dynamic { .. }) {
                assert!(
                    shared.host_data().is_none(),
                    "a dynamic memory may move on growth"
                );
                continue;
            }
            let (base, size) = shared.host_data().unwrap();
            assert_eq!(size, 65536);
            for pages in 2..=4 {
                raw.grow(Pages(1)).unwrap();
                assert_eq!(shared.host_data(), Some((base, pages * 65536)));
            }
        }
    }

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
