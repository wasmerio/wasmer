use macro_wasmer_engine_test::engine_test;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[cfg(feature = "js")]
use wasm_bindgen_test::*;
use wasmer::{Instance, Memory, MemoryLocation, MemoryType, Module, Store, imports};

#[test]
#[allow(unused_attributes)]
#[cfg_attr(
    feature = "v8",
    ignore = "v8 does not currently support shared memory through wasm_c_api"
)]
fn test_shared_memory_atomics_notify_send() {
    let mut store = Store::default();
    let wat = r#"(module
(import "host" "memory" (memory 10 65536 shared))
)"#;
    let module = Module::new(&store, wat)
        .map_err(|e| format!("{e:?}"))
        .unwrap();

    let mem = Memory::new(&mut store, MemoryType::new(10, Some(65536), true)).unwrap();

    let imports = imports! {
        "host" => {
            "memory" => mem.clone(),
        },
    };

    let _inst = Instance::new(&mut store, &module, &imports).unwrap();

    let mem = if let Some(m) = mem.as_shared(&store) {
        m
    } else {
        #[cfg(feature = "sys")]
        panic!("Memory is not shared");
        #[cfg(not(feature = "sys"))]
        return;
    };

    // Test basic notify.
    let mem2 = mem.clone();
    std::thread::spawn(move || {
        loop {
            if mem2.notify(MemoryLocation::new_32(10), 1).unwrap() > 0 {
                break;
            }
        }
    });

    mem.wait(MemoryLocation::new_32(10), None).unwrap();

    // Test wake_all

    let done = Arc::new(AtomicBool::new(false));

    std::thread::spawn({
        let mem = mem.clone();
        let done = done.clone();
        move || {
            while !done.load(Ordering::SeqCst) {
                mem.wake_all_atomic_waiters().ok();
            }
        }
    });

    mem.wait(MemoryLocation::new_32(10), None).unwrap();
    done.store(true, Ordering::SeqCst);
}

#[cfg(feature = "sys")]
#[test]
#[cfg_attr(
    feature = "v8-default",
    ignore = "shared memory atomics are not supported by the default v8 backend"
)]
fn test_shared_memory_disable_atomics() {
    use wasmer::AtomicsError;

    let mut store = Store::default();
    let mem = Memory::new(&mut store, MemoryType::new(10, Some(65536), true)).unwrap();

    let mem = mem.as_shared(&store).unwrap();
    mem.disable_atomics().unwrap();

    let err = mem.wait(MemoryLocation::new_32(1), None).unwrap_err();
    assert_eq!(err, AtomicsError::AtomicsDisabled);
}

#[cfg(feature = "sys")]
#[test]
fn test_shared_memory_copy_is_independent() {
    let mut store = Store::default();
    let memory = Memory::new(&mut store, MemoryType::new(1, Some(2), true)).unwrap();

    memory.view(&store).write(0, b"before").unwrap();

    let mut copy_store = Store::default();
    let copied = memory.copy(&store).unwrap().attach(&mut copy_store);

    memory.view(&store).write(0, b"after!").unwrap();

    let mut buf = [0; 6];
    copied.view(&copy_store).read(0, &mut buf).unwrap();
    assert_eq!(&buf, b"before");
}

#[engine_test]
fn test_non_shared_memory_copy_is_rejected() -> Result<(), String> {
    let mut store = Store::default();
    let memory = Memory::new(&mut store, MemoryType::new(1, Some(2), false)).unwrap();
    assert!(matches!(
        memory.copy(&store),
        Err(wasmer::MemoryError::MemoryNotShared)
    ));
    assert!(memory.as_shared(&store).is_none());
    let mut other_store = Store::default();
    assert!(matches!(
        memory.copy_to_store(&store, &mut other_store),
        Err(wasmer::MemoryError::MemoryNotShared)
    ));
    Ok(())
}

