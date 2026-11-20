#![cfg(all(
    unix,
    feature = "experimental-host-interrupt",
    not(feature = "baremetal")
))]

use std::{
    sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;
use wasmer::{
    AsStoreMut, Exception, Function, FunctionEnv, Instance, InterruptCallWrapper, Module,
    RuntimeError, Store, Tag, WasmCall, imports,
};
use wasmer_vm::TrapCode;

const CALL_IMPORT_WAT: &str = r#"
    (module
      (import "env" "f" (func $f))
      (func (export "run")
        call $f
      )
    )"#;

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
fn plain_calls_are_not_interrupted() -> Result<()> {
    let mut store = Store::default();
    let interrupter = store.interrupter();
    let probe = Function::new_typed(&mut store, {
        let interrupter = interrupter.clone();
        move || interrupter.interrupt()
    });
    let module = Module::new(&store, wat::parse_str(CALL_IMPORT_WAT)?)?;
    let instance = Instance::new(&mut store, &module, &imports! { "env" => { "f" => probe } })?;
    let run = instance.exports.get_function("run")?;
    run.call(&mut store, &[])?;
    run.call_wrapped(
        &mut store,
        &mut |call: &mut WasmCall<'_>| call.invoke(),
        &[],
    )?;
    run.typed::<(), ()>(&store)?.call(&mut store)?;

    let mut wrapper = InterruptCallWrapper::new(interrupter);
    let error = run.call_wrapped(&mut store, &mut wrapper, &[]).unwrap_err();
    assert_eq!(error.to_trap(), Some(TrapCode::HostInterrupt));
    run.call(&mut store, &[])?;
    Ok(())
}

#[test]
fn plain_module_start_is_not_interrupted() -> Result<()> {
    let mut store = Store::default();
    let interrupter = store.interrupter();
    let interrupt = Function::new_typed(&mut store, move || interrupter.interrupt());
    let module = Module::new(
        &store,
        wat::parse_str(
            r#"(module
                (import "env" "interrupt" (func $interrupt))
                (func $start call $interrupt)
                (start $start)
            )"#,
        )?,
    )?;
    let imports = imports! { "env" => { "interrupt" => interrupt } };

    Instance::new(&mut store, &module, &imports)?;
    Ok(())
}

#[test]
fn interrupt_wrapper_checks_before_guest_resumes_after_host_call() -> Result<()> {
    let mut store = Store::default();
    let interrupter = store.interrupter();
    let interrupt = Function::new_typed(&mut store, {
        let interrupter = interrupter.clone();
        move || interrupter.interrupt()
    });
    let resumed = Arc::new(AtomicBool::new(false));
    let after = Function::new_typed(&mut store, {
        let resumed = resumed.clone();
        move || resumed.store(true, Ordering::Relaxed)
    });
    let module = Module::new(
        &store,
        wat::parse_str(
            r#"(module
                (import "env" "interrupt" (func $interrupt))
                (import "env" "after" (func $after))
                (func (export "run") call $interrupt call $after)
            )"#,
        )?,
    )?;
    let imports = imports! { "env" => { "interrupt" => interrupt, "after" => after } };
    let instance = Instance::new(&mut store, &module, &imports)?;
    let run = instance.exports.get_function("run")?;
    let mut wrapper = InterruptCallWrapper::new(interrupter);
    let error = run.call_wrapped(&mut store, &mut wrapper, &[]).unwrap_err();
    assert_eq!(error.to_trap(), Some(TrapCode::HostInterrupt));
    assert!(!resumed.load(Ordering::Relaxed));
    Ok(())
}

#[test]
fn interrupt_wrapper_composes_with_reentry() -> Result<()> {
    let mut store = Store::default();
    let interrupter = store.interrupter();
    let entries = Arc::new(AtomicU32::new(0));
    let probe = Function::new_typed(&mut store, {
        let interrupter = interrupter.clone();
        let entries = entries.clone();
        move || {
            if entries.fetch_add(1, Ordering::Relaxed) >= 1 {
                interrupter.interrupt();
            }
        }
    });
    let module = Module::new(&store, wat::parse_str(CALL_IMPORT_WAT)?)?;
    let instance = Instance::new(&mut store, &module, &imports! { "env" => { "f" => probe } })?;
    let run = instance
        .exports
        .get_typed_function::<(), ()>(&store, "run")?;
    let mut wrapper =
        InterruptCallWrapper::new(interrupter).with_wrapper(|call: &mut WasmCall<'_>| {
            call.invoke()?;
            call.invoke()
        });
    let error = run.call_wrapped(&mut store, &mut wrapper).unwrap_err();
    assert_eq!(error.to_trap(), Some(TrapCode::HostInterrupt));
    assert_eq!(entries.load(Ordering::Relaxed), 2);
    run.call(&mut store)?;
    assert_eq!(entries.load(Ordering::Relaxed), 3);
    Ok(())
}

