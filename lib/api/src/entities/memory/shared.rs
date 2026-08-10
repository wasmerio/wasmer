use std::sync::Arc;

use wasmer_types::{MemoryError, MemoryStyle, Pages};

use crate::{
    AsStoreMut, Memory,
    error::AtomicsError,
    location::{MemoryLocation, SharedMemoryOps},
    vm::VMMemory,
    vm::VMSharedMemory,
};

/// A shared memory instance that can be shared across multiple stores and threads,
/// not attached to any specific store.
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
    pub fn attach(self, store: &mut impl AsStoreMut) -> Memory {
        let memory = self.memory.into_vm_memory(store);
        Memory::new_from_existing(store, memory)
    }

    /// Create an operations handle that does not keep the underlying memory alive.
    pub fn ops(&self) -> MemoryOps {
        MemoryOps {
            ops: self.ops.clone(),
        }
    }

    /// Grows this memory by `delta` pages, returning the previous size.
    ///
    /// Unlike [`Memory::grow`] this needs no store, so an embedder holding a
    /// clone of this handle can grow the memory from a thread that has no
    /// access to one. Growth is shared: every clone of the handle addresses
    /// the same allocation and observes the new size.
    ///
    /// Growing is serialized per memory and returns the size the memory had
    /// beforehand, so concurrent callers each learn the range they claimed.
    ///
    /// Returns [`MemoryError::UnsupportedOperation`] on backends that cannot
    /// grow a shared memory without a store (currently every backend except
    /// `sys`).
    pub fn grow(&self, delta: Pages) -> Result<Pages, MemoryError> {
        self.memory.grow(delta)
    }

    /// The host address of guest offset 0, without a store.
    ///
    /// This is the same pointer [`MemoryView::data_ptr`] returns, for
    /// embedders that need it on a thread with no store. Returns `None` on
    /// backends that cannot report it store-free.
    ///
    /// The pointer is only stable across [`SharedMemory::grow`] if the host
    /// mapping was reserved up front — see [`SharedMemory::style`]. For a
    /// [`MemoryStyle::Dynamic`] memory, growth may move the mapping and
    /// invalidate any previously returned pointer.
    ///
    /// [`MemoryView::data_ptr`]: crate::MemoryView::data_ptr
    pub fn data_ptr(&self) -> Option<*mut u8> {
        self.memory.data_ptr()
    }

    /// How this memory's host mapping is laid out, or `None` on backends that
    /// do not model a style.
    ///
    /// A [`MemoryStyle::Static`] memory whose `bound` covers its maximum has
    /// its whole range reserved up front, so growing it can never move the
    /// mapping and [`SharedMemory::data_ptr`] stays valid for the memory's
    /// lifetime.
    pub fn style(&self) -> Option<MemoryStyle> {
        self.memory.style()
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

    mod store_free_ops {
        use crate::{Memory, Store};
        use wasmer_types::{MemoryError, MemoryStyle, MemoryType, Pages};

        /// `None` when the default backend of this build cannot create a shared
        /// memory at all, which is not something these tests can assert about.
        ///
        /// Note this deliberately does *not* key off `cfg(feature = "sys")`: the
        /// workspace test matrix builds `wasmer` with `sys` compiled in while
        /// `Store::default()` resolves to another backend (feature unification
        /// plus `v8-default`), so the feature says nothing about which backend
        /// is actually in play.
        fn shared_memory() -> Option<(Store, crate::SharedMemory)> {
            let mut store = Store::default();
            let ty = MemoryType {
                minimum: Pages(1),
                maximum: Some(Pages(4)),
                shared: true,
            };
            let memory = Memory::new(&mut store, ty).ok()?;
            let shared = memory.as_shared(&store)?;
            Some((store, shared))
        }

        /// The three store-free accessors are all-or-nothing: a backend either
        /// implements the set (sys) or reports it unavailable (v8/js). Assert
        /// whichever contract this build implements, and that the three agree
        /// with one another — a backend offering `data_ptr` but no `style`
        /// would leave a caller unable to tell whether caching the base is safe.
        #[test]
        fn store_free_ops_match_the_backend_contract() {
            let Some((_store, shared)) = shared_memory() else {
                return;
            };

            match (shared.data_ptr(), shared.style()) {
                (Some(before), Some(style)) => {
                    assert!(!before.is_null());

                    // `grow` reports the size the memory had beforehand, and the
                    // growth is visible through an independent clone: the whole
                    // point of the `&self` receiver is that several threads can
                    // hold their own handle.
                    let clone = shared.clone();
                    assert_eq!(shared.grow(Pages(1)).expect("grow"), Pages(1));
                    assert_eq!(clone.grow(Pages(2)).expect("grow via clone"), Pages(2));
                    // Past the declared maximum of 4 pages.
                    assert!(shared.grow(Pages(1)).is_err());

                    let after = shared.data_ptr().expect("data_ptr after grow");
                    match style {
                        // A fully reserved mapping cannot move, which is the
                        // invariant that makes caching the base sound.
                        MemoryStyle::Static { bound, .. } if bound >= Pages(4) => {
                            assert_eq!(before, after, "a reserved mapping must not move")
                        }
                        // A dynamic mapping may move; only require a readable base.
                        _ => assert!(!after.is_null()),
                    }
                }
                (None, None) => {
                    // Must degrade with an error rather than panicking.
                    assert!(matches!(
                        shared.grow(Pages(1)),
                        Err(MemoryError::UnsupportedOperation { .. })
                    ));
                }
                (data_ptr, style) => panic!(
                    "data_ptr and style must agree on support: data_ptr={} style={}",
                    data_ptr.is_some(),
                    style.is_some()
                ),
            }
        }
    }
}
