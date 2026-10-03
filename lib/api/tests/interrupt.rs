#![cfg(all(unix, feature = "experimental-host-interrupt"))]

// TODO: tests for recursive function calls across different stores

use std::{
    sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use anyhow::Result;
use wasmer::{
    AsStoreMut, Exception, Function, FunctionEnv, Instance, Module, RuntimeError, Store, Tag,
    imports,
};
use wasmer_types::StoreId;
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

const HOST_CALL_LOOP_WAT: &str = r#"
    (module
      (import "env" "f" (func $f))
      (func (export "run")
        loop
          call $f
          br 0
        end
      )
    )"#;

/// Interrupts `store_id` once per round, after a pseudo-random delay, so the
/// interrupts land all over the guest's host-call loop. Bump `round` before
/// each call and set `stop` when done.
fn spawn_round_interrupter(
    store_id: StoreId,
    round: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut interrupted_round = 0;
        let mut rng = 0x9e37_79b9_u32;
        while !stop.load(Ordering::Relaxed) {
            let current = round.load(Ordering::Acquire);
            if current == interrupted_round {
                std::hint::spin_loop();
                continue;
            }
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            for _ in 0..rng % 1024 {
                std::hint::spin_loop();
            }
            // Fails while the call has not started yet; try again then.
            if wasmer_vm::interrupt_registry::interrupt(store_id).is_ok() {
                interrupted_round = current;
            }
        }
    })
}

// An interrupt that lands while a host function's trampoline is back on the
// Wasm stack must not abandon anything the trampoline holds there: a leaked
// store-context borrow made the next call on the thread panic, and abort.
#[test]
fn interrupt_during_host_function_return() -> Result<()> {
    let wasm = wat::parse_str(HOST_CALL_LOOP_WAT)?;
    let mut store = Store::default();
    let module = Module::new(&store, &wasm)?;
    let f = Function::new_typed(&mut store, || {});
    let imports = imports! { "env" => { "f" => f } };
    let instance = Instance::new(&mut store, &module, &imports)?;
    let run = instance
        .exports
        .get_typed_function::<(), ()>(&store, "run")?;

    let round = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let interrupter = spawn_round_interrupter(store.id(), round.clone(), stop.clone());
    for i in 1..=100_000 {
        round.store(i, Ordering::Release);
        let err = run.call(&mut store).unwrap_err();
        assert_eq!(err.to_trap(), Some(TrapCode::HostInterrupt));
    }
    stop.store(true, Ordering::Relaxed);
    interrupter.join().unwrap();

    Ok(())
}
