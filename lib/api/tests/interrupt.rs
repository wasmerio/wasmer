#![cfg(unix)]

// TODO: tests for recursive function calls across different stores

use std::{
    sync::{
        Arc, Barrier,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use anyhow::Result;
use wasmer::{Instance, Module, Store, imports};
use wasmer_vm::TrapCode;

const INFINITE_LOOP_WAT: &str = r#"
    (module
      (func (export "infinite")
        loop
          br 0
        end
      )
    )"#;

// TODO: VMOwnedMemory doesn't support memory.atomic.wait, otherwise the
// memory here doesn't need to be shared
const INFINITE_ATOMIC_WAIT_WAT: &str = r#"
    (module
      (memory 1 1 shared)
      (func (export "infinite")
        i32.const 0
        i32.const 0
        i64.const -1
        memory.atomic.wait32
        drop
      )
    )"#;

#[test]
fn test_interrupt_hot_loop() -> Result<()> {
    test_interruptable(INFINITE_LOOP_WAT)
}

#[test]
fn test_interrupt_memory_wait() -> Result<()> {
    test_interruptable(INFINITE_ATOMIC_WAIT_WAT)
}

// TODO: update/fix this as we implement more of the feature
fn test_interruptable(wat: &str) -> Result<()> {
    let wasm = wat::parse_str(wat)?;

    let mut store = Store::default();
    let interrupter = store.interrupter();
    let module = Module::new(&store, &wasm)?;
    let imports = imports! {};
    let instance = Instance::new(&mut store, &module, &imports)?;
    let f = instance
        .exports
        .get_typed_function::<(), ()>(&store, "infinite")?;

    let barrier = Arc::new(Barrier::new(2));

    let worker = thread::spawn({
        let barrier = barrier.clone();
        move || {
            barrier.wait();
            f.call(&mut store)
        }
    });

    barrier.wait();
    // Make absolutely sure the function is running WASM when we raise the signal
    thread::sleep(Duration::from_millis(500));

    interrupter.interrupt();
    let result = worker.join().unwrap().unwrap_err();
    assert_eq!(result.to_trap().unwrap(), TrapCode::HostInterrupt);

    Ok(())
}

#[test]
fn correct_store_is_interrupted_only() -> Result<()> {
    let wasm = wat::parse_str(INFINITE_LOOP_WAT)?;

    let mut store = Store::default();
    let interrupter = store.interrupter();
    let module = Module::new(&store, &wasm)?;
    let imports = imports! {};
    let instance = Instance::new(&mut store, &module, &imports)?;
    let f = instance
        .exports
        .get_typed_function::<(), ()>(&store, "infinite")?;

    let barrier = Arc::new(Barrier::new(2));
    let finished = Arc::new(AtomicBool::new(false));

    let worker = thread::spawn({
        let barrier = barrier.clone();
        let finished = finished.clone();
        move || {
            barrier.wait();
            let res = f.call(&mut store);
            finished.store(true, Ordering::SeqCst);
            res
        }
    });

    let store2 = Store::default();
    let interrupter2 = store2.interrupter();

    barrier.wait();
    // Make absolutely sure the function is running WASM when we raise the signal
    thread::sleep(Duration::from_millis(500));

    // Interrupt store2; this should have no effect
    interrupter2.interrupt();
    // Joining at this point will deadlock, wait for some time instead...
    thread::sleep(Duration::from_millis(500));
    // ... and make sure the code wasn't interrupted by checking the atomic
    assert_eq!(finished.load(Ordering::SeqCst), false);

    interrupter.interrupt();
    let result = worker.join().unwrap().unwrap_err();
    assert_eq!(finished.load(Ordering::SeqCst), true);
    assert_eq!(result.to_trap().unwrap(), TrapCode::HostInterrupt);

    Ok(())
}

#[test]
fn interrupted_store_cant_be_entered_again() -> Result<()> {
    // It's important to build an actual Store here so that initialization
    // logic is run and the signal handler is registered
    let store = Store::default();
    let store_id = store.id();

    let interrupt_guard = wasmer_vm::interrupt_registry::install(store_id)?;
    wasmer_vm::interrupt_registry::interrupt(store_id)?;
    assert!(matches!(
        wasmer_vm::interrupt_registry::install(store_id),
        Err(wasmer_vm::interrupt_registry::InstallError::AlreadyInterrupted)
    ));

    drop(interrupt_guard);

    Ok(())
}
