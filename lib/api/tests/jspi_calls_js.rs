//! Async calls on the JavaScript backend: who holds the store while a guest
//! runs, is suspended and resumes; what dropping a call does to its guest;
//! what a guest sees when it cannot be suspended; and what goes through an
//! async import.
#![cfg(all(feature = "experimental-async", feature = "js", target_arch = "wasm32"))]

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
};

use futures::{FutureExt, channel::oneshot};
use js_sys::Promise;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::wasm_bindgen_test;
use wasmer::{
    AsStoreAsync, AsStoreMut, AsStoreRef, AsyncFunctionEnvMut, Function, FunctionEnv,
    FunctionEnvMut, FunctionType, Instance, Module, RuntimeError, Store, StoreAsync, Type, Value,
    imports, js::AsJs,
};
use wasmer_types::{StoreId, TrapCode};

async fn macrotask() {
    let promise = Promise::new(&mut |resolve, _reject| {
        let set_timeout: js_sys::Function =
            js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("setTimeout"))
                .unwrap()
                .unchecked_into();
        set_timeout
            .call2(&JsValue::UNDEFINED, &resolve, &JsValue::from_f64(0.0))
            .unwrap();
    });
    JsFuture::from(promise).await.unwrap();
}

/// Lets everything that is ready to run, run.
async fn settle() {
    for _ in 0..3 {
        macrotask().await;
    }
}

/// Polls with a waker that does nothing: a guest only goes on from here when
/// the test polls again.
fn poll_once<F: Future + Unpin>(future: &mut F) -> Poll<F::Output> {
    future.poll_unpin(&mut Context::from_waker(futures::task::noop_waker_ref()))
}

/// Polls `call` until `ready` holds, letting the event loop run in between.
/// The host function of an async import runs once the engine has suspended
/// the guest, which is known a few microtasks after the guest called it.
async fn poll_until(call: &mut Call, ready: impl Fn() -> bool) {
    for _ in 0..20 {
        assert!(poll_once(call).is_pending(), "the call finished");
        if ready() {
            return;
        }
        macrotask().await;
    }
    panic!("the call never got there");
}

/// Whether somebody holds the write lock of `store`.
fn locked(store: &impl AsStoreAsync) -> bool {
    poll_once(&mut Box::pin(store.write_lock())).is_pending()
}

/// A waker that counts how often it was woken.
struct Wakes(AtomicUsize);

impl Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

type Log = Rc<RefCell<Vec<String>>>;
type Call = Pin<Box<dyn Future<Output = Result<Box<[Value]>, RuntimeError>>>>;

/// The error of a host function, as an embedder would define one.
#[derive(Clone, Debug, PartialEq)]
struct HostExit(i32);

impl std::fmt::Display for HostExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "exited with {}", self.0)
    }
}

impl std::error::Error for HostExit {}

/// An async host function `wait(id) -> id`. What its future does depends on
/// the id: below `AUTO` it waits for the test to finish it by hand, and from
/// each of the constants below up it does what they say.
#[derive(Clone, Default)]
struct Gate(Rc<GateState>);

/// Finishes by itself, a macrotask later.
const AUTO: i32 = 1000;
/// Has nothing to wait for.
const READY: i32 = 2000;
/// Wakes itself at its first poll, and is ready at its second.
const YIELD: i32 = 3000;
/// Fails at its first poll, with `HostExit(id)`.
const FAIL: i32 = 4000;

/// Not ready once, having woken whoever polled it.
struct YieldOnce(bool);

impl Future for YieldOnce {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if std::mem::replace(&mut self.0, true) {
            return Poll::Ready(());
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

#[derive(Default)]
struct GateState {
    log: Log,
    open: RefCell<HashMap<i32, oneshot::Sender<Result<(), HostExit>>>>,
    /// Futures dropped before they finished.
    dropped: Cell<u32>,
}

struct Unfinished(Rc<GateState>);

impl Drop for Unfinished {
    fn drop(&mut self) {
        self.0.dropped.set(self.0.dropped.get() + 1);
    }
}

impl Gate {
    fn new(log: &Log) -> Self {
        Self(Rc::new(GateState {
            log: log.clone(),
            ..Default::default()
        }))
    }

    fn import(&self, store: &mut Store) -> Function {
        let state = self.0.clone();
        Function::new_typed_async(store, move |id: i32| {
            // This is the host function itself. It runs when its future is
            // first polled, by the call.
            state.log.borrow_mut().push(format!("wait{id}"));
            let (open, opened) = oneshot::channel();
            if id < AUTO {
                state.open.borrow_mut().insert(id, open);
            }
            let unfinished = Unfinished(state.clone());
            async move {
                let outcome = match id {
                    ..AUTO => opened.await.unwrap_or(Ok(())),
                    AUTO..READY => {
                        macrotask().await;
                        Ok(())
                    }
                    READY..YIELD => Ok(()),
                    YIELD..FAIL => {
                        YieldOnce(false).await;
                        Ok(())
                    }
                    _ => Err(HostExit(id)),
                };
                std::mem::forget(unfinished);
                outcome.map(|()| id)
            }
        })
    }

    fn waiting(&self, id: i32) -> bool {
        self.0.open.borrow().contains_key(&id)
    }

    fn finish(&self, id: i32, outcome: Result<(), HostExit>) {
        let open = self.0.open.borrow_mut().remove(&id);
        open.expect("no guest waits in this host function")
            .send(outcome)
            .expect("the host future is gone");
    }

    fn release(&self, id: i32) {
        self.finish(id, Ok(()));
    }

    fn dropped(&self) -> u32 {
        self.0.dropped.get()
    }
}

/// What the synchronous host functions of a guest keep.
struct Host {
    log: Log,
    /// The export `inner`, for the imports that call back into the guest.
    inner: Option<Function>,
    errors: Vec<RuntimeError>,
}

// The tests are single-threaded; a function environment has to be `Send`.
unsafe impl Send for Host {}

struct Guest {
    store: StoreAsync,
    instance: Instance,
    env: FunctionEnv<Host>,
    gate: Gate,
    log: Log,
}

const GUEST: &str = r#"(module
  (import "host" "wait" (func $wait (param i32) (result i32)))
  (import "host" "observe" (func $observe (param i32)))
  (import "host" "call_inner" (func $call_inner (param i32) (result i32)))
  (import "host" "call_inner_raw" (func $call_inner_raw (param i32) (result i32)))

  (func (export "inner") (param i32) (result i32) (call $wait (local.get 0)))

  (func (export "once") (param i32) (result i32) (local i32)
    (call $observe (i32.const 1))
    (local.set 1 (call $wait (local.get 0)))
    (call $observe (i32.const 2))
    (local.get 1))

  ;; Waits in `id` and then in `id + 1`, and returns the sum of what it got.
  (func (export "twice") (param i32) (result i32) (local i32)
    (local.set 1 (call $wait (local.get 0)))
    (call $observe (local.get 0))
    (local.set 1
      (i32.add (local.get 1) (call $wait (i32.add (local.get 0) (i32.const 1)))))
    (call $observe (local.get 0))
    (local.get 1))

  ;; Waits in `a` and then in `b`, and returns the sum of what it got.
  (func (export "pair") (param $a i32) (param $b i32) (result i32) (local i32)
    (local.set 2 (call $wait (local.get $a)))
    (call $observe (i32.const 1))
    (local.set 2 (i32.add (local.get 2) (call $wait (local.get $b))))
    (call $observe (i32.const 2))
    (local.get 2))

  (func (export "trap_now") unreachable)
  (func (export "trap_after") (param i32) (drop (call $wait (local.get 0))) unreachable)

