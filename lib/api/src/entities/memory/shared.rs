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
    #[cfg(feature = "sys")]
    native_owner: Option<Arc<wasmer_vm::VMMemory>>,
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
            #[cfg(feature = "sys")]
            native_owner: self.native_owner.clone(),
        }
    }
}

impl SharedMemory {
    /// Create a new shared memory.
    pub(crate) fn new(memory: VMSharedMemory) -> Self {
        Self {
            memory,
            ops: None,
            #[cfg(feature = "sys")]
            native_owner: None,
        }
    }

    /// Create a new shared memory with memory operations.
    pub(crate) fn new_with_ops(
        memory: VMSharedMemory,
        ops: Arc<dyn SharedMemoryOps + Send + Sync>,
    ) -> Self {
        Self {
            memory,
            ops: Some(ops),
            #[cfg(feature = "sys")]
            native_owner: None,
        }
    }

    // Keep custom LinearMemory wrappers through detach/reattach. Flattening to
    // VMSharedMemory alone loses allocation accounting and wrapped growth.
    #[cfg(feature = "sys")]
    pub(crate) fn new_native(
        memory: VMSharedMemory,
        owner: wasmer_vm::VMMemory,
        ops: Option<Arc<dyn SharedMemoryOps + Send + Sync>>,
    ) -> Self {
        let mut shared = match ops {
            Some(ops) => Self::new_with_ops(memory, ops),
            None => Self::new(memory),
        };
        shared.native_owner = Some(Arc::new(owner));
        shared
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
    /// Custom native memory wrappers may reject cloning for a new attachment.
    pub fn try_attach(self, store: &mut impl AsStoreMut) -> Result<Memory, MemoryError> {
        #[cfg(feature = "sys")]
        if let Some(owner) = self.native_owner {
            let memory = match Arc::try_unwrap(owner) {
                Ok(memory) => memory,
                Err(owner) => owner.try_clone()?,
            };
            return Ok(Memory::new_from_existing(store, VMMemory::Sys(memory)));
        }
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

    /// Pins an exact anonymous private writable range in native static memory.
    /// Cloned handles share the same physical mapping exclusion.
    #[cfg(feature = "sys")]
    #[doc(hidden)]
    pub fn pin_host_range(
        &self,
        range: std::ops::Range<u64>,
    ) -> Result<crate::sys::HostMemoryPin, MemoryError> {
        match &self.memory {
            VMSharedMemory::Sys(memory) => {
                let mut pin = memory.pin_host_range(range)?;
                self.retain_native_owner(&mut pin);
                Ok(pin)
            }
            #[allow(unreachable_patterns)]
            _ => Err(MemoryError::UnsupportedOperation {
                message: "native host pins are unsupported by this backend".into(),
            }),
        }
    }

    /// Admits a finite physical replacement before any OS operation or callback.
    /// Keep this allocation alive until the change and its metadata finish.
    #[cfg(feature = "sys")]
    #[doc(hidden)]
    pub fn begin_host_mapping_change(
        &self,
        range: std::ops::Range<u64>,
    ) -> Result<crate::sys::HostMemoryMappingChange, MemoryError> {
        match &self.memory {
            VMSharedMemory::Sys(memory) => {
                use wasmer_vm::LinearMemory as _;
                memory.begin_host_mapping_change(range)
            }
            #[allow(unreachable_patterns)]
            _ => Err(MemoryError::UnsupportedOperation {
                message: "native mapping changes are unsupported by this backend".into(),
            }),
        }
    }

    /// Atomically promotes a completed fresh private mapping to a retained pin.
    ///
    /// # Safety
    /// The exact change range must now have anonymous private writable backing;
    /// all physical changes and their metadata must be complete.
    #[cfg(feature = "sys")]
    #[doc(hidden)]
    pub unsafe fn commit_private_host_pin(
        &self,
        mapping: crate::sys::HostMemoryMappingChange,
    ) -> Result<crate::sys::HostMemoryPin, MemoryError> {
        match &self.memory {
            VMSharedMemory::Sys(memory) => {
                // SAFETY: caller proves successful replacement of the exact range.
                let mut pin = unsafe { memory.commit_private_host_pin(mapping) }?;
                self.retain_native_owner(&mut pin);
                Ok(pin)
            }
            #[allow(unreachable_patterns)]
            _ => Err(MemoryError::UnsupportedOperation {
                message: "native host pins are unsupported by this backend".into(),
            }),
        }
    }

    #[cfg(feature = "sys")]
    fn retain_native_owner(&self, pin: &mut crate::sys::HostMemoryPin) {
        if let Some(owner) = &self.native_owner {
            // A newly created pin has no previous wrapper owner. The mapping
            // state lock was released before retaining or dropping this Arc.
            assert!(pin.retain_owner(owner.clone()).is_ok());
        }
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

    #[cfg(feature = "sys")]
    #[test]
    fn mapping_changes_preserve_no_op_growth_for_movable_memory() {
        use wasmer_types::{MemoryStyle, MemoryType, Pages};
        use wasmer_vm::LinearMemory as _;
        let mut raw = wasmer_vm::VMSharedMemory::new(
            &MemoryType::new(1, Some(4), true),
            &MemoryStyle::Dynamic {
                offset_guard_size: 0,
            },
        )
        .unwrap();
        // SAFETY: the owned definition remains alive; only the copied address escapes.
        let base = unsafe { raw.vmmemory().as_ref().base };
        let mapping = raw.begin_host_mapping_change(0..65536).unwrap();
        assert_eq!(raw.grow(Pages(0)).unwrap(), Pages(1));
        raw.grow_at_least(65536).unwrap();
        // SAFETY: no actual growth is admitted while the mapping transaction exists.
        assert_eq!(unsafe { raw.vmmemory().as_ref().base }, base);
        assert_eq!(raw.size(), Pages(1));
        assert!(raw.grow(Pages(1)).is_err());
        assert!(raw.grow_at_least(131072).is_err());
        drop(mapping);
        assert_eq!(raw.grow(Pages(1)).unwrap(), Pages(1));
        assert_eq!(raw.size(), Pages(2));
    }

    #[cfg(feature = "sys")]
    #[test]
    fn host_pins_keep_exact_allocation_custody_across_growth_and_copy() {
        use wasmer_types::{MemoryStyle, MemoryType, Pages};
        use wasmer_vm::LinearMemory as _;
        let mut raw = wasmer_vm::VMSharedMemory::new(
            &MemoryType::new(2, Some(4), true),
            &MemoryStyle::Static,
        )
        .unwrap();
        let shared = super::SharedMemory::new(crate::vm::VMSharedMemory::Sys(raw.clone()));
        let pin = shared.pin_host_range(65536..131072).unwrap();
        let pointer = pin.as_ptr();
        let copy = raw.copy().unwrap();
        assert!(
            copy.pin_host_range(65536..131072).is_ok(),
            "physical copies have independent pins"
        );
        raw.grow(Pages(1)).unwrap();
        assert_eq!(pin.as_ptr(), pointer);
        assert!(raw.reset().is_err());
        assert!(
            shared
                .clone()
                .begin_host_mapping_change(65536..131072)
                .is_err()
        );
        drop(shared);
        drop(raw);
        // SAFETY: the token alone retains the exact allocation and its protection.
        unsafe {
            pointer.write(42);
            assert_eq!(pointer.read(), 42);
        }
        drop(pin);
    }

    #[cfg(feature = "sys")]
    #[test]
    fn unfinished_mapping_changes_remain_unknown_after_owned_to_shared_conversion() {
        use wasmer_types::{MemoryStyle, MemoryType};
        use wasmer_vm::LinearMemory as _;
        let owned =
            wasmer_vm::VMOwnedMemory::new(&MemoryType::new(2, Some(4), true), &MemoryStyle::Static)
                .unwrap();
        let mapping = owned.begin_host_mapping_change(65536..131072).unwrap();
        let shared = owned.to_shared();
        assert!(shared.pin_host_range(65536..131072).is_err());
        assert!(
            shared
                .clone()
                .begin_host_mapping_change(65536..131072)
                .is_err()
        );
        drop(mapping);
        assert!(
            shared.pin_host_range(65536..131072).is_err(),
            "interrupted changes must not restore old private assumptions"
        );
        assert!(shared.pin_host_range(0..65536).is_ok());
        assert!(
            shared.copy().unwrap().pin_host_range(65536..131072).is_ok(),
            "a physical copy establishes fresh anonymous backing"
        );
    }

    #[cfg(all(feature = "sys", feature = "wat", feature = "compiler"))]
    #[test]
    fn a_pin_outlives_the_store_of_an_instance_defined_memory() {
        use crate::{FunctionEnv, Imports, Instance, Module, Store};
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        struct ObserveDrop(Arc<AtomicUsize>);
        impl Drop for ObserveDrop {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicUsize::new(0));
        let pin = {
            let mut store = Store::default();
            let _env = FunctionEnv::new(&mut store, ObserveDrop(dropped.clone()));
            let module =
                Module::new(&store, r#"(module (memory (export "memory") 2 4 shared))"#).unwrap();
            let instance = Instance::new(&mut store, &module, &Imports::new()).unwrap();
            let memory = instance.exports.get_memory("memory").unwrap();
            let shared = memory.as_shared(&store).unwrap();
            let pin = shared.pin_host_range(65536..131072).unwrap();
            // Check before destroying inline VM metadata: an old derived Debug must
            // fail here, without first dereferencing metadata after its owner died.
            assert!(
                !format!("{pin:?}").contains("memory:"),
                "pin Debug must not traverse VM memory metadata"
            );
            pin
        };
        assert_eq!(
            dropped.load(Ordering::SeqCst),
            1,
            "the pin must not retain its Store"
        );
        // SAFETY: the exact pin alone now owns these stable anonymous writable bytes.
        unsafe {
            pin.as_ptr().write(42);
            assert_eq!(pin.as_ptr().read(), 42);
        }
        assert!(format!("{pin:?}").contains("range:"));
        drop(pin);
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
