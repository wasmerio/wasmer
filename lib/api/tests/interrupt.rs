#![cfg(all(unix, feature = "experimental-host-interrupt"))]

// TODO: tests for recursive function calls across different stores

use std::{
    sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use anyhow::Result;
use wasmer::{
    AsStoreMut, Exception, Function, FunctionEnv, Instance, Memory, MemoryLocation, MemoryType,
    Module, RuntimeError, Store, Tag, imports,
};
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
    test_interruptible(INFINITE_LOOP_WAT)
}

#[test]
fn test_interrupt_memory_wait() -> Result<()> {
    test_interruptible(INFINITE_ATOMIC_WAIT_WAT)
}

#[test]
fn non_interrupt_lib_trap_keeps_guest_trace() -> Result<()> {
    let mut store = Store::default();
    let module = Module::new(
        &store,
        r#"(module
          (memory 1 1 shared)
          (func (export "trap")
            i32.const 1
            i32.const 0
            i64.const 0
            memory.atomic.wait32
            drop))"#,
    )?;
    let instance = Instance::new(&mut store, &module, &imports! {})?;
    let trap = instance
        .exports
        .get_typed_function::<(), ()>(&store, "trap")?
        .call(&mut store)
        .unwrap_err();
    assert!(!trap.trace().is_empty());
    assert_eq!(trap.to_trap(), Some(TrapCode::UnalignedAtomic));
    Ok(())
}

#[test]
fn concurrent_atomic_disable_and_store_interrupt_do_not_strand_runtime_locks() -> Result<()> {
    const ITERATIONS: usize = 32;
    const WAT: &str = r#"
        (module
          (import "test" "started" (func $started))
          (memory (export "memory") 1 1 shared)
          (func (export "wait")
            call $started
            i32.const 0
            i32.const 0
            i64.const -1
            memory.atomic.wait32
            drop))"#;

    for _ in 0..ITERATIONS {
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (control_tx, control_rx) = std::sync::mpsc::channel();
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let result = (|| -> Result<Result<(), RuntimeError>> {
                let mut store = Store::default();
                let interrupter = store.interrupter();
                let module = Module::new(&store, WAT)?;
                let started_tx = Mutex::new(Some(started_tx));
                let started = Function::new_typed(&mut store, move || {
                    started_tx.lock().unwrap().take().unwrap().send(()).unwrap();
                });
                let instance = Instance::new(
                    &mut store,
                    &module,
                    &imports! { "test" => { "started" => started } },
                )?;
                let memory = instance
                    .exports
                    .get_memory("memory")?
                    .as_shared(&store)
                    .unwrap();
                let wait = instance
                    .exports
                    .get_typed_function::<(), ()>(&store, "wait")?;
                control_tx.send((interrupter, memory)).unwrap();
                Ok(wait.call(&mut store))
            })();
            result_tx.send(result).unwrap();
        });

        let (interrupter, memory) = control_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let race = Arc::new(Barrier::new(2));
        let disable = thread::spawn({
            let race = race.clone();
            move || {
                race.wait();
                memory.disable_atomics().unwrap();
            }
        });
        race.wait();
        interrupter.interrupt();
        disable.join().unwrap();

        let result = result_rx.recv_timeout(Duration::from_secs(5)).unwrap()?;
        worker.join().unwrap();
        let result = result.unwrap_err();
        assert_eq!(result.to_trap(), Some(TrapCode::HostInterrupt));
    }

    // Module registration takes FRAME_INFO's write side. Reaching this point
    // repeatedly proves that no interrupted backtrace retained its global lock.
    let store = Store::default();
    Module::new(&store, INFINITE_LOOP_WAT)?;
    Ok(())
}