/// See https://github.com/wasmerio/wasmer/issues/5444
#[test]
#[cfg(feature = "sys")]
fn test_wasm_slice_issue_5444() {
    let mut store = Store::default();
    let wat = r#"(module
(import "host" "memory" (memory 10 65536))
)"#;
    let module = Module::new(&store, wat)
        .map_err(|e| format!("{e:?}"))
        .unwrap();

    let mem = Memory::new(&mut store, MemoryType::new(10, Some(65536), false)).unwrap();

    let imports = imports! {
        "host" => {
            "memory" => mem.clone(),
        },
    };

    let _inst = Instance::new(&mut store, &module, &imports).unwrap();

    let view = mem.view(&store);
    let slice = wasmer::WasmSlice::<u64>::new(&view, 1, 10).unwrap();
    let access = slice.access();

    assert!(matches!(
        access.err(),
        Some(wasmer::MemoryAccessError::UnalignedPointerRead)
    ))
}

#[test]
fn test_wasm_slice_new_rejects_out_of_bounds_ranges() {
    let mut store = Store::default();
    let memory = Memory::new(&mut store, MemoryType::new(1, Some(1), false)).unwrap();
    let view = memory.view(&store);

    let slice = wasmer::WasmSlice::<u64>::new(&view, 0, 8193);
    assert!(matches!(
        slice.err(),
        Some(wasmer::MemoryAccessError::HeapOutOfBounds)
    ));
}

#[test]
fn test_wasm_memory_size() {
    let mut store = Store::default();

    // Test once with not-shared memory...
    {
        let memory = Memory::new(&mut store, MemoryType::new(10, Some(65536), false)).unwrap();
        assert_eq!(memory.size(&store).0, 10);
        memory.grow(&mut store, wasmer::Pages(1)).unwrap();
        assert_eq!(memory.size(&store).0, 11);
    }

    // ... and once with shared memory, since JS has different representations for it
    {
        let memory = Memory::new(&mut store, MemoryType::new(10, Some(65536), true)).unwrap();
        assert_eq!(memory.size(&store).0, 10);
        memory.grow(&mut store, wasmer::Pages(1)).unwrap();
        assert_eq!(memory.size(&store).0, 11);
    }
}

#[cfg(feature = "js")]
#[wasm_bindgen_test]
fn js_compare_exchange_checks_word_and_refreshes_after_growth() {
    let mut store = Store::default();
    let memory = Memory::new(&mut store, MemoryType::new(1, Some(2), true)).unwrap();
    let view = memory.as_js().view(&store);
    assert_eq!(view.compare_exchange_u32(0, 0, u32::MAX).unwrap(), 0);
    assert_eq!(view.compare_exchange_u32(0, 1, 2).unwrap(), u32::MAX);
    assert_eq!(view.compare_exchange_u32(0, u32::MAX, 7).unwrap(), u32::MAX);
    assert!(view.compare_exchange_u32(1, 0, 1).is_err());
    assert!(view.compare_exchange_u32(65536, 0, 1).is_err());
    assert!(view.compare_exchange_u32(u64::MAX - 3, 0, 1).is_err());
    memory.grow(&mut store, 1).unwrap();
    assert_eq!(
        memory
            .as_js()
            .view(&store)
            .compare_exchange_u32(65536, 0, 42)
            .unwrap(),
        0
    );
    let mut bytes = [0; 4];
    memory.view(&store).read(65536, &mut bytes).unwrap();
    assert_eq!(u32::from_le_bytes(bytes), 42);
}

#[engine_test]
#[cfg_attr(feature = "v8", ignore = "v8 does not support shared memories")]
fn imported_memory_aliases_keep_identity_with_different_type_limits() -> Result<(), String> {
    for shared in [false, true] {
        let mut store = Store::default();
        let ty = MemoryType::new(2, Some(4), shared);
        let memory = Memory::new(&mut store, ty).unwrap();
        let other = Memory::new(&mut store, ty).unwrap();
        let module = Module::new(
            &store,
            format!(
                "(module (import \"host\" \"memory\" (memory 1 8 {})) (export \"memory\" (memory 0)))",
                if shared { "shared" } else { "" }
            ),
        )
        .unwrap();
        let instance = Instance::new(
            &mut store,
            &module,
            &imports! { "host" => { "memory" => memory.clone() } },
        )
        .unwrap();
        let alias = instance.exports.get_memory("memory").unwrap().clone();
        assert_eq!(
            alias, memory,
            "type annotations do not change memory identity"
        );
        assert_ne!(alias, other, "separate memories must stay distinct");
        drop(instance);
        memory.view(&store).write(0, b"same").unwrap();
        let mut bytes = [0; 4];
        alias.view(&store).read(0, &mut bytes).unwrap();
        assert_eq!(&bytes, b"same");
    }
    Ok(())
}