  ;; Enters `inner`, which waits in `within`, from a host function: where the
  ;; guest cannot be suspended. Then waits in `id`, where it can.
  (func (export "reenter")
    (param $id i32) (param $raw i32) (param $resumed i32) (param $within i32) (result i32)
    (local $inner i32)
    (if (local.get $resumed) (then (drop (call $wait (local.get $id)))))
    (local.set $inner
      (if (result i32) (local.get $raw)
        (then (call $call_inner_raw (local.get $within)))
        (else (call $call_inner (local.get $within)))))
    (call $observe (i32.const 1))
    (drop (call $wait (local.get $id)))
    (call $observe (i32.const 2))
    (local.get $inner)))"#;

/// Records `label`, and how the host function reaches its store: as the store
/// of an async call, and with the write lock held for its guest.
fn observe(mut env: FunctionEnvMut<'_, Host>, label: i32) {
    let store = match env.as_store_async() {
        Some(store) if locked(&store) => "async, locked",
        Some(_) => "async, free",
        None => "sync",
    };
    env.data_mut()
        .log
        .borrow_mut()
        .push(format!("{label}: {store}"));
}

/// Calls the export `inner` with `Function::call`.
fn call_inner(mut env: FunctionEnvMut<'_, Host>, id: i32) -> i32 {
    let inner = env.data().inner.clone().unwrap();
    match inner.call(&mut env.as_store_mut(), &[Value::I32(id)]) {
        Ok(results) => results[0].unwrap_i32(),
        Err(error) => {
            env.data_mut().errors.push(error);
            -1
        }
    }
}

/// Calls the export `inner` as the JavaScript function it is, so that only
/// the engine knows the guest was entered.
fn call_inner_raw(mut env: FunctionEnvMut<'_, Host>, id: i32) -> i32 {
    let inner = env.data().inner.clone().unwrap();
    let inner: js_sys::Function = inner.as_jsvalue(&env.as_store_ref()).unchecked_into();
    match inner.call1(&JsValue::UNDEFINED, &id.into()) {
        Ok(value) => value.as_f64().unwrap() as i32,
        Err(error) => {
            env.data_mut().errors.push(error.into());
            -1
        }
    }
}

fn guest() -> Guest {
    guest_of(GUEST)
}

/// Instantiates `wat`, which imports what it needs of the host functions above.
fn guest_of(wat: &str) -> Guest {
    let mut store = Store::default();
    let module = Module::new(&store, wat).unwrap();
    let log: Log = Rc::default();
    let gate = Gate::new(&log);
    let env = FunctionEnv::new(
        &mut store,
        Host {
            log: log.clone(),
            inner: None,
            errors: Vec::new(),
        },
    );
    let host = imports! {
        "host" => {
            "wait" => gate.import(&mut store),
            "observe" => Function::new_typed_with_env(&mut store, &env, observe),
            "call_inner" => Function::new_typed_with_env(&mut store, &env, call_inner),
            "call_inner_raw" => Function::new_typed_with_env(&mut store, &env, call_inner_raw),
            "fail" => Function::new_typed(&mut store, |code: i32| Err::<(), _>(HostExit(code))),
        }
    };
    let instance = Instance::new(&mut store, &module, &host).unwrap();
    env.as_mut(&mut store).inner = instance.exports.get_function("inner").ok().cloned();
    Guest {
        store: store.into_async(),
        instance,
        env,
        gate,
        log,
    }
}

impl Guest {
    fn call(&self, export: &str, arguments: &[i32]) -> Call {
        let function = self.instance.exports.get_function(export).unwrap();
        let arguments = arguments.iter().copied().map(Value::I32).collect();
        Box::pin(function.call_async(&self.store, arguments))
    }

    /// Runs `call` until its guest waits in the host function for `id`.
    async fn until_waiting(&self, call: &mut Call, id: i32) {
        poll_until(call, || self.gate.waiting(id)).await;
    }

    fn log(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    async fn errors(&self) -> Vec<RuntimeError> {
        let lock = self.store.read_lock().await;
        self.env.as_ref(&lock).errors.clone()
    }

    /// Takes the store back, which nothing may be holding any more.
    fn into_store(self) -> Store {
        let Self { store, .. } = self;
        store
            .into_store()
            .unwrap_or_else(|_| panic!("something still holds the store"))
    }
}

fn i32_of(result: Result<Box<[Value]>, RuntimeError>) -> i32 {
    result.unwrap()[0].unwrap_i32()
}

// --- the store, around a suspension ------------------------------------------

/// A guest runs with its store's write lock before and after a suspension, and
/// a synchronous host function reaches the store as that of an async call in
/// both spans. While the guest is suspended the store is free, and that is
/// when the host function runs.
#[wasm_bindgen_test]
async fn a_resumed_guest_runs_with_its_store() {
    let guest = guest();
    let mut call = guest.call("once", &[7]);
    guest.until_waiting(&mut call, 7).await;
    assert_eq!(guest.log(), ["1: async, locked", "wait7"]);
    assert!(!locked(&guest.store), "free while the guest is suspended");

    guest.gate.release(7);
    assert_eq!(i32_of(call.await), 7);
    assert_eq!(
        guest.log(),
        ["1: async, locked", "wait7", "2: async, locked"]
    );
    assert!(!locked(&guest.store), "free once the guest has finished");
    guest.into_store();
}

/// A guest is woken with its store. If other host code holds the store when
/// the host function finishes, the guest is woken once it has it back, and
/// does not run without it; code that asks for the store after the wake waits
/// for the guest.
#[wasm_bindgen_test]
async fn a_guest_is_woken_only_with_its_store() {
    let guest = guest();
    let mut call = guest.call("once", &[7]);
    guest.until_waiting(&mut call, 7).await;

    let job = guest.store.write_lock().await;
    guest.gate.release(7);
    assert!(poll_once(&mut call).is_pending());
    settle().await;
    assert!(poll_once(&mut call).is_pending());
    settle().await;
    assert_eq!(
        guest.log(),
        ["1: async, locked", "wait7"],
        "the guest ran while other host code held its store"
    );
    drop(job);
    assert_eq!(i32_of(call.await), 7);
    assert_eq!(guest.log().last().unwrap(), "2: async, locked");

    let mut call = guest.call("once", &[8]);
    guest.until_waiting(&mut call, 8).await;
    guest.gate.release(8);
    assert!(poll_once(&mut call).is_pending());
    assert!(
        locked(&guest.store),
        "the woken guest does not have its store"
    );
    assert_eq!(i32_of(call.await), 8);
    guest.into_store();
}

/// A guest that traps gives its store back, in its first span and in a later
/// one, and the store goes on being usable.
#[wasm_bindgen_test]
async fn a_trapping_guest_does_not_wedge_its_store() {
    let guest = guest();
    let error = guest.call("trap_now", &[]).await.unwrap_err();
    assert!(
        error.message().to_lowercase().contains("unreachable"),
        "{error}"
    );
    assert!(!locked(&guest.store));

    let mut call = guest.call("trap_after", &[7]);
    guest.until_waiting(&mut call, 7).await;
    guest.gate.release(7);
    let error = call.await.unwrap_err();
    assert!(
        error.message().to_lowercase().contains("unreachable"),
        "{error}"
    );
    assert!(!locked(&guest.store));

    assert_eq!(i32_of(guest.call("once", &[AUTO]).await), AUTO);
    guest.into_store();
}

/// Two calls in one store, suspended in the same import and resumed in either
/// order: each resumed guest is given its own results and the store.
#[wasm_bindgen_test]
async fn two_calls_in_one_store_resume_in_either_order() {
    for first in [10, 20] {
        let second = 30 - first;
        let guest = guest();
        let mut calls = HashMap::from([
            (10, guest.call("twice", &[10])),
            (20, guest.call("twice", &[20])),
        ]);
        guest.until_waiting(calls.get_mut(&10).unwrap(), 10).await;
        guest.until_waiting(calls.get_mut(&20).unwrap(), 20).await;

        // `first` goes all the way while `second` stays where it is.
        guest.gate.release(first);
        guest
            .until_waiting(calls.get_mut(&first).unwrap(), first + 1)
            .await;
        guest.gate.release(first + 1);
        assert_eq!(i32_of(calls.remove(&first).unwrap().await), 2 * first + 1);

        guest.gate.release(second);
        guest
            .until_waiting(calls.get_mut(&second).unwrap(), second + 1)
            .await;
        guest.gate.release(second + 1);
        assert_eq!(i32_of(calls.remove(&second).unwrap().await), 2 * second + 1);

        let observed: Vec<String> = guest
            .log()
            .into_iter()
            .filter(|entry| !entry.starts_with("wait"))
            .collect();
        let expected: Vec<String> = [first, first, second, second]
            .iter()
            .map(|id| format!("{id}: async, locked"))
            .collect();
        assert_eq!(observed, expected);
        guest.into_store();
    }
}

/// Two calls in one store woken in the same tick. The first to resume returns,
/// and holds the store until the engine has settled its promise. The second
/// resumes before that: it finds the store taken, and waits for it.
#[wasm_bindgen_test]
async fn two_calls_woken_together_take_turns() {
    let guest = guest();
    let mut a = guest.call("once", &[10]);
    let mut b = guest.call("once", &[20]);
    guest.until_waiting(&mut a, 10).await;
    guest.until_waiting(&mut b, 20).await;
    guest.gate.release(10);
    guest.gate.release(20);
    assert!(poll_once(&mut a).is_pending() && poll_once(&mut b).is_pending());
    let (a, b) = futures::join!(a, b);
    assert_eq!((i32_of(a), i32_of(b)), (10, 20));
    assert_eq!(
        guest.log()[4..],
        ["2: async, locked", "2: async, locked"],
        "{:?}",
        guest.log()
    );
    guest.into_store();
}

/// Two stores on one thread, their guests suspended at the same time, and one
/// entered from inside a host function of the other: no guest is ever given
/// the other one's store.
#[wasm_bindgen_test]
async fn two_stores_overlap_on_one_thread() {
    struct Other {
        guest: Guest,
        call: Option<Call>,
        this_store: Option<StoreId>,
    }
    unsafe impl Send for Other {}

    let other = guest();
    let mut store = Store::default();
    let module = Module::new(
        &store,
        r#"(module
          (import "host" "wait" (func $wait (param i32) (result i32)))
          (import "host" "enter_other" (func $enter_other (param i32)))
          (import "host" "same_store" (func $same_store (result i32)))
          (func (export "run") (param i32) (result i32)
            (call $enter_other (i32.add (local.get 0) (i32.const 1)))
            (drop (call $wait (local.get 0)))
            (call $same_store)))"#,
    )
    .unwrap();
    let log: Log = Rc::default();
    let gate = Gate::new(&log);
    let this_store = store.id();
    let env = FunctionEnv::new(
        &mut store,
        Other {
            guest: other,
            call: None,
            this_store: Some(this_store),
        },
    );
    // Starts a call in the other store, from the stack of this store's guest.
    let enter_other = |mut env: FunctionEnvMut<'_, Other>, id: i32| {
        let mut call = env.data().guest.call("once", &[id]);
        assert!(poll_once(&mut call).is_pending());
        env.data_mut().call = Some(call);
    };
    // Whether this store, and not the other one, is the one of the call.
    let same_store = |env: FunctionEnvMut<'_, Other>| -> i32 {
        let this = env.data().this_store;
        let current = env.as_store_async().map(|store| store.store_id());
        (current == this && this != Some(env.data().guest.store.store_id())) as i32
    };
    let imports = imports! {
        "host" => {
            "wait" => gate.import(&mut store),
            "enter_other" => Function::new_typed_with_env(&mut store, &env, enter_other),
            "same_store" => Function::new_typed_with_env(&mut store, &env, same_store),
        }
    };
    let instance = Instance::new(&mut store, &module, &imports).unwrap();
    let run = instance.exports.get_function("run").unwrap().clone();
    let store = store.into_async();

    let mut call: Call = Box::pin(run.call_async(&store, vec![Value::I32(7)]));
    poll_until(&mut call, || gate.waiting(7)).await;
    let (other_log, other_gate, other_store, mut other_call) = {
        let mut lock = store.write_lock().await;
        let other = env.as_mut(&mut lock);
        (
            other.guest.log.clone(),
            other.guest.gate.clone(),
            other.guest.store.store(),
            other.call.take().unwrap(),
        )
    };
    poll_until(&mut other_call, || other_gate.waiting(8)).await;
    // Both guests are suspended, each in its own store.
    assert!(!locked(&store));
    assert!(!locked(&other_store));
    assert_eq!(*other_log.borrow(), ["1: async, locked", "wait8"]);

    // Resumed in the order they were not entered in.
    gate.release(7);
    assert_eq!(i32_of(call.await), 1);
    other_gate.release(8);
    assert_eq!(i32_of(other_call.await), 8);
    assert_eq!(other_log.borrow().last().unwrap(), "2: async, locked");
    drop(other_store);
    assert!(store.into_store().is_ok());
}