#[test]
fn interrupting_one_store_does_not_notify_another_atomic_waiter() -> Result<()> {
    const WAT: &str = r#"
        (module
          (import "test" "started" (func $started))
          (import "test" "memory" (memory 1 1 shared))
          (func (export "wait")
            call $started
            i32.const 0
            i32.const 0
            i64.const -1
            memory.atomic.wait32
            drop))"#;

    let mut owner = Store::default();
    let memory = Memory::new(&mut owner, MemoryType::new(1, Some(1), true))?
        .as_shared(&owner)
        .unwrap();

    let spawn_waiter = |memory: wasmer::SharedMemory| {
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (control_tx, control_rx) = std::sync::mpsc::channel();
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let result = (|| -> Result<Result<(), RuntimeError>> {
                let mut store = Store::default();
                let interrupter = store.interrupter();
                let module = Module::new(&store, WAT)?;
                let attached = memory.attach(&mut store);
                let started_tx = Mutex::new(Some(started_tx));
                let started = Function::new_typed(&mut store, move || {
                    started_tx.lock().unwrap().take().unwrap().send(()).unwrap();
                });
                let instance = Instance::new(
                    &mut store,
                    &module,
                    &imports! { "test" => { "started" => started, "memory" => attached } },
                )?;
                let wait = instance
                    .exports
                    .get_typed_function::<(), ()>(&store, "wait")?;
                control_tx.send(interrupter).unwrap();
                Ok(wait.call(&mut store))
            })();
            result_tx.send(result).unwrap();
        });
        (started_rx, control_rx, result_rx, worker)
    };

    let (started_a, control_a, result_a, worker_a) = spawn_waiter(memory.clone());
    let (started_b, _control_b, result_b, worker_b) = spawn_waiter(memory.clone());
    let interrupter_a = control_a.recv_timeout(Duration::from_secs(5)).unwrap();
    started_a.recv_timeout(Duration::from_secs(5)).unwrap();
    started_b.recv_timeout(Duration::from_secs(5)).unwrap();

    interrupter_a.interrupt();
    let interrupted = result_a.recv_timeout(Duration::from_secs(5)).unwrap()?;
    assert_eq!(
        interrupted.unwrap_err().to_trap(),
        Some(TrapCode::HostInterrupt)
    );
    assert!(result_b.recv_timeout(Duration::from_millis(100)).is_err());

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if memory.notify(MemoryLocation::new_32(0), 1)? == 1 {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        thread::yield_now();
    }
    result_b.recv_timeout(Duration::from_secs(5)).unwrap()??;
    worker_a.join().unwrap();
    worker_b.join().unwrap();
    Ok(())
}

// TODO: update/fix this as we implement more of the feature
fn test_interruptible(wat: &'static str) -> Result<()> {
    let barrier = Arc::new(Barrier::new(2));
    let interrupter_slot = Arc::new(Mutex::new(None));

    let worker = thread::spawn({
        let barrier = barrier.clone();
        let interrupter_slot = interrupter_slot.clone();
        move || {
            let wasm = wat::parse_str(wat)?;

            let mut store = Store::default();
            let interrupter = store.interrupter();
            interrupter_slot.lock().unwrap().replace(interrupter);
            let module = Module::new(&store, &wasm)?;
            let imports = imports! {};
            let instance = Instance::new(&mut store, &module, &imports)?;
            let f = instance
                .exports
                .get_typed_function::<(), ()>(&store, "infinite")?;

            barrier.wait();
            anyhow::Ok(f.call(&mut store))
        }
    });

    barrier.wait();
    // Make absolutely sure the function is running WASM when we raise the signal
    thread::sleep(Duration::from_millis(500));

    interrupter_slot
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .interrupt();
    let result = worker.join().unwrap().unwrap().unwrap_err();
    assert_eq!(result.to_trap().unwrap(), TrapCode::HostInterrupt);

    Ok(())
}