#[cfg(all(feature = "sys", feature = "compiler", feature = "wat"))]
mod grow_panic {
    use super::*;
    use std::{
        panic::{AssertUnwindSafe, catch_unwind, panic_any},
        process::{Child, Command},
        ptr::NonNull,
        sync::atomic::AtomicUsize,
        time::{Duration, Instant},
    };
    use wasmer::sys::vm::{
        LinearMemory, MemoryError, MemoryStyle, TableStyle, VMDefinitionLocation, VMMemory,
        VMMemoryDefinition, VMTable, VMTableDefinition,
    };
    use wasmer::sys::{BaseTunables, NativeEngineExt, Tunables};
    use wasmer::{Pages, TableType, Value};

    #[derive(Debug)]
    struct Payload;
    #[derive(Debug)]
    struct State {
        armed: AtomicBool,
        dropped: AtomicUsize,
        payload: Arc<Payload>,
    }
    struct GrowDrop(Arc<State>);
    impl Drop for GrowDrop {
        fn drop(&mut self) {
            self.0.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[derive(Debug)]
    struct PanicMemory {
        inner: VMMemory,
        state: Arc<State>,
    }
    impl LinearMemory for PanicMemory {
        fn ty(&self) -> MemoryType {
            self.inner.ty()
        }
        fn size(&self) -> Pages {
            self.inner.size()
        }
        fn style(&self) -> MemoryStyle {
            self.inner.style()
        }
        fn grow(&mut self, delta: Pages) -> Result<Pages, MemoryError> {
            if self.state.armed.swap(false, Ordering::SeqCst) {
                let _drop = GrowDrop(self.state.clone());
                panic_any(self.state.payload.clone());
            }
            self.inner.grow(delta)
        }
        fn vmmemory(&self) -> NonNull<VMMemoryDefinition> {
            self.inner.vmmemory()
        }
        fn try_clone(&self) -> Result<Box<dyn LinearMemory + Send + Sync>, MemoryError> {
            Ok(Box::new(Self {
                inner: self.inner.try_clone()?,
                state: self.state.clone(),
            }))
        }
        fn copy(&self) -> Result<Box<dyn LinearMemory + Send + Sync>, MemoryError> {
            Ok(Box::new(Self {
                inner: VMMemory::from_custom(self.inner.copy()?),
                state: self.state.clone(),
            }))
        }
    }
    struct PanicTunables {
        base: BaseTunables,
        state: Arc<State>,
    }
    impl PanicTunables {
        fn wrap(&self, inner: VMMemory) -> VMMemory {
            VMMemory::from_custom(Box::new(PanicMemory {
                inner,
                state: self.state.clone(),
            }) as Box<dyn LinearMemory + Send + Sync>)
        }
    }
    impl Tunables for PanicTunables {
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
            self.base
                .create_host_memory(ty, style)
                .map(|inner| self.wrap(inner))
        }
        unsafe fn create_vm_memory(
            &self,
            ty: &MemoryType,
            style: &MemoryStyle,
            location: VMDefinitionLocation<VMMemoryDefinition>,
        ) -> Result<VMMemory, MemoryError> {
            // SAFETY: forward the exact definition location and its storage owner.
            unsafe {
                self.base
                    .create_vm_memory(ty, style, location)
                    .map(|inner| self.wrap(inner))
            }
        }
        fn create_host_table(&self, ty: &TableType, style: &TableStyle) -> Result<VMTable, String> {
            self.base.create_host_table(ty, style)
        }
        unsafe fn create_vm_table(
            &self,
            ty: &TableType,
            style: &TableStyle,
            location: VMDefinitionLocation<VMTableDefinition>,
        ) -> Result<VMTable, String> {
            // SAFETY: forward the exact definition location and its storage owner.
            unsafe { self.base.create_vm_table(ty, style, location) }
        }
    }