/// The host future of an async import is polled with the caller's own
/// waker: first in the microtask that sees the guest wait, then by the future
/// of the call whenever the host future wakes it. The caller is woken again
/// when the guest has finished, and for nothing else.
#[wasm_bindgen_test]
async fn the_host_future_is_polled_with_the_callers_waker() {
    let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    let wakes = || wakes.0.load(Ordering::Relaxed);

    let guest = guest();
    let mut call = guest.call("once", &[7]);
    assert!(call.poll_unpin(&mut cx).is_pending());
    assert!(
        !guest.gate.waiting(7),
        "the host function ran before the guest waited"
    );
    settle().await;
    assert!(guest.gate.waiting(7), "the host function did not run");
    assert_eq!(wakes(), 0, "the caller was woken for nothing");

    guest.gate.release(7);
    assert_eq!(wakes(), 1, "the host future did not wake its caller");
    settle().await;
    assert_eq!(guest.log().len(), 2, "the guest went on without its caller");

    assert!(call.poll_unpin(&mut cx).is_pending());
    settle().await;
    assert_eq!(guest.log().last().unwrap(), "2: async, locked");
    assert_eq!(wakes(), 2, "the finished guest did not wake its caller");
    assert!(call.poll_unpin(&mut cx).is_ready());
    guest.into_store();
}

// --- cancellation ------------------------------------------------------------

/// Guests that wait inside a handler which catches everything a guest can
/// catch, and say so with `observe(9)`.
const CATCH_ALL_LEGACY: &str = r#"(module
  (import "host" "wait" (func $wait (param i32) (result i32)))
  (import "host" "fail" (func $fail (param i32)))
  (import "host" "observe" (func $observe (param i32)))
  (func (export "once") (param i32) (result i32)
    (call $observe (i32.const 1))
    try
      (drop (call $wait (local.get 0)))
    catch_all
      (call $observe (i32.const 9))
    end
    (call $observe (i32.const 2))
    (local.get 0))
  (func (export "fail") (param i32)
    try
      (call $fail (local.get 0))
    catch_all
      (call $observe (i32.const 9))
    end))"#;

const CATCH_ALL_TRY_TABLE: &str = r#"(module
  (import "host" "wait" (func $wait (param i32) (result i32)))
  (import "host" "fail" (func $fail (param i32)))
  (import "host" "observe" (func $observe (param i32)))
  (func (export "once") (param i32) (result i32)
    (call $observe (i32.const 1))
    (block $done
      (block $caught
        (try_table (catch_all $caught)
          (drop (call $wait (local.get 0)))
          (br $done)))
      (call $observe (i32.const 9)))
    (call $observe (i32.const 2))
    (local.get 0))
  (func (export "fail") (param i32)
    (block $caught
      (try_table (catch_all $caught)
        (call $fail (local.get 0))
        (return)))
    (call $observe (i32.const 9))))"#;

/// The guests of the tests below: without a handler, and with each kind of
/// handler the engine compiles.
fn guests_with_handlers() -> Vec<(&'static str, Guest)> {
    let mut guests = vec![("no handler", guest_of(GUEST))];
    for (name, wat) in [
        ("legacy catch_all", CATCH_ALL_LEGACY),
        ("try_table catch_all", CATCH_ALL_TRY_TABLE),
    ] {
        if Module::new(&Store::default(), wat).is_ok() {
            guests.push((name, guest_of(wat)));
        }
    }
    assert!(
        guests.len() > 1,
        "no handler compiles here: nothing is tested"
    );
    guests
}

/// Dropping a call while its guest is suspended drops the host future at
/// once, gives the store back at once, and unwinds the guest without running
/// any of it: not the code after the import, and not its handlers.
#[wasm_bindgen_test]
async fn a_cancelled_guest_runs_no_more_code() {
    for (name, guest) in guests_with_handlers() {
        let mut call = guest.call("once", &[7]);
        guest.until_waiting(&mut call, 7).await;
        drop(call);
        assert_eq!(
            guest.gate.dropped(),
            1,
            "{name}: the host future outlived its call"
        );
        let log = guest.log.clone();
        // Right away: nothing waits for the guest's stack to unwind.
        let store = guest.into_store();
        settle().await;
        assert_eq!(*log.borrow(), ["1: async, locked", "wait7"], "{name}");
        drop(store);
    }
}

/// The same when the host function has finished and its guest is woken, but
/// not yet resumed: nothing can stop the engine resuming it, and it still
/// runs nothing.
#[wasm_bindgen_test]
async fn a_guest_cancelled_after_its_wake_runs_no_more_code() {
    for (name, guest) in guests_with_handlers() {
        let mut call = guest.call("once", &[7]);
        guest.until_waiting(&mut call, 7).await;
        guest.gate.release(7);
        assert!(poll_once(&mut call).is_pending());
        drop(call);
        let log = guest.log.clone();
        let store = guest.into_store();
        settle().await;
        assert_eq!(*log.borrow(), ["1: async, locked", "wait7"], "{name}");
        drop(store);
    }
}

/// The handlers of the guests above do run: a guest that is not cancelled
/// goes on, and they catch what a synchronous host function throws.
#[wasm_bindgen_test]
async fn the_handlers_catch_what_can_be_caught() {
    for (name, guest) in guests_with_handlers().into_iter().skip(1) {
        let mut call = guest.call("once", &[7]);
        guest.until_waiting(&mut call, 7).await;
        guest.gate.release(7);
        assert_eq!(i32_of(call.await), 7, "{name}");
        guest.call("fail", &[3]).await.unwrap();
        assert_eq!(
            guest.log(),
            [
                "1: async, locked",
                "wait7",
                "2: async, locked",
                "9: async, locked"
            ],
            "{name}"
        );
    }
}

/// A call dropped while its guest is on the stack cannot unwind it there. The
/// guest keeps its store until it returns, or until its next async import,
/// which traps.
#[wasm_bindgen_test]
async fn a_call_dropped_while_its_guest_runs_stops_it_at_the_next_import() {
    /// The call to drop, and whether the guest ran past its last import.
    struct Dropper(Rc<RefCell<Option<Call>>>, bool);
    unsafe impl Send for Dropper {}
    let mut store = Store::default();
    let module = Module::new(
        &store,
        r#"(module
          (import "host" "wait" (func $wait (param i32) (result i32)))
          (import "host" "drop_call" (func $drop_call))
          (import "host" "after" (func $after))
          (func (export "run") (result i32)
            (drop (call $wait (i32.const 7)))
            (call $drop_call)
            (drop (call $wait (i32.const 8)))
            (call $after)
            (i32.const 1)))"#,
    )
    .unwrap();
    let log: Log = Rc::default();
    let gate = Gate::new(&log);
    let slot: Rc<RefCell<Option<Call>>> = Rc::default();
    let env = FunctionEnv::new(&mut store, Dropper(slot.clone(), false));
    let drop_call = |env: FunctionEnvMut<'_, Dropper>| drop(env.data().0.borrow_mut().take());
    let after = |mut env: FunctionEnvMut<'_, Dropper>| env.data_mut().1 = true;
    let imports = imports! {
        "host" => {
            "wait" => gate.import(&mut store),
            "drop_call" => Function::new_typed_with_env(&mut store, &env, drop_call),
            "after" => Function::new_typed_with_env(&mut store, &env, after),
        }
    };
    let instance = Instance::new(&mut store, &module, &imports).unwrap();
    let run = instance.exports.get_function("run").unwrap().clone();
    let store = store.into_async();

    let mut call: Call = Box::pin(run.call_async(&store, vec![]));
    poll_until(&mut call, || gate.waiting(7)).await;
    *slot.borrow_mut() = Some(call);
    gate.release(7);
    let woken = poll_once(slot.borrow_mut().as_mut().unwrap());
    assert!(woken.is_pending());
    // The guest resumes from the event loop and drops its own call.
    settle().await;
    assert_eq!(*log.borrow(), ["wait7"], "the second host function ran");
    assert!(slot.borrow().is_none(), "the guest did not resume");
    let mut store = store.into_store().expect("something still holds the store");
    assert!(
        !env.as_mut(&mut store).1,
        "the guest ran past its cancellation"
    );
}

