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

    /// Pins an exact anonymous private writable range in native static memory.
    /// Cloned handles share the same physical mapping exclusion.
    #[cfg(feature = "sys")]
    #[doc(hidden)]
    pub fn pin_host_range(
        &self,
        range: std::ops::Range<u64>,
    ) -> Result<crate::sys::HostMemoryPin, MemoryError> {
        match &self.memory {
            VMSharedMemory::Sys(memory) => memory.pin_host_range(range),
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
    #[cfg(all(feature = "sys", feature = "wat", feature = "compiler"))]
    #[test]
    fn defined_shared_memory_retains_its_metadata_after_store_drop_and_worker_reattach() {
        use crate::{AsStoreMut, Function, Instance, Module, Store, imports};
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use wasmer_vm::{StoreObject, VMInstance};
        struct OwnerThreadDrop {
            owner: std::thread::ThreadId,
            drops: Arc<AtomicUsize>,
        }
        impl Drop for OwnerThreadDrop {
            fn drop(&mut self) {
                assert_eq!(self.owner, std::thread::current().id());
                self.drops.fetch_add(1, Ordering::SeqCst);
            }
        }
        fn send_sync<T: Send + Sync>() {}
        send_sync::<wasmer_vm::VMInstanceStorage>();
        let drops = Arc::new(AtomicUsize::new(0));
        let shared = {
            let mut store = Store::default();
            let probe = OwnerThreadDrop {
                owner: std::thread::current().id(),
                drops: drops.clone(),
            };
            let host = Function::new_typed(&mut store, move || {
                std::hint::black_box(&probe);
            });
            let module = Module::new(
                &store,
                r#"(module
                (import "env" "host" (func))
                (memory (export "memory") 1 3 shared))"#,
            )
            .unwrap();
            let instance = Instance::new(
                &mut store,
                &module,
                &imports! { "env" => { "host" => host } },
            )
            .unwrap();
            let memory = instance.exports.get_memory("memory").unwrap();
            memory.view(&store).write(0, &[42]).unwrap();
            let shared = memory.as_shared(&store).unwrap();
            // Missing memory-side custody must fail before destroying or reading
            // the original inline definition after Store destruction.
            assert!(
                VMInstance::list(store.objects_mut().as_sys_mut())[0].storage_owner_count() > 1,
                "defined memory has no independent definition storage owner"
            );
            shared
        };
        assert_eq!(
            drops.load(Ordering::SeqCst),
            1,
            "detached memory must not retain Instance fields or its Store"
        );
        let shared = std::thread::spawn(move || {
            let mut store = Store::default();
            let memory = shared.clone().try_attach(&mut store).unwrap();
            assert_eq!(memory.ty(&store).minimum, crate::Pages(1));
            assert_eq!(
                memory.grow(&mut store, crate::Pages(1)).unwrap(),
                crate::Pages(1)
            );
            assert_eq!(memory.ty(&store).minimum, crate::Pages(2));
            let mut byte = [0];
            memory.view(&store).read(0, &mut byte).unwrap();
            assert_eq!(byte, [42]);
            let module = Module::new(
                &store,
                r#"(module
                (import "env" "memory" (memory 1 3 shared))
                (func (export "size") (result i32) memory.size))"#,
            )
            .unwrap();
            let instance = Instance::new(
                &mut store,
                &module,
                &imports! { "env" => { "memory" => memory } },
            )
            .unwrap();
            assert_eq!(
                instance
                    .exports
                    .get_typed_function::<(), i32>(&store, "size")
                    .unwrap()
                    .call(&mut store)
                    .unwrap(),
                2
            );
            drop(store);
            shared
        })
        .join()
        .unwrap();
        assert_eq!(shared.host_data().unwrap().1, 2 * 65536);
        drop(shared);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[cfg(all(feature = "sys", feature = "wat", feature = "compiler"))]
    #[test]
    fn a_partial_instantiation_error_keeps_published_definition_storage_alive() {
        use crate::sys::{BaseTunables, NativeEngineExt, Tunables};
        use crate::{Imports, Instance, Module, Store};
        use std::sync::{Arc, Mutex, Weak};
        use wasmer_types::{MemoryStyle, MemoryType, TableStyle, TableType};
        use wasmer_vm::{
            LinearMemory, MemoryError, VMDefinitionLocation, VMInstanceStorage, VMMemory,
            VMMemoryDefinition, VMSharedMemory, VMTable, VMTableDefinition,
        };
        type PublishedMemory = Arc<Mutex<Option<(VMSharedMemory, Weak<VMInstanceStorage>)>>>;
        struct FailAfterMemory {
            base: BaseTunables,
            published: PublishedMemory,
        }
        impl Tunables for FailAfterMemory {
            fn memory_style(&self, ty: &MemoryType) -> MemoryStyle {
                self.base.memory_style(ty)
            }
            fn table_style(&self, ty: &TableType) -> TableStyle {
                self.base.table_style(ty)
            }
            fn create_host_memory(
                &self,
                ty: &MemoryType,
                style: &MemoryStyle,
            ) -> Result<VMMemory, MemoryError> {
                self.base.create_host_memory(ty, style)
            }
            unsafe fn create_vm_memory(
                &self,
                ty: &MemoryType,
                style: &MemoryStyle,
                location: VMDefinitionLocation<VMMemoryDefinition>,
            ) -> Result<VMMemory, MemoryError> {
                let owner = Arc::downgrade(location.storage().expect("managed definition storage"));
                // SAFETY: pass the location's exact pointer and custody together.
                let memory = unsafe { self.base.create_vm_memory(ty, style, location)? };
                let shared = memory.as_shared()?;
                *self.published.lock().unwrap() = Some((shared, owner));
                Ok(memory)
            }
            fn create_host_table(
                &self,
                ty: &TableType,
                style: &TableStyle,
            ) -> Result<VMTable, String> {
                self.base.create_host_table(ty, style)
            }
            unsafe fn create_vm_table(
                &self,
                _ty: &TableType,
                _style: &TableStyle,
                _location: VMDefinitionLocation<VMTableDefinition>,
            ) -> Result<VMTable, String> {
                Err("intentional failure after publishing the memory".into())
            }
        }
        let published = Arc::new(Mutex::new(None));
        let mut store = Store::default();
        store.engine_mut().set_tunables(FailAfterMemory {
            base: BaseTunables::new(),
            published: published.clone(),
        });
        let module =
            Module::new(&store, r#"(module (memory 1 3 shared) (table 1 funcref))"#).unwrap();
        assert!(Instance::new(&mut store, &module, &Imports::new()).is_err());
        let (mut memory, owner) = published.lock().unwrap().take().unwrap();
        drop(store);
        // Assert custody before touching a definition whose allocator unwound.
        assert!(
            owner.upgrade().is_some(),
            "partial-error publication lost definition storage"
        );
        assert_eq!(memory.ty().minimum, crate::Pages(1));
        assert_eq!(memory.grow(crate::Pages(1)).unwrap(), crate::Pages(1));
        let copy = memory.copy().unwrap();
        drop(memory);
        assert!(
            owner.upgrade().is_none(),
            "a physical copy retained the source definition storage"
        );
        assert_eq!(copy.ty().minimum, crate::Pages(2));
    }
}