#[test]
fn correct_store_is_interrupted_only() -> Result<()> {
    let barrier = Arc::new(Barrier::new(2));
    let finished = Arc::new(AtomicBool::new(false));
    let interrupter_slot = Arc::new(Mutex::new(None));

    let worker = thread::spawn({
        let barrier = barrier.clone();
        let finished = finished.clone();
        let interrupter_slot = interrupter_slot.clone();
        move || {
            let wasm = wat::parse_str(INFINITE_LOOP_WAT)?;

            let mut store = Store::default();
            let interrupter = store.interrupter();
            interrupter_slot.lock().unwrap().replace(interrupter);
            let module = Module::new(&store, &wasm)?;
            let imports = imports! {};
            let instance = Instance::new(&mut store, &module, &imports)?;
            let f = instance
                .exports
                .get_typed_function::<(), ()>(&store, "infinite")?;

            barrier.wait();
            let res = f.call(&mut store);
            finished.store(true, Ordering::SeqCst);
            anyhow::Ok(res)
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
    assert!(!finished.load(Ordering::SeqCst));

    interrupter_slot
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .interrupt();
    let result = worker.join().unwrap().unwrap().unwrap_err();
    assert!(finished.load(Ordering::SeqCst));
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

#[test]
fn imported_functions_are_interrupted_correctly() -> Result<()> {
    test_imported_function_interrupt(|store, rx| {
        Function::new_typed(store, move || {
            rx.recv().unwrap();
        })
    })
}

#[test]
fn imported_functions_are_interrupted_if_exception_is_thrown() -> Result<()> {
    test_imported_function_interrupt(|store, rx| {
        let env = FunctionEnv::new(store, ());
        Function::new_typed_with_env(store, &env, move |mut env: wasmer::FunctionEnvMut<_>| {
            rx.recv().unwrap();
            let mut store = env.as_store_mut();
            let tag = Tag::new(&mut store, []);
            let exc = Exception::new(&mut store, &tag, &[]);
            Result::<(), _>::Err(RuntimeError::exception(&store, exc))
        })
    })
}

fn test_imported_function_interrupt<F>(build_imported_function: F) -> Result<()>
where
    F: (FnOnce(&mut Store, crossbeam_channel::Receiver<()>) -> Function) + Send + Sync + 'static,
{
    // std::mpsc receivers are not Sync, so we need something else here
    let (tx, rx) = crossbeam_channel::bounded(1);
    let interrupter_slot = Arc::new(Mutex::new(None));

    let barrier = Arc::new(Barrier::new(2));
    let finished = Arc::new(AtomicBool::new(false));

    let worker = thread::spawn({
        let barrier = barrier.clone();
        let finished = finished.clone();
        let interrupter_slot = interrupter_slot.clone();
        move || {
            let wasm = wat::parse_str(
                r#"
                (module
                  (import "env" "f" (func $f))
                  (func (export "infinite")
                    call $f
                  )
                )"#,
            )?;

            let mut store = Store::default();
            let interrupter = store.interrupter();
            interrupter_slot.lock().unwrap().replace(interrupter);
            let module = Module::new(&store, &wasm)?;

            let f = build_imported_function(&mut store, rx);
            let imports = imports! {
                "env" => {
                    "f" => f
                }
            };

            let instance = Instance::new(&mut store, &module, &imports)?;
            let f = instance
                .exports
                .get_typed_function::<(), ()>(&store, "infinite")?;

            barrier.wait();
            let res = f.call(&mut store);
            finished.store(true, Ordering::SeqCst);

            anyhow::Ok(res)
        }
    });

    barrier.wait();
    // Make absolutely sure the function is waiting on the channel when we raise the signal
    thread::sleep(Duration::from_millis(500));

    interrupter_slot
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .interrupt();
    thread::sleep(Duration::from_millis(100));

    // At this point, we're still waiting in the imported function, which can *not* be
    // interrupted.
    assert!(!finished.load(Ordering::SeqCst));

    // Now send a message to the channel. This should unblock the imported function,
    // which will return control to the WASM code. Since the store was already interrupted,
    // this should result in the correct trap being raised.
    tx.send(()).unwrap();

    let result = worker.join().unwrap().unwrap().unwrap_err();
    assert!(finished.load(Ordering::SeqCst));
    assert_eq!(result.to_trap().unwrap(), TrapCode::HostInterrupt);

    Ok(())
}