/// Whoever awaits a call is not kept by it once the call is dropped.
#[wasm_bindgen_test]
async fn a_dropped_call_retains_no_waker() {
    let guest = guest();
    let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
    let waker = Waker::from(wakes.clone());
    let mut call = guest.call("once", &[7]);
    assert!(
        call.poll_unpin(&mut Context::from_waker(&waker))
            .is_pending()
    );
    assert!(
        Arc::strong_count(&wakes) > 2,
        "the call does not hold its waker"
    );
    drop(call);
    assert_eq!(
        Arc::strong_count(&wakes),
        2,
        "a dropped call kept its waker"
    );
    settle().await;
    guest.into_store();
}

// --- host errors -------------------------------------------------------------

/// An error from an async host function is a trap: no handler of the guest
/// sees it, and the caller gets the error itself back.
#[wasm_bindgen_test]
async fn a_host_error_is_a_trap_and_keeps_its_type() {
    for (name, guest) in guests_with_handlers() {
        let mut call = guest.call("once", &[7]);
        guest.until_waiting(&mut call, 7).await;
        guest.gate.finish(7, Err(HostExit(42)));
        let error = call.await.unwrap_err();
        assert_eq!(
            error.downcast_ref::<HostExit>(),
            Some(&HostExit(42)),
            "{name}: {error}"
        );
        assert_eq!(guest.log(), ["1: async, locked", "wait7"], "{name}");
        assert!(!locked(&guest.store), "{name}");
        guest.into_store();
    }
}

/// A dynamic host function is trusted with the number and types of its
/// results. Wrong ones trap, instead of reaching the guest as an exception.
#[wasm_bindgen_test]
async fn results_that_do_not_match_the_signature_trap() {
    for wrong in [
        vec![],
        vec![Value::I64(1)],
        vec![Value::I32(1), Value::I32(2)],
    ] {
        let mut store = Store::default();
        let module = Module::new(&store, signature_guest(&[], &[Type::I32])).unwrap();
        let caught = Rc::new(Cell::new(0));
        let imports = imports! {
            "host" => {
                "f" => Function::new_async(&mut store, FunctionType::new([], [Type::I32]), {
                    let wrong = wrong.clone();
                    move |_| {
                        let wrong = wrong.clone();
                        async move { Ok(wrong) }
                    }
                }),
                "caught" => counter(&mut store, &caught),
            }
        };
        let instance = Instance::new(&mut store, &module, &imports).unwrap();
        let run = instance.exports.get_function("run").unwrap().clone();
        let error = run
            .call_async(&store.into_async(), vec![])
            .await
            .unwrap_err();
        assert!(
            error.message().contains("do not match its signature"),
            "{error}"
        );
        assert_eq!(caught.get(), 0, "the guest caught the error");
    }
}

// --- where a guest cannot be suspended ---------------------------------------

/// An async import reached through a synchronous call from a host function,
/// or through a JavaScript frame, cannot suspend its guest. The call fails
/// with the error other backends give, the host function does not run, and
/// the call beneath is left as it was: it has its store and can still wait.
#[wasm_bindgen_test]
async fn a_refused_suspension_leaves_the_call_usable() {
    for raw in [0, 1] {
        for resumed in [0, 1] {
            let case = format!("raw: {raw}, resumed: {resumed}");
            let guest = guest();
            let mut call = guest.call("reenter", &[AUTO, raw, resumed, 99]);
            assert_eq!(i32_of((&mut call).await), -1, "{case}");

            let waits = format!("wait{AUTO}");
            let mut expected = vec!["1: async, locked", &waits, "2: async, locked"];
            if resumed == 1 {
                expected.insert(0, &waits);
            }
            // The host function did not run for the refused import, `wait99`.
            assert_eq!(guest.log(), expected, "{case}");
            let errors = guest.errors().await;
            assert_eq!(errors.len(), 1, "{case}");
            assert_eq!(
                errors[0].clone().to_trap(),
                Some(TrapCode::YieldOutsideAsyncContext),
                "{case}: {}",
                errors[0]
            );
            assert_eq!(guest.gate.dropped(), 0, "{case}");
            drop(call);
            guest.into_store();
        }
    }
}

/// A guest that was not entered by `call_async` at all gets the same error,
/// from `Function::call` and from a start function.
#[wasm_bindgen_test]
fn an_async_import_outside_call_async_is_refused() {
    let Guest {
        store,
        instance,
        log,
        ..
    } = guest();
    let mut store = store.into_store().unwrap();
    let inner = instance.exports.get_function("inner").unwrap();
    let error = inner.call(&mut store, &[Value::I32(7)]).unwrap_err();
    assert_eq!(
        error.clone().to_trap(),
        Some(TrapCode::YieldOutsideAsyncContext),
        "{error}"
    );

    let module = Module::new(
        &store,
        r#"(module
          (import "host" "wait" (func $wait (param i32) (result i32)))
          (func $start (drop (call $wait (i32.const 7))))
          (start $start))"#,
    )
    .unwrap();
    let gate = Gate::new(&log);
    let imports = imports! { "host" => { "wait" => gate.import(&mut store) } };
    assert!(Instance::new(&mut store, &module, &imports).is_err());
    assert!(
        log.borrow().is_empty(),
        "a host function ran: {:?}",
        log.borrow()
    );
}

/// An async host function can be called with `call_async` itself.
#[wasm_bindgen_test]
async fn an_async_host_function_can_be_called_directly() {
    let mut store = Store::default();
    let log: Log = Rc::default();
    let gate = Gate::new(&log);
    let wait = gate.import(&mut store);
    let store = store.into_async();
    let call = |id: i32| -> Call { Box::pin(wait.call_async(&store, vec![Value::I32(id)])) };
    assert_eq!(i32_of(call(AUTO).await), AUTO);

    let mut pending = call(7);
    assert!(poll_once(&mut pending).is_pending());
    gate.finish(7, Err(HostExit(3)));
    let error = pending.await.unwrap_err();
    assert_eq!(error.downcast_ref::<HostExit>(), Some(&HostExit(3)));

    assert!(call(8).now_or_never().is_none());
    assert_eq!(gate.dropped(), 1);
    assert!(store.into_store().is_ok());
}

// --- host futures ------------------------------------------------------------

/// A lock the host function still holds when it has to wait stays with its
/// future: the store is not free while the guest is suspended, and the guest
/// gets it back when the future is done.
#[wasm_bindgen_test]
async fn a_guard_held_while_waiting_stays_with_the_future() {
    let mut store = Store::default();
    let module = Module::new(&store, GUEST).unwrap();
    let log: Log = Rc::default();
    let env = FunctionEnv::new(
        &mut store,
        Host {
            log: log.clone(),
            inner: None,
            errors: Vec::new(),
        },
    );
    let (release, released) = oneshot::channel::<()>();
    let released = Rc::new(RefCell::new(Some(released)));
    let held = Rc::new(Cell::new(false));
    let hold = {
        let held = held.clone();
        move |env: AsyncFunctionEnvMut<Host>, arguments: &[Value]| {
            let (released, id) = (released.borrow_mut().take(), arguments[0].clone());
            let held = held.clone();
            async move {
                let guard = env.write().await;
                held.set(true);
                released.unwrap().await.unwrap();
                drop(guard);
                Ok(vec![id])
            }
        }
    };
    let unary = FunctionType::new([Type::I32], [Type::I32]);
    let imports = imports! {
        "host" => {
            "wait" => Function::new_with_env_async(&mut store, &env, unary, hold),
            "observe" => Function::new_typed_with_env(&mut store, &env, observe),
            "call_inner" => Function::new_typed_with_env(&mut store, &env, call_inner),
            "call_inner_raw" => Function::new_typed_with_env(&mut store, &env, call_inner_raw),
        }
    };
    let instance = Instance::new(&mut store, &module, &imports).unwrap();
    let once = instance.exports.get_function("once").unwrap().clone();
    let store = store.into_async();

    let mut call: Call = Box::pin(once.call_async(&store, vec![Value::I32(7)]));
    poll_until(&mut call, || held.get()).await;
    assert_eq!(*log.borrow(), ["1: async, locked"]);
    assert!(locked(&store), "the suspended host function lost its guard");
    release.send(()).unwrap();
    assert_eq!(i32_of(call.await), 7);
    assert_eq!(log.borrow().last().unwrap(), "2: async, locked");
    assert!(!locked(&store));
    assert!(store.into_store().is_ok());
}