#[test]
fn interrupt_wrapper_rejects_another_store() -> Result<()> {
    let mut store = Store::default();
    let other_store = Store::default();
    let mut wrapper = InterruptCallWrapper::new(other_store.interrupter());
    let module = Module::new(&store, wat::parse_str(r#"(module (func (export "run")))"#)?)?;
    let instance = Instance::new(&mut store, &module, &imports! {})?;
    let run = instance.exports.get_function("run")?;
    assert!(run.call_wrapped(&mut store, &mut wrapper, &[]).is_err());
    Ok(())
}

#[test]
fn interrupting_outer_store_does_not_interrupt_nested_plain_store() -> Result<()> {
    const NESTED_WAIT_WAT: &str = r#"
        (module
          (import "env" "entered" (func $entered))
          (memory 1 1 shared)
          (func (export "wait") (result i32)
            call $entered
            i32.const 0
            i32.const 0
            i64.const 500000000
            memory.atomic.wait32
          )
        )"#;

    let (entered_tx, entered_rx) = crossbeam_channel::bounded(1);
    let (done_tx, done_rx) = crossbeam_channel::bounded(1);
    let worker = thread::spawn(move || -> Result<()> {
        let mut store = Store::default();
        let interrupter = store.interrupter();
        let mut wrapper = InterruptCallWrapper::new(interrupter.clone());
        let nested_wasm = wat::parse_str(NESTED_WAIT_WAT)?;
        let nested = Function::new_typed(&mut store, move || {
            let mut inner_store = Store::default();
            let module = Module::new(&inner_store, &nested_wasm).unwrap();
            let entered = Function::new_typed(&mut inner_store, {
                let interrupter = interrupter.clone();
                let entered_tx = entered_tx.clone();
                move || entered_tx.send(interrupter.clone()).unwrap()
            });
            let instance = Instance::new(
                &mut inner_store,
                &module,
                &imports! { "env" => { "entered" => entered } },
            )
            .unwrap();
            let wait = instance
                .exports
                .get_typed_function::<(), i32>(&inner_store, "wait")
                .unwrap();

            // The inner wait must return "timed out" without trapping.
            assert_eq!(wait.call(&mut inner_store).unwrap(), 2);
        });
        let module = Module::new(&store, wat::parse_str(CALL_IMPORT_WAT)?)?;
        let instance = Instance::new(
            &mut store,
            &module,
            &imports! { "env" => { "f" => nested } },
        )?;
        let run = instance
            .exports
            .get_typed_function::<(), ()>(&store, "run")?;
        done_tx
            .send(
                run.call_wrapped(&mut store, &mut wrapper)
                    .map_err(|err| err.to_trap()),
            )
            .unwrap();
        Ok(())
    });

    let timeout = Duration::from_secs(5);
    let interrupter = entered_rx.recv_timeout(timeout)?;
    let deadline = Instant::now() + timeout;
    // Retry after the inner host import returns so the signal reaches the Wasm stack.
    let result = loop {
        interrupter.interrupt();
        match done_rx.recv_timeout(Duration::from_millis(10)) {
            Ok(result) => break result,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
            Err(err) => return Err(err.into()),
        }
    };
    worker.join().unwrap()?;
    assert_eq!(result, Err(Some(TrapCode::HostInterrupt)));
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
            let mut wrapper = InterruptCallWrapper::new(interrupter.clone());
            interrupter_slot.lock().unwrap().replace(interrupter);
            let module = Module::new(&store, &wasm)?;
            let imports = imports! {};
            let instance = Instance::new(&mut store, &module, &imports)?;
            let f = instance
                .exports
                .get_typed_function::<(), ()>(&store, "infinite")?;

            barrier.wait();
            anyhow::Ok(f.call_wrapped(&mut store, &mut wrapper))
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
            let mut wrapper = InterruptCallWrapper::new(interrupter.clone());
            interrupter_slot.lock().unwrap().replace(interrupter);
            let module = Module::new(&store, &wasm)?;
            let imports = imports! {};
            let instance = Instance::new(&mut store, &module, &imports)?;
            let f = instance
                .exports
                .get_typed_function::<(), ()>(&store, "infinite")?;

            barrier.wait();
            let res = f.call_wrapped(&mut store, &mut wrapper);
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
            let mut wrapper = InterruptCallWrapper::new(interrupter.clone());
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
            let res = f.call_wrapped(&mut store, &mut wrapper);
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
