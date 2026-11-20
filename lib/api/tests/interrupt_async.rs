#![cfg(all(
    unix,
    feature = "experimental-host-interrupt",
    feature = "experimental-async"
))]

// TODO: tests for recursive function calls across different stores

use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    sync::{Arc, Barrier, Mutex, atomic::AtomicU32},
    task::{Context, Poll, Wake, Waker},
    thread,
    time::Duration,
};

use anyhow::Result;
use futures::task::noop_waker;
use wasmer::{Function, Instance, Module, RuntimeError, Store, Value, imports};
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

/// An interrupted call ends at once, with its coroutine: the store is not
/// left interrupted by calls that returned their error but were not dropped,
/// and a new call on it runs.
#[test]
fn an_interrupted_call_does_not_keep_its_store_interrupted() -> Result<()> {
    let (store, waits, plain) = waiting_guest()?;
    let interrupter = store.interrupter();
    let store = store.into_async();
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);

    let mut call = Box::pin(waits.call_async(&store, vec![]));
    assert!(call.as_mut().poll(&mut cx).is_pending());
    interrupter.interrupt();
    let Poll::Ready(Err(error)) = call.as_mut().poll(&mut cx) else {
        panic!("the call was not interrupted");
    };
    assert_eq!(error.to_trap(), Some(TrapCode::HostInterrupt));

    // `call` is kept: it finished, and holds nothing.
    let mut next = Box::pin(plain.call_async(&store, vec![]));
    let Poll::Ready(result) = next.as_mut().poll(&mut cx) else {
        panic!("the next call did not finish");
    };
    assert_eq!(result?.to_vec(), vec![Value::I32(1)]);
    drop(call);
    Ok(())
}

type CallFuture = Pin<Box<dyn Future<Output = Result<Box<[Value]>, RuntimeError>>>>;

thread_local! {
    /// The call `PollsAtOnce` polls.
    static POLLED_AT_ONCE: RefCell<Option<CallFuture>> = const { RefCell::new(None) };
}

/// A waker that polls its call as soon as it is woken.
struct PollsAtOnce;

impl Wake for PollsAtOnce {
    fn wake(self: Arc<Self>) {
        let waker = Waker::from(self);
        POLLED_AT_ONCE.with_borrow_mut(|call| {
            if let Some(call) = call {
                let _ = call.as_mut().poll(&mut Context::from_waker(&waker));
            }
        });
    }
}

/// The interrupt wakes a call whose waker polls it at once: the poll can
/// take the interrupt's bookkeeping again.
#[test]
fn an_interrupt_wakes_a_waker_that_polls_at_once() -> Result<()> {
    let (store, waits, _) = waiting_guest()?;
    let interrupter = store.interrupter();
    let store = store.into_async();
    let waker = Waker::from(Arc::new(PollsAtOnce));
    let mut call: CallFuture = Box::pin(waits.call_async(&store, vec![]));
    assert!(
        call.as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    );
    POLLED_AT_ONCE.set(Some(call));
    interrupter.interrupt();
    drop(POLLED_AT_ONCE.take());
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