// --- what this backend cannot follow -----------------------------------------

/// JavaScript that suspends a guest outside of Wasmer.
mod foreign {
    use wasm_bindgen::prelude::wasm_bindgen;

    #[wasm_bindgen(inline_js = r#"
    export function foreignSuspending() {
      let resolve;
      const promise = new Promise((r) => { resolve = r; });
      return [new WebAssembly.Suspending(() => promise), resolve];
    }
    export function throwingProxy() {
      const trap = () => { throw new Error("a trap ran"); };
      return new Proxy(function () {}, { get: trap, has: trap, getOwnPropertyDescriptor: trap });
    }
    export function relay(bytes, suspending) {
      const module = new WebAssembly.Module(bytes);
      return new WebAssembly.Instance(module, { h: { s: suspending } }).exports.s;
    }
    "#)]
    extern "C" {
        /// A `WebAssembly.Suspending` function of nothing that returns an
        /// `i32`, and the function that resolves what it waits for.
        #[wasm_bindgen(js_name = foreignSuspending)]
        pub(super) fn foreign_suspending() -> js_sys::Array;
        /// The export `s` of `bytes`, instantiated with `suspending` as its
        /// import `h.s`.
        pub(super) fn relay(
            bytes: &[u8],
            suspending: &wasm_bindgen::JsValue,
        ) -> wasm_bindgen::JsValue;
        /// A function whose `Proxy` traps throw.
        #[wasm_bindgen(js_name = throwingProxy)]
        pub(super) fn throwing_proxy() -> wasm_bindgen::JsValue;
    }
}

/// A guest suspended in a `WebAssembly.Suspending` function that Wasmer did
/// not make, which another module's export calls, keeps its store: nothing
/// tells this backend when it goes on.
#[wasm_bindgen_test]
async fn a_guest_suspended_outside_wasmer_keeps_its_store() {
    let pair = foreign::foreign_suspending();
    let relay = wat::parse_str(
        r#"(module
          (import "h" "s" (func $s (result i32)))
          (func (export "s") (result i32) (call $s)))"#,
    )
    .unwrap();
    let suspending = foreign::relay(&relay, &pair.get(0));
    let resolve: js_sys::Function = pair.get(1).unchecked_into();

    let mut store = Store::default();
    let ty = FunctionType::new([], [Type::I32]);
    let s = Function::from_jsvalue(&mut store, &ty, &suspending).unwrap();
    let module = Module::new(
        &store,
        r#"(module
          (import "h" "s" (func $s (result i32)))
          (func (export "run") (result i32) (i32.add (call $s) (i32.const 1))))"#,
    )
    .unwrap();
    let instance = Instance::new(&mut store, &module, &imports! { "h" => { "s" => s } }).unwrap();
    let run = instance.exports.get_function("run").unwrap().clone();
    let store = store.into_async();

    let mut call: Call = Box::pin(run.call_async(&store, vec![]));
    assert!(poll_once(&mut call).is_pending());
    settle().await;
    assert!(
        locked(&store),
        "the store was given up while the guest waits"
    );
    resolve
        .call1(&JsValue::UNDEFINED, &JsValue::from(5))
        .unwrap();
    assert_eq!(i32_of(call.await), 6);
    assert!(!locked(&store));
    assert!(store.into_store().is_ok());
}

/// A guest re-entered by JavaScript that a host function calls, which calls
/// an async import there, and then is suspended in a `WebAssembly.Suspending`
/// function that Wasmer did not make. The import refuses before it makes a
/// wait, on engines that run its body before refusing too: nothing runs the
/// host function, and the guest keeps its store while it is suspended.
#[wasm_bindgen_test]
async fn a_refused_reentry_then_a_foreign_suspension_keeps_the_store() {
    let pair = foreign::foreign_suspending();
    let relay = wat::parse_str(
        r#"(module
          (import "h" "s" (func $s (result i32)))
          (func (export "s") (result i32) (call $s)))"#,
    )
    .unwrap();
    let suspending = foreign::relay(&relay, &pair.get(0));
    let resolve: js_sys::Function = pair.get(1).unchecked_into();

    let mut store = Store::default();
    let module = Module::new(
        &store,
        r#"(module
          (import "host" "wait" (func $wait (param i32) (result i32)))
          (import "host" "call_inner_raw" (func $call_inner_raw (param i32) (result i32)))
          (import "host" "s" (func $s (result i32)))
          (func (export "inner") (param i32) (result i32) (call $wait (local.get 0)))
          (func (export "run") (result i32)
            (i32.add (call $call_inner_raw (i32.const 99)) (call $s))))"#,
    )
    .unwrap();
    let log: Log = Rc::default();
    let gate = Gate::new(&log);
    let env = FunctionEnv::new(
        &mut store,
        Host {
            log: log.clone(),
            inner: None,
            errors: Vec::new(),
        },
    );
    let ty = FunctionType::new([], [Type::I32]);
    let imports = imports! {
        "host" => {
            "wait" => gate.import(&mut store),
            "call_inner_raw" => Function::new_typed_with_env(&mut store, &env, call_inner_raw),
            "s" => Function::from_jsvalue(&mut store, &ty, &suspending).unwrap(),
        }
    };
    let instance = Instance::new(&mut store, &module, &imports).unwrap();
    env.as_mut(&mut store).inner = instance.exports.get_function("inner").ok().cloned();
    let run = instance.exports.get_function("run").unwrap().clone();
    let store = store.into_async();

    let mut call: Call = Box::pin(run.call_async(&store, vec![]));
    assert!(poll_once(&mut call).is_pending());
    settle().await;
    assert!(poll_once(&mut call).is_pending());
    assert!(
        locked(&store),
        "the store was given up while the guest waits"
    );
    assert!(log.borrow().is_empty(), "the refused host function ran");
    resolve
        .call1(&JsValue::UNDEFINED, &JsValue::from(5))
        .unwrap();
    // The re-entry failed with -1.
    assert_eq!(i32_of(call.await), 4);
    let errors = {
        let lock = store.read_lock().await;
        env.as_ref(&lock).errors.clone()
    };
    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors[0].clone().to_trap(),
        Some(TrapCode::YieldOutsideAsyncContext)
    );
    assert_eq!(gate.dropped(), 0);
    env.as_mut(&mut store.write_lock().await).inner = None;
    assert!(store.into_store().is_ok());
}

/// An async host function cannot give a guest a thenable: the promise the
/// guest waits on would wait for it, and give the guest what it settles
/// with. The import fails instead, and the guest is unwound as when the host
/// function fails.
///
/// So does one whose result throws when its `then` is read.
#[wasm_bindgen_test]
async fn a_thenable_result_of_an_async_import_fails_it() {
    for (value, message) in [
        (externref_results::thenable(true), "thenable"),
        (externref_results::throwing_then(), "then threw"),
    ] {
        let mut store = Store::default();
        let ty = FunctionType::new([], [Type::ExternRef]);
        let value = Value::from_jsvalue(&mut store, &Type::ExternRef, &value).unwrap();
        let f = Function::new_async(&mut store, ty, move |_| {
            let value = value.clone();
            async move { Ok(vec![value]) }
        });
        let module = Module::new(&store, signature_guest(&[], &[Type::ExternRef])).unwrap();
        let caught = Rc::new(Cell::new(0));
        let imports = imports! { "host" => { "f" => f, "caught" => counter(&mut store, &caught) } };
        let instance = Instance::new(&mut store, &module, &imports).unwrap();
        let run = instance.exports.get_function("run").unwrap().clone();
        let store = store.into_async();

        let error = run.call_async(&store, vec![]).await.unwrap_err();
        assert!(error.message().contains(message), "{error}");
        assert_eq!(caught.get(), 0, "{message}: the guest caught the failure");
        assert!(store.into_store().is_ok(), "{message}");
    }
}

/// An async host function is given to JavaScript in the form a guest imports
/// it in, as `Instance::new` imports it: by `Function::as_jsvalue`,
/// `Extern::as_jsvalue` and `Imports::as_jsvalue` alike, since the function
/// itself works only for a guest Wasmer instantiated. `Function::from_jsvalue`
/// takes that form for the host function. Any other function is given as it
/// is. Nothing of what JavaScript is given is read to tell them apart: a
/// `Proxy` whose traps throw goes through as it is.
#[wasm_bindgen_test]
async fn async_host_functions_are_given_to_javascript_as_imports() {
    let mut store = Store::default();
    let wait = Function::new_typed_async(&mut store, async |value: i32| value + 1);
    let plain = Function::new_typed(&mut store, |value: i32| value);
    let suspending = wait.as_jsvalue(&store);
    let extern_ = wasmer::Extern::Function(wait.clone()).as_jsvalue(&store);
    assert!(js_sys::Object::is(&extern_, &suspending));
    let plain_function = plain.as_jsvalue(&store);
    assert!(plain_function.is_function());
    assert!(!js_sys::Object::is(&suspending, &plain_function));

    let imports = imports! { "h" => { "wait" => wait, "plain" => plain } };
    let object = imports.as_jsvalue(&store);
    let namespace = js_sys::Reflect::get(&object, &"h".into()).unwrap();
    let get = |name: &str| js_sys::Reflect::get(&namespace, &name.into()).unwrap();
    assert!(js_sys::Object::is(&get("wait"), &suspending));
    assert!(js_sys::Object::is(&get("plain"), &plain_function));

    let ty = FunctionType::new([Type::I32], [Type::I32]);
    let hostile = foreign::throwing_proxy();
    let taken = Function::from_jsvalue(&mut store, &ty, &hostile).unwrap();
    assert!(js_sys::Object::is(&taken.as_jsvalue(&store), &hostile));
    let wait = Function::from_jsvalue(&mut store, &ty, &suspending).unwrap();
    let store = store.into_async();
    let results = wait.call_async(&store, vec![Value::I32(1)]).await.unwrap();
    assert_eq!(results[0].unwrap_i32(), 2);
}

