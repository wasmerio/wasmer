use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use wasmer::{
    imports, Instance, Memory, MemoryAccessError, MemoryLocation, MemoryType, Module, Store,
    WasmSlice,
};

#[test]
#[cfg_attr(feature = "wamr", ignore = "wamr ignores import memories")]
#[cfg_attr(feature = "wasmi", ignore = "wasmi does not support threads")]
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
    std::thread::spawn(move || loop {
        if mem2.notify(MemoryLocation::new_32(10), 1).unwrap() > 0 {
            break;
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
fn test_shared_memory_disable_atomics() {
    use wasmer::AtomicsError;

    let mut store = Store::default();
    let mem = Memory::new(&mut store, MemoryType::new(10, Some(65536), true)).unwrap();

    let mem = mem.as_shared(&store).unwrap();
    mem.disable_atomics().unwrap();

    let err = mem.wait(MemoryLocation::new_32(1), None).unwrap_err();
    assert_eq!(err, AtomicsError::AtomicsDisabled);
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
    let slice = WasmSlice::<u64>::new(&view, 1, 10).unwrap();
    let access = slice.access();

    assert!(matches!(
        access.err(),
        Some(MemoryAccessError::UnalignedPointerRead)
    ))
}
