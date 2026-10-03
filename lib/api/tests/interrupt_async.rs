#![cfg(all(
    unix,
    feature = "experimental-host-interrupt",
    feature = "experimental-async"
))]

// TODO: tests for recursive function calls across different stores

use std::{
    sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    thread,
    time::Duration,
};

use anyhow::Result;
use futures::task::noop_waker;
use wasmer::{Function, Instance, Module, Store, imports};
use wasmer_vm::TrapCode;

const WAT: &str = r#"
    (module
      (import "env" "f" (func $f))
      (func (export "async")
        call $f
      )
    )"#;

#[test]
fn async_function_can_be_interrupted() -> Result<()> {
    let interrupter_slot = Arc::new(Mutex::new(None));
    let barrier = Arc::new(Barrier::new(2));

    let worker = thread::spawn({
        let barrier = barrier.clone();
        let interrupter_slot = interrupter_slot.clone();
        move || {
            let wasm = wat::parse_str(WAT)?;

            let mut store = Store::default();
            let interrupter = store.interrupter();
            interrupter_slot.lock().unwrap().replace(interrupter);
            let module = Module::new(&store, &wasm)?;

            let f = Function::new_typed_async(&mut store, async || {
                std::future::pending::<()>().await;
            });
            let imports = imports! {
                "env" => {
                    "f" => f
                }
            };

            let instance = Instance::new(&mut store, &module, &imports)?;
            let f = instance
                .exports
                .get_typed_function::<(), ()>(&store, "async")?;

            let store_async = store.into_async();

            barrier.wait();
            let res = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(f.call_async(&store_async));
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
    let result = worker.join().unwrap().unwrap().unwrap_err();
    assert_eq!(result.to_trap().unwrap(), TrapCode::HostInterrupt);

    Ok(())
}

#[test]
fn all_futures_from_store_are_interrupted() -> Result<()> {
    let wasm = wat::parse_str(WAT)?;

    let mut store = Store::default();
    let interrupter = store.interrupter();
    let module = Module::new(&store, &wasm)?;

    let pending_count = Arc::new(AtomicU32::new(0));

    let f = Function::new_typed_async(&mut store, {
        let pending_count = pending_count.clone();
        move || {
            let pending_count = pending_count.clone();
            async move {
                pending_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::future::pending::<()>().await;
            }
        }
    });
    let imports = imports! {
        "env" => {
            "f" => f
        }
    };

    let instance = Instance::new(&mut store, &module, &imports)?;
    let f = instance
        .exports
        .get_typed_function::<(), ()>(&store, "async")?;

    let store_async = store.into_async();
    let mut futures = [
        Box::pin(f.call_async(&store_async)),
        Box::pin(f.call_async(&store_async)),
        Box::pin(f.call_async(&store_async)),
        Box::pin(f.call_async(&store_async)),
    ];

    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    // All futures should be in pending state for now
    for f in &mut futures {
        assert!(f.as_mut().poll(&mut cx).is_pending());
    }

    // Since we polled everything already, there should be 4 calls to the imported function
    assert_eq!(pending_count.load(std::sync::atomic::Ordering::SeqCst), 4);

    interrupter.interrupt();

    for f in &mut futures {
        let result = f.as_mut().poll(&mut cx);
        let Poll::Ready(result) = result else {
            panic!("Futures should be ready by now")
        };
        assert_eq!(
            result.unwrap_err().to_trap().unwrap(),
            TrapCode::HostInterrupt
        );
    }

    Ok(())
}

// The async flavour of `interrupt_during_host_function_return` in
// `interrupt.rs`, which is how WASIX runs guests: the store context belongs to
// `AsyncCallFuture` here, and a leaked borrow made its guard panic during
// unwinding, aborting the process.
#[test]
fn async_interrupt_during_host_function_return() -> Result<()> {
    let wasm = wat::parse_str(
        r#"
        (module
          (import "env" "f" (func $f))
          (func (export "run")
            loop
              call $f
              br 0
            end
          )
        )"#,
    )?;
    let mut store = Store::default();
    let module = Module::new(&store, &wasm)?;
    let f = Function::new_typed(&mut store, || {});
    let imports = imports! { "env" => { "f" => f } };
    let instance = Instance::new(&mut store, &module, &imports)?;
    let run = instance
        .exports
        .get_typed_function::<(), ()>(&store, "run")?;

    let store_id = store.id();
    let store_async = store.into_async();
    let round = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    // Interrupts once per round, after a pseudo-random delay.
    let interrupter = thread::spawn({
        let round = round.clone();
        let stop = stop.clone();
        move || {
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
                if wasmer_vm::interrupt_registry::interrupt(store_id).is_ok() {
                    interrupted_round = current;
                }
            }
        }
    });
    for i in 1..=100_000 {
        round.store(i, Ordering::Release);
        let err = futures::executor::block_on(run.call_async(&store_async)).unwrap_err();
        assert_eq!(err.to_trap(), Some(TrapCode::HostInterrupt));
    }
    stop.store(true, Ordering::Relaxed);
    interrupter.join().unwrap();

    Ok(())
}