/// A guest beneath a JavaScript frame calls an async import, which an engine
/// may refuse to suspend it in only once the import's body has run, and then
/// drops its own call. It is still running, and keeps its store until it
/// returns: a wait is taken for one only once the guest's slice is over.
#[wasm_bindgen_test]
async fn a_call_dropped_after_a_refused_wait_keeps_the_store_for_its_guest() {
    struct Env {
        inner: Option<Function>,
        call: Rc<RefCell<Option<Call>>>,
        held: Rc<Cell<Option<bool>>>,
    }
    unsafe impl Send for Env {}
    let mut store = Store::default();
    let module = Module::new(
        &store,
        r#"(module
          (import "host" "wait" (func $wait (param i32) (result i32)))
          (import "host" "call_inner_raw" (func $call_inner_raw (param i32) (result i32)))
          (import "host" "drop_call" (func $drop_call))
          (import "host" "held" (func $held))
          (func (export "inner") (param i32) (result i32) (call $wait (local.get 0)))
          (func (export "run") (result i32)
            (drop (call $wait (i32.const 1000)))
            (drop (call $call_inner_raw (i32.const 99)))
            (call $drop_call)
            (call $held)
            (i32.const 1)))"#,
    )
    .unwrap();
    let log: Log = Rc::default();
    let gate = Gate::new(&log);
    let slot: Rc<RefCell<Option<Call>>> = Rc::default();
    let held: Rc<Cell<Option<bool>>> = Rc::default();
    let env = FunctionEnv::new(
        &mut store,
        Env {
            inner: None,
            call: slot.clone(),
            held: held.clone(),
        },
    );
    let call_inner_raw = |env: FunctionEnvMut<'_, Env>, id: i32| -> i32 {
        let inner = env.data().inner.clone().unwrap();
        let inner: js_sys::Function = inner.as_jsvalue(&env.as_store_ref()).unchecked_into();
        match inner.call1(&JsValue::UNDEFINED, &id.into()) {
            Ok(value) => value.as_f64().unwrap() as i32,
            Err(_) => -1,
        }
    };
    let drop_call = |env: FunctionEnvMut<'_, Env>| drop(env.data().call.borrow_mut().take());
    let held_now = |env: FunctionEnvMut<'_, Env>| {
        let store = env.as_store_async().expect("a guest of an async call");
        env.data().held.set(Some(locked(&store)));
    };
    let imports = imports! {
        "host" => {
            "wait" => gate.import(&mut store),
            "call_inner_raw" => Function::new_typed_with_env(&mut store, &env, call_inner_raw),
            "drop_call" => Function::new_typed_with_env(&mut store, &env, drop_call),
            "held" => Function::new_typed_with_env(&mut store, &env, held_now),
        }
    };
    let instance = Instance::new(&mut store, &module, &imports).unwrap();
    env.as_mut(&mut store).inner = instance.exports.get_function("inner").ok().cloned();
    let run = instance.exports.get_function("run").unwrap().clone();
    let store = store.into_async();

    *slot.borrow_mut() = Some(Box::pin(run.call_async(&store, vec![])));
    for _ in 0..20 {
        let polled = slot.borrow_mut().as_mut().map(poll_once);
        if polled.is_none() {
            break;
        }
        macrotask().await;
    }
    assert!(slot.borrow().is_none(), "the guest did not drop its call");
    assert_eq!(held.get(), Some(true), "the running guest lost its store");
    settle().await;
    assert_eq!(*log.borrow(), ["wait1000"]);
    env.as_mut(&mut store.write_lock().await).inner = None;
    assert!(
        store.into_store().is_ok(),
        "something still holds the store"
    );
}

// --- signatures --------------------------------------------------------------

fn wat_types(types: &[Type]) -> String {
    let names = types.iter().map(|ty| match ty {
        Type::I32 => "i32",
        Type::I64 => "i64",
        Type::F32 => "f32",
        Type::F64 => "f64",
        Type::ExternRef => "externref",
        other => panic!("{other} is not a type of these tests"),
    });
    names.collect::<Vec<_>>().join(" ")
}

/// A guest that calls the import `f` inside a handler which catches all a
/// guest can catch, and says so with `caught`.
fn signature_guest(params: &[Type], results: &[Type]) -> String {
    let (params, results) = (wat_types(params), wat_types(results));
    let arguments: String = (0..params.split_whitespace().count())
        .map(|index| format!("(local.get {index}) "))
        .collect();
    format!(
        r#"(module
          (import "host" "f" (func $f (param {params}) (result {results})))
          (import "host" "caught" (func $caught))
          (func (export "run") (param {params}) (result {results})
            (block $caught
              (try_table (catch_all $caught)
                (return (call $f {arguments}))))
            (call $caught)
            (unreachable)))"#
    )
}

fn counter(store: &mut Store, count: &Rc<Cell<u32>>) -> Function {
    struct Count(Rc<Cell<u32>>);
    unsafe impl Send for Count {}
    let env = FunctionEnv::new(store, Count(count.clone()));
    Function::new_typed_with_env(store, &env, |env: FunctionEnvMut<'_, Count>| {
        env.data().0.set(env.data().0.get() + 1)
    })
}

/// What a value is, to compare one before and after it went through a guest.
fn describe(store: &impl AsStoreRef, value: &Value) -> String {
    match value {
        Value::ExternRef(Some(reference)) => {
            format!("externref {:?}", reference.downcast::<u32>(store))
        }
        value => format!("{value:?}"),
    }
}

/// Every type an async host function can take or return, alone and together,
/// and whatever the call comes to: results, a host error, a cancellation. The
/// guest gets the values or is unwound, and never has anything to catch.
#[wasm_bindgen_test]
async fn every_signature_goes_through_and_unwinds() {
    use Type::*;
    let signatures: [(&[Type], &[Type]); 9] = [
        (&[], &[]),
        (&[I32], &[I32]),
        (&[I64], &[I64]),
        (&[F32], &[F32]),
        (&[F64], &[F64]),
        (&[ExternRef], &[ExternRef]),
        (&[I32, I64, F32, F64], &[F64, F32, I64, I32]),
        (&[I64, ExternRef, I32], &[ExternRef, I64, F32]),
        (&[I64; 40], &[I64; 20]),
    ];
    for (params, results) in signatures {
        let ty = FunctionType::new(params.to_vec(), results.to_vec());
        let case = format!("{ty}");
        let mut store = Store::default();
        let module = Module::new(&store, signature_guest(params, results)).unwrap();
        let caught = Rc::new(Cell::new(0));
        // The outcome of the next call, sent by the test.
        type Outcome = Result<Vec<Value>, HostExit>;
        let outcomes: Rc<RefCell<Option<oneshot::Receiver<Outcome>>>> = Rc::default();
        let received: Rc<RefCell<Vec<Value>>> = Rc::default();
        let f = Function::new_async(&mut store, ty.clone(), {
            let (outcomes, received) = (outcomes.clone(), received.clone());
            move |arguments| {
                *received.borrow_mut() = arguments.to_vec();
                let outcome = outcomes.borrow_mut().take().unwrap();
                async move {
                    outcome
                        .await
                        .unwrap()
                        .map_err(|exit| RuntimeError::user(exit.into()))
                }
            }
        });
        let imports = imports! { "host" => { "f" => f, "caught" => counter(&mut store, &caught) } };
        let instance = Instance::new(&mut store, &module, &imports).unwrap();
        let run = instance.exports.get_function("run").unwrap().clone();
        let mut sample = |ty: &Type| match ty {
            I32 => Value::I32(-7),
            I64 => Value::I64(i64::MIN + 5),
            F32 => Value::F32(1.5),
            F64 => Value::F64(-2.25),
            ExternRef => Value::ExternRef(Some(wasmer::ExternRef::new(&mut store, 99u32))),
            _ => unreachable!(),
        };
        let arguments: Vec<Value> = params.iter().map(&mut sample).collect();
        let values: Vec<Value> = results.iter().map(&mut sample).collect();
        let store = store.into_async();
        // `at_once`: the outcome is there when the host future is first
        // polled.
        let call = |outcome: Option<Outcome>, at_once: bool| {
            let (send, receive) = oneshot::channel();
            *outcomes.borrow_mut() = Some(receive);
            let mut send = Some(send);
            if at_once {
                send.take().unwrap().send(outcome.clone().unwrap()).unwrap();
            }
            let mut call: Call = Box::pin(run.call_async(&store, arguments.clone()));
            assert!(
                poll_once(&mut call).is_pending(),
                "{case}: the call finished at its first poll"
            );
            if let (Some(send), Some(outcome)) = (send, outcome) {
                send.send(outcome).unwrap();
            }
            call
        };

        // Cancelled while the results are not there.
        drop(call(None, false));
        settle().await;

        for at_once in [false, true] {
            // The host function fails.
            let error = call(Some(Err(HostExit(5))), at_once).await.unwrap_err();
            assert_eq!(
                error.downcast_ref::<HostExit>(),
                Some(&HostExit(5)),
                "{case}, at once: {at_once}: {error}"
            );

            // The host function returns.
            let returned = call(Some(Ok(values.clone())), at_once).await.unwrap();
            let lock = store.read_lock().await;
            let describe = |values: &[Value]| -> Vec<String> {
                values.iter().map(|value| describe(&lock, value)).collect()
            };
            assert_eq!(describe(&returned), describe(&values), "{case}: results");
            assert_eq!(
                describe(&received.borrow()),
                describe(&arguments),
                "{case}: arguments"
            );
        }
        assert_eq!(caught.get(), 0, "{case}: the guest caught something");
    }
}

