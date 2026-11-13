#![cfg(all(
    unix,
    feature = "experimental-host-interrupt",
    feature = "experimental-async",
    not(feature = "baremetal")
))]

// TODO: tests for recursive function calls across different stores

use std::{
    sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    thread,
    time::Duration,
};

use anyhow::Result;
use futures::task::noop_waker;
use wasmer::{AsStoreAsync, AsStoreMut, Function, Instance, Module, Store, imports};
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
            let interrupter = store_async.store().write().objects_mut().interrupter();
            interrupter_slot.lock().unwrap().replace(interrupter);

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
    #[derive(Default)]
    struct CountWakes(AtomicU32);

    impl Wake for CountWakes {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

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
                pending_count.fetch_add(1, Ordering::SeqCst);
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
    assert_eq!(pending_count.load(Ordering::SeqCst), 4);

    let wakes = Arc::new(CountWakes::default());
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    for f in &mut futures {
        assert!(f.as_mut().poll(&mut cx).is_pending());
    }

    interrupter.interrupt();
    assert_eq!(wakes.0.load(Ordering::Relaxed), 4);

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

#[test]
fn cancelling_an_interrupted_store_preserves_another_suspended_store() -> Result<()> {
    let wasm = wat::parse_str(WAT)?;

    let mut first_store = Store::default();
    let interrupter = first_store.interrupter();
    let mut second_store = Store::new(first_store.engine().clone());
    let module = Module::new(&first_store, &wasm)?;
    let f = Function::new_typed_async(&mut first_store, std::future::pending::<()>);
    let first_instance = Instance::new(
        &mut first_store,
        &module,
        &imports! { "env" => { "f" => f } },
    )?;
    let first_func = first_instance
        .exports
        .get_typed_function::<(), ()>(&first_store, "async")?;
    let first_store = first_store.into_async();

    let ready = Arc::new(AtomicBool::new(false));
    let f = Function::new_typed_async(&mut second_store, {
        let ready = ready.clone();
        move || {
            let ready = ready.clone();
            std::future::poll_fn(move |_| {
                if ready.load(Ordering::Relaxed) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
        }
    });
    let second_instance = Instance::new(
        &mut second_store,
        &module,
        &imports! { "env" => { "f" => f } },
    )?;
    let second_func = second_instance
        .exports
        .get_typed_function::<(), ()>(&second_store, "async")?;
    let second_store = second_store.into_async();

    let mut first = Box::pin(first_func.call_async(&first_store));
    let mut second = Box::pin(second_func.call_async(&second_store));
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    assert!(first.as_mut().poll(&mut cx).is_pending());
    assert!(second.as_mut().poll(&mut cx).is_pending());

    interrupter.interrupt();
    let Poll::Ready(Err(err)) = first.as_mut().poll(&mut cx) else {
        panic!("the interrupted call should trap");
    };
    assert_eq!(err.to_trap(), Some(TrapCode::HostInterrupt));
    drop(first);

    ready.store(true, Ordering::Relaxed);
    assert!(matches!(second.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    Ok(())
}
