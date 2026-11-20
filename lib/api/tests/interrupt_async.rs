#![cfg(all(
    unix,
    feature = "experimental-host-interrupt",
    feature = "experimental-async"
))]

// TODO: tests for recursive function calls across different stores

use std::{
    sync::{Arc, Barrier, Mutex, atomic::AtomicU32},
    task::{Context, Poll},
    thread,
    time::Duration,
};

use anyhow::Result;
use futures::task::noop_waker;
use wasmer::{Function, Instance, Module, Store, imports};
use wasmer_vm::{TrapCode, interrupt_registry};

const WAT: &str = r#"
    (module
      (import "env" "f" (func $f))
      (func (export "async")
        call $f
      )
      (func (export "plain") (result i32)
        i32.const 1
      )
    )"#;

/// A store whose export `async` waits for good, and its export `plain`.
/// Untyped: the future of a typed call drops the untyped call it awaits as
/// soon as that returns, and a caller keeps an untyped one as long as it likes.
fn waiting_guest() -> Result<(Store, Function, Function)> {
    let wasm = wat::parse_str(WAT)?;
    let mut store = Store::default();
    let module = Module::new(&store, &wasm)?;
    let f = Function::new_typed_async(&mut store, async || {
        std::future::pending::<()>().await;
    });
    let instance = Instance::new(&mut store, &module, &imports! { "env" => { "f" => f } })?;
    let waits = instance.exports.get_function("async")?.clone();
    let plain = instance.exports.get_function("plain")?.clone();
    Ok((store, waits, plain))
}

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

/// A call forgotten while it is suspended keeps its store's interrupt state,
/// aimed at its thread. Once the thread is gone, an interrupt does not signal
/// it: the store is not running anywhere.
#[test]
fn a_forgotten_call_on_an_exited_thread_is_not_signalled() -> Result<()> {
    let id = thread::spawn(|| -> Result<_> {
        let (store, waits, _) = waiting_guest()?;
        let id = store.id();
        let store = store.into_async();
        let mut call = Box::pin(waits.call_async(&store, vec![]));
        assert!(
            call.as_mut()
                .poll(&mut Context::from_waker(&noop_waker()))
                .is_pending()
        );
        std::mem::forget(call);
        Ok(id)
    })
    .join()
    .unwrap()?;
    assert!(matches!(
        interrupt_registry::interrupt(id),
        Err(interrupt_registry::InterruptError::StoreNotRunning)
    ));
    Ok(())
}