/// Numbers through async host functions arrive as they were sent, in both
/// directions: every kind, from no parameter to more than are passed as plain
/// numbers, with a result and without, at once or after a wait.
#[wasm_bindgen_test]
async fn numbers_go_through_async_host_functions_as_they_are() {
    use Type::*;
    fn bits(value: &Value) -> (Type, u64) {
        match value {
            Value::I32(value) => (I32, *value as u32 as u64),
            Value::F32(value) => (F32, value.to_bits() as u64),
            Value::F64(value) => (F64, value.to_bits()),
            other => panic!("{other:?} is not a number"),
        }
    }
    let samples = |ty: &Type, index: usize| match ty {
        I32 => Value::I32([i32::MIN, -1, i32::MAX][index % 3]),
        F32 => Value::F32([-0.0, f32::MIN_POSITIVE / 4.0, f32::NEG_INFINITY][index % 3]),
        _ => Value::F64([5e-324, -0.0, 9_007_199_254_740_993.0][index % 3]),
    };
    let kinds = [I32, F32, F64];
    for arity in [0, 1, 3, 8, 9] {
        for result in [None, Some(I32), Some(F32), Some(F64)] {
            let params: Vec<Type> = kinds.iter().copied().cycle().take(arity).collect();
            let results: Vec<Type> = result.into_iter().collect();
            let ty = FunctionType::new(params.clone(), results.clone());
            let case = format!("{ty}");
            let mut store = Store::default();
            let module = Module::new(&store, signature_guest(&params, &results)).unwrap();
            let caught = Rc::new(Cell::new(0));
            type Calls = Vec<Vec<(Type, u64)>>;
            let received: Rc<RefCell<Calls>> = Rc::default();
            let returns: Vec<Value> = results.iter().map(|ty| samples(ty, arity)).collect();
            let f = Function::new_async(&mut store, ty, {
                let (received, returns) = (received.clone(), returns.clone());
                move |arguments| {
                    let wait = received.borrow().len() % 2 == 1;
                    received
                        .borrow_mut()
                        .push(arguments.iter().map(bits).collect());
                    let returns = returns.clone();
                    async move {
                        if wait {
                            macrotask().await;
                        }
                        Ok(returns)
                    }
                }
            });
            let imports =
                imports! { "host" => { "f" => f, "caught" => counter(&mut store, &caught) } };
            let instance = Instance::new(&mut store, &module, &imports).unwrap();
            let run = instance.exports.get_function("run").unwrap().clone();
            let store = store.into_async();
            // Once with a host function that is ready, once with one that waits.
            for round in 0..2 {
                let arguments: Vec<Value> = (0..arity)
                    .map(|index| samples(&params[index], index + round))
                    .collect();
                let returned = run.call_async(&store, arguments.clone()).await.unwrap();
                let sent: Vec<_> = arguments.iter().map(bits).collect();
                assert_eq!(received.borrow()[round], sent, "{case}: arguments");
                let returned: Vec<_> = returned.iter().map(bits).collect();
                let expected: Vec<_> = returns.iter().map(bits).collect();
                assert_eq!(returned, expected, "{case}: results");
            }
            assert_eq!(caught.get(), 0, "{case}");
        }
    }
}

/// An external reference to JavaScript's `undefined` is a value like any
/// other, and reaches the guest as it is.
#[wasm_bindgen_test]
async fn an_undefined_externref_stays_undefined() {
    let mut store = Store::default();
    let module = Module::new(
        &store,
        r#"(module
          (import "host" "get" (func $get (result externref)))
          (func (export "get") (result externref) (call $get)))"#,
    )
    .unwrap();
    let undefined = Value::from_jsvalue(&mut store, &Type::ExternRef, &JsValue::UNDEFINED);
    let undefined = undefined.ok().unwrap();
    let ty = FunctionType::new([], [Type::ExternRef]);
    let get = Function::new_async(&mut store, ty, move |_| {
        let undefined = undefined.clone();
        async move { Ok(vec![undefined]) }
    });
    let instance = Instance::new(
        &mut store,
        &module,
        &imports! { "host" => { "get" => get } },
    );
    let get = instance
        .unwrap()
        .exports
        .get_function("get")
        .unwrap()
        .clone();
    let store = store.into_async();
    let results = get.call_async(&store, vec![]).await.unwrap();
    let lock = store.read_lock().await;
    assert!(results[0].as_jsvalue(&lock).is_undefined(), "{results:?}");
}

/// A function reference, which a guest cannot return to its caller on this
/// backend, so the guest calls it: null and not, alone and after a number,
/// and with the import cancelled and failing first.
#[wasm_bindgen_test]
async fn a_funcref_result_reaches_the_guest() {
    let mut store = Store::default();
    let module = Module::new(
        &store,
        r#"(module
          (type $unary (func (param i32) (result i32)))
          (import "host" "get" (func $get (result funcref)))
          (import "host" "get_two" (func $get_two (result i64 funcref)))
          (import "host" "caught" (func $caught))
          (table $table 1 funcref)
          (func (export "double") (param i32) (result i32)
            (i32.mul (local.get 0) (i32.const 2)))
          ;; Calls what `get` returns, or returns -1 for a null.
          (func (export "apply") (param i32) (param $two i32) (result i32)
            (local $function funcref)
            (block $caught
              (try_table (catch_all $caught)
                (if (local.get $two)
                  (then (local.set $function (call $get_two)) (drop))
                  (else (local.set $function (call $get))))
                (if (ref.is_null (local.get $function))
                  (then (return (i32.const -1))))
                (table.set $table (i32.const 0) (local.get $function))
                (return (call_indirect $table (type $unary) (local.get 0) (i32.const 0)))))
            (call $caught)
            (unreachable)))"#,
    )
    .unwrap();
    let caught = Rc::new(Cell::new(0));
    type Outcome = Result<Option<Function>, HostExit>;
    let outcomes: Rc<RefCell<Option<oneshot::Receiver<Outcome>>>> = Rc::default();
    let get = |store: &mut Store, two: bool| {
        let results = if two {
            vec![Type::I64, Type::FuncRef]
        } else {
            vec![Type::FuncRef]
        };
        let outcomes = outcomes.clone();
        Function::new_async(store, FunctionType::new([], results), move |_| {
            let outcome = outcomes.borrow_mut().take().unwrap();
            async move {
                let function = Value::FuncRef(outcome.await.unwrap().map_err(Box::from)?);
                Ok(if two {
                    vec![Value::I64(1), function]
                } else {
                    vec![function]
                })
            }
        })
    };
    let imports = imports! {
        "host" => {
            "get" => get(&mut store, false),
            "get_two" => get(&mut store, true),
            "caught" => counter(&mut store, &caught),
        }
    };
    let instance = Instance::new(&mut store, &module, &imports).unwrap();
    let apply = instance.exports.get_function("apply").unwrap().clone();
    let double = instance.exports.get_function("double").unwrap().clone();
    let store = store.into_async();
    for two in [0, 1] {
        let call = |outcome: Option<Outcome>| {
            let (send, receive) = oneshot::channel();
            *outcomes.borrow_mut() = Some(receive);
            let arguments = vec![Value::I32(21), Value::I32(two)];
            let mut call: Call = Box::pin(apply.call_async(&store, arguments));
            assert!(poll_once(&mut call).is_pending());
            if let Some(outcome) = outcome {
                assert!(send.send(outcome).is_ok());
            }
            call
        };
        drop(call(None));
        settle().await;
        let error = call(Some(Err(HostExit(5)))).await.unwrap_err();
        assert_eq!(
            error.downcast_ref::<HostExit>(),
            Some(&HostExit(5)),
            "{error}"
        );
        assert_eq!(i32_of(call(Some(Ok(None))).await), -1);
        assert_eq!(i32_of(call(Some(Ok(Some(double.clone())))).await), 42);
    }
    assert_eq!(caught.get(), 0, "the guest caught something");
}

/// A `v128` cannot go through a JavaScript function, so an async host
/// function with one is refused when it is made, not when it is called.
#[wasm_bindgen_test]
#[should_panic(expected = "cannot take or return a `V128`")]
fn a_signature_javascript_cannot_call_is_refused() {
    let mut store = Store::default();
    let ty = FunctionType::new([Type::V128], []);
    Function::new_async(&mut store, ty, |_| async { Ok(vec![]) });
}