    // Match the bounded owned-child native-call proofs in async_sync_borrow.rs.
    // An uncaught custom-memory panic must not abort the test harness.
    fn subprocess(test_name: &str) -> bool {
        const CHILD: &str = "WASMER_MEMORY_GROW_PANIC_CHILD";
        if std::env::var_os(CHILD).is_some() {
            return true;
        }
        struct OwnedChild(Child);
        impl Drop for OwnedChild {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = OwnedChild(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", test_name, "--nocapture"])
                .env(CHILD, "1")
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match child.0.try_wait() {
                Ok(Some(status)) => {
                    assert!(status.success(), "memory grow panic proof failed: {status}");
                    break;
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                result => panic!("memory grow panic proof did not finish: {result:?}"),
            }
        }
        false
    }

    fn proof(imported: bool) {
        for typed in [false, true] {
            let state = Arc::new(State {
                armed: AtomicBool::new(false),
                dropped: AtomicUsize::new(0),
                payload: Arc::new(Payload),
            });
            let mut store = Store::default();
            store.engine_mut().set_tunables(PanicTunables {
                base: BaseTunables::new(),
                state: state.clone(),
            });
            let declaration = if imported {
                "(import \"env\" \"memory\" (memory 1 3))"
            } else {
                "(memory 1 3)"
            };
            let module = Module::new(
                &store,
                format!(
                    r#"(module {declaration}
                (func (export "grow") (param i32) (result i32) local.get 0 memory.grow)
                (func (export "fresh") (result i32) i32.const 42))"#
                ),
            )
            .unwrap();
            let imports = if imported {
                let memory = Memory::new(&mut store, MemoryType::new(1, Some(3), false)).unwrap();
                imports! { "env" => { "memory" => memory } }
            } else {
                wasmer::Imports::new()
            };
            let instance = Instance::new(&mut store, &module, &imports).unwrap();
            state.armed.store(true, Ordering::SeqCst);
            let panic = catch_unwind(AssertUnwindSafe(|| {
                if typed {
                    instance
                        .exports
                        .get_typed_function::<i32, i32>(&store, "grow")
                        .unwrap()
                        .call(&mut store, 1)
                        .map(|_| ())
                } else {
                    instance
                        .exports
                        .get_function("grow")
                        .unwrap()
                        .call(&mut store, &[Value::I32(1)])
                        .map(|_| ())
                }
            }))
            .unwrap_err();
            let payload = panic.downcast::<Arc<Payload>>().unwrap();
            assert!(
                Arc::ptr_eq(&payload, &state.payload),
                "memory grow replaced the original panic"
            );
            assert_eq!(
                state.dropped.load(Ordering::SeqCst),
                1,
                "grow locals were not destroyed before panic transport"
            );
            let fresh = instance
                .exports
                .get_typed_function::<(), i32>(&store, "fresh")
                .unwrap();
            assert_eq!(fresh.call(&mut store).unwrap(), 42);
            let grow = instance
                .exports
                .get_typed_function::<i32, i32>(&store, "grow")
                .unwrap();
            assert_eq!(grow.call(&mut store, 1).unwrap(), 1);
            assert_eq!(grow.call(&mut store, 10).unwrap(), -1);
            assert_eq!(fresh.call(&mut store).unwrap(), 42);
            drop(store);
            assert_eq!(state.dropped.load(Ordering::SeqCst), 1);
        }
    }
    #[test]
    fn defined_memory_grow_preserves_a_custom_memory_panic() {
        if subprocess("grow_panic::defined_memory_grow_preserves_a_custom_memory_panic") {
            proof(false);
        }
    }
    #[test]
    fn imported_memory_grow_preserves_a_custom_memory_panic() {
        if subprocess("grow_panic::imported_memory_grow_preserves_a_custom_memory_panic") {
            proof(true);
        }
    }
}
