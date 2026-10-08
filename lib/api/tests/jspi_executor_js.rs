//! Whether the JS backend's async calls can be driven by an executor other than
//! `wasm_bindgen_futures`'.
//!
//! `Function::call_async` returns a future, and on `sys` that future is
//! executor-agnostic: `AsyncCallFuture` polls the host's future from its own
//! `poll`, so whoever drives the call drives everything. The JS backend instead
//! hands each host future to `future_to_promise`, which spawns it on
//! `wasm_bindgen_futures`' queue — an executor the caller never chose.
//!
//! These tests pin what a replacement has to keep working. They drive
//! `call_async` by hand, with a waker of their own, yielding to the JS event
//! loop between polls — which is the shape any JS-side executor must take, since
//! `WebAssembly.Suspending` suspends on every call and only the event loop can
//! resume a suspended stack.

#![cfg(all(feature = "experimental-async", feature = "js", target_arch = "wasm32"))]

use std::{
    cell::RefCell,
    future::Future,
    rc::Rc,
    sync::Arc,
    task::{Context, Poll},
};

use futures::task::{ArcWake, waker};
use js_sys::Promise;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::wasm_bindgen_test;
use wasmer::{Function, Instance, Module, Store, TypedFunction, imports};

thread_local! {
    static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn record(entry: String) {
    LOG.with(|l| l.borrow_mut().push(entry));
}

fn take_log() -> Vec<String> {
    LOG.with(|l| std::mem::take(&mut *l.borrow_mut()))
}

/// Counts how often the future asked to be polled again.
#[derive(Default)]
struct Wakes(std::sync::atomic::AtomicUsize);

impl ArcWake for Wakes {
    fn wake_by_ref(arc: &Arc<Self>) {
        arc.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Hands control back to the JS event loop, so a suspended guest can resume.
async fn yield_to_event_loop() {
    JsFuture::from(Promise::resolve(&JsValue::UNDEFINED))
        .await
        .unwrap();
}

/// The module both tests use: each guest calls the async import twice.
const TWO_SUSPENSIONS: &str = r#"
(module
  (import "host" "step" (func $step (param i32) (result i32)))
  (func (export "run") (param i32) (result i32)
    local.get 0
    call $step
    call $step))
"#;

fn instantiate(store: &mut Store) -> TypedFunction<i32, i32> {
    let module = Module::new(&store, TWO_SUSPENSIONS).unwrap();
    let step = Function::new_typed_async(store, async move |value: i32| {
        record(format!("g{value}"));
        // A real suspension: the host future is not ready on first poll.
        yield_to_event_loop().await;
        value
    });
    let instance =
        Instance::new(store, &module, &imports! { "host" => { "step" => step } }).unwrap();
    instance.exports.get_typed_function(&*store, "run").unwrap()
}

/// A `call_async` future must make progress under an executor that is not
/// `wasm_bindgen_futures`', provided that executor yields to the event loop —
/// and it must signal readiness through its waker rather than rely on the
/// caller spinning.
#[wasm_bindgen_test]
async fn a_call_async_future_runs_under_a_foreign_executor() {
    let _ = take_log();
    let mut store = Store::default();
    let run = instantiate(&mut store);
    let store_async = store.into_async();

    let wakes = Arc::new(Wakes::default());
    let w = waker(wakes.clone());
    let mut call = Box::pin(run.call_async(&store_async, 7));

    let mut polls = 0usize;
    let result = loop {
        polls += 1;
        assert!(
            polls < 1000,
            "call_async made no progress under a foreign executor"
        );
        let mut cx = Context::from_waker(&w);
        match call.as_mut().poll(&mut cx) {
            Poll::Ready(value) => break value.unwrap(),
            Poll::Pending => yield_to_event_loop().await,
        }
    };

    assert_eq!(result, 7, "the guest should have run to completion");
    assert_eq!(
        take_log(),
        vec!["g7", "g7"],
        "both suspensions should have run"
    );
    assert!(
        wakes.0.load(std::sync::atomic::Ordering::SeqCst) > 0,
        "the future never woke its caller: progress depended on the caller spinning"
    );
}

/// Two calls on one store must interleave rather than serialise: this is what
/// WASIX green threads are built on, so any redesign has to preserve it.
#[wasm_bindgen_test]
async fn two_calls_on_one_store_interleave() {
    let _ = take_log();
    let mut store = Store::default();
    let first = instantiate(&mut store);
    let second = instantiate(&mut store);
    let store_async = store.into_async();

    let wakes = Arc::new(Wakes::default());
    let w = waker(wakes);

    let mut a = Box::pin(first.call_async(&store_async, 1));
    let mut b = Box::pin(second.call_async(&store_async, 2));
    let done: Rc<RefCell<(Option<i32>, Option<i32>)>> = Rc::new(RefCell::new((None, None)));

    let mut polls = 0usize;
    loop {
        polls += 1;
        assert!(polls < 2000, "calls made no progress: {:?}", take_log());
        let mut cx = Context::from_waker(&w);
        if done.borrow().0.is_none()
            && let Poll::Ready(v) = a.as_mut().poll(&mut cx)
        {
            done.borrow_mut().0 = Some(v.unwrap());
        }
        if done.borrow().1.is_none()
            && let Poll::Ready(v) = b.as_mut().poll(&mut cx)
        {
            done.borrow_mut().1 = Some(v.unwrap());
        }
        if done.borrow().0.is_some() && done.borrow().1.is_some() {
            break;
        }
        yield_to_event_loop().await;
    }

    assert_eq!(
        *done.borrow(),
        (Some(1), Some(2)),
        "both calls should complete"
    );

    // Serialised execution would be g1 g1 g2 g2; interleaved is anything else.
    let log = take_log();
    assert_eq!(log.len(), 4, "each guest suspends twice: {log:?}");
    assert_ne!(
        log,
        vec!["g1", "g1", "g2", "g2"],
        "the two calls serialised instead of interleaving"
    );
}