// --- memory ------------------------------------------------------------------

/// A cancelled call leaves nothing behind in the engine: its stack is unwound
/// and freed, where a stack that is only abandoned stays for good on V8.
/// Prints the growth per call, next to that of calls that finish.
///
/// Slow, and a measurement more than a test: run it with `--include-ignored`.
#[wasm_bindgen_test]
#[ignore]
async fn memory_per_cancelled_call_stays_flat() {
    fn resident() -> f64 {
        let process = js_sys::Reflect::get(&js_sys::global(), &"process".into()).unwrap();
        let usage: js_sys::Function = js_sys::Reflect::get(&process, &"memoryUsage".into())
            .unwrap()
            .unchecked_into();
        let usage = usage.call0(&process).unwrap();
        js_sys::Reflect::get(&usage, &"rss".into())
            .unwrap()
            .as_f64()
            .unwrap()
    }
    const CALLS: usize = 4000;
    let guest = guest();
    let mut per_call = [0.0, 0.0];
    for (cancel, per_call) in [true, false].into_iter().zip(&mut per_call) {
        for round in 0..4 {
            let before = resident();
            for index in 0..CALLS {
                if cancel {
                    assert!(
                        guest
                            .call("once", &[index as i32 % 100])
                            .now_or_never()
                            .is_none()
                    );
                    if index % 4 == 3 {
                        macrotask().await;
                    }
                } else {
                    guest.call("once", &[AUTO]).await.unwrap();
                }
            }
            settle().await;
            guest.log.borrow_mut().clear();
            guest.gate.0.open.borrow_mut().clear();
            // The first round pays for what grows once.
            if round > 0 {
                *per_call += (resident() - before) / 1024.0 / (3 * CALLS) as f64;
            }
        }
    }
    wasm_bindgen_test::console_log!(
        "resident memory per call: {:+.2} KB cancelled, {:+.2} KB finished",
        per_call[0],
        per_call[1]
    );
    assert!(
        per_call[0] < per_call[1].max(0.0) + 4.0,
        "cancelled calls leave their stacks behind"
    );
    guest.into_store();
}

/// A waker that polls the call at once, inside `wake`, as an executor may.
/// The guest's second wait begins in a later slice, where that poll runs
/// while the guest is still on the stack: the promise the guest waits on is
/// made before the call is woken, and the host future is polled only once the
/// engine has suspended the guest. The call finishes.
#[wasm_bindgen_test]
async fn a_waker_that_polls_the_call_at_once() {
    struct PollsAtOnce {
        call: RefCell<Option<Call>>,
        result: RefCell<Option<Result<Box<[Value]>, RuntimeError>>>,
    }
    // The tests run on one thread; a waker has to be `Send` and `Sync`.
    unsafe impl Send for PollsAtOnce {}
    unsafe impl Sync for PollsAtOnce {}
    impl Wake for PollsAtOnce {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            let Ok(mut call) = self.call.try_borrow_mut() else {
                return;
            };
            let Some(future) = call.as_mut() else {
                return;
            };
            let waker = Waker::from(self.clone());
            if let Poll::Ready(result) = future.as_mut().poll(&mut Context::from_waker(&waker)) {
                *self.result.borrow_mut() = Some(result);
                *call = None;
            }
        }
    }

    // Ready at once, and waking the call during its first poll: the poll
    // of the call from inside it does not take the import for finished.
    for id in [READY, YIELD] {
        let guest = guest();
        let poller = Arc::new(PollsAtOnce {
            call: RefCell::new(Some(guest.call("twice", &[id]))),
            result: RefCell::default(),
        });
        poller.wake_by_ref();
        for _ in 0..20 {
            if poller.result.borrow().is_some() {
                break;
            }
            macrotask().await;
        }
        let result = poller.result.borrow_mut().take();
        assert_eq!(
            i32_of(result.expect("the call finished")),
            id + id + 1,
            "{id}"
        );
        assert_eq!(guest.gate.dropped(), 0, "{id}: a host future was dropped");
        guest.into_store();
    }
}

/// A waker that, woken during the first poll of a host future, polls the call
/// at once with another waker, and does nothing when it is woken again. The
/// wake that poll could not act on reaches the waker that polls the call now.
#[wasm_bindgen_test]
async fn a_wake_during_the_first_poll_reaches_the_latest_waker() {
    type Slot = Rc<RefCell<Option<Call>>>;
    type Outcome = Rc<RefCell<Option<Result<Box<[Value]>, RuntimeError>>>>;
    fn poll_with(call: &Slot, outcome: &Outcome, waker: &Waker) {
        let Ok(mut slot) = call.try_borrow_mut() else {
            return;
        };
        let Some(future) = slot.as_mut() else {
            return;
        };
        if let Poll::Ready(result) = future.as_mut().poll(&mut Context::from_waker(waker)) {
            *outcome.borrow_mut() = Some(result);
            *slot = None;
        }
    }
    /// Polls the call with itself whenever it is woken.
    struct Latest(Slot, Outcome);
    /// Polls the call with `Latest`, the first time it is woken.
    struct First(Slot, Outcome, Waker, Cell<bool>);
    // The tests run on one thread; a waker has to be `Send` and `Sync`.
    unsafe impl Send for Latest {}
    unsafe impl Sync for Latest {}
    unsafe impl Send for First {}
    unsafe impl Sync for First {}
    impl Wake for Latest {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            poll_with(&self.0, &self.1, &Waker::from(self.clone()));
        }
    }
    impl Wake for First {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            if !self.3.replace(true) {
                poll_with(&self.0, &self.1, &self.2);
            }
        }
    }

    let guest = guest();
    let (call, outcome): (Slot, Outcome) = Default::default();
    *call.borrow_mut() = Some(guest.call("twice", &[YIELD]));
    let latest = Waker::from(Arc::new(Latest(call.clone(), outcome.clone())));
    let first = Arc::new(First(
        call.clone(),
        outcome.clone(),
        latest,
        Cell::new(false),
    ));
    let first = Waker::from(first);
    // The first poll enters the guest, with `First` for its waker.
    assert!(
        call.borrow_mut()
            .as_mut()
            .unwrap()
            .as_mut()
            .poll(&mut Context::from_waker(&first))
            .is_pending()
    );
    for _ in 0..20 {
        if outcome.borrow().is_some() {
            break;
        }
        macrotask().await;
    }
    let result = outcome.borrow_mut().take();
    assert_eq!(
        i32_of(result.expect("the call finished")),
        YIELD + YIELD + 1
    );
    guest.into_store();
}

/// Guests whose result JavaScript could take for a promise.
mod externref_results {
    use wasm_bindgen::prelude::wasm_bindgen;

    use super::*;

    #[wasm_bindgen(inline_js = r#"
    export function throwingThen() {
      return { get then() { throw new Error("then threw"); } };
    }
    export function thenable(settles) {
      return { then(resolve) { if (settles) resolve("settled"); } };
    }
    export function returning(value) {
      return () => value;
    }
    "#)]
    extern "C" {
        pub(super) fn thenable(settles: bool) -> JsValue;
        #[wasm_bindgen(js_name = throwingThen)]
        pub(super) fn throwing_then() -> JsValue;
        fn returning(value: &JsValue) -> JsValue;
    }

    /// A guest that returns a thenable, which a promise may take for one. V8
    /// gives the thenable back as it is; JavaScriptCore, as the JSPI
    /// specification says, waits for it, and the call ends with what it
    /// settles with, if it ever does. Nothing tells this backend that the
    /// guest has returned before then, so the guest keeps its store until the
    /// call ends.
    #[wasm_bindgen_test]
    async fn a_guest_that_returns_a_thenable() {
        for settles in [false, true] {
            let mut store = Store::default();
            let value = thenable(settles);
            let ty = FunctionType::new([], [Type::ExternRef]);
            let make = Function::from_jsvalue(&mut store, &ty, &returning(&value)).unwrap();
            let module = Module::new(
                &store,
                r#"(module
                  (import "h" "make" (func $make (result externref)))
                  (func (export "run") (result externref) (call $make)))"#,
            )
            .unwrap();
            let imports = imports! { "h" => { "make" => make } };
            let instance = Instance::new(&mut store, &module, &imports).unwrap();
            let run = instance.exports.get_function("run").unwrap().clone();
            let store = store.into_async();

            let mut call: Call = Box::pin(run.call_async(&store, vec![]));
            let _ = poll_once(&mut call);
            settle().await;
            match poll_once(&mut call) {
                Poll::Ready(results) => {
                    let results = results.unwrap();
                    let lock = store.read_lock().await;
                    let returned = results[0].as_jsvalue(&lock);
                    let itself = js_sys::Object::is(&returned, &value);
                    let settled = settles && returned == JsValue::from_str("settled");
                    assert!(itself || settled, "settles: {settles}: {returned:?}");
                }
                // The call never ends, and the store stays with it.
                Poll::Pending => {
                    assert!(!settles, "the thenable settled");
                    assert!(locked(&store), "the store was given up");
                    continue;
                }
            }
            drop(call);
            assert!(!locked(&store), "settles: {settles}: the store is held");
            assert!(store.into_store().is_ok(), "settles: {settles}");
        }
    }
}
