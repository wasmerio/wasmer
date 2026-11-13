//! Async calls on the JavaScript backend: who holds the store while a guest
//! runs, is suspended and resumes; what dropping a call does to its guest;
//! what a guest sees when it cannot be suspended; and what changes for a host
//! function that is eager.
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
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
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
        self.function(store, false)
    }

    fn function(&self, store: &mut Store, eager: bool) -> Function {
        let state = self.0.clone();
        let function = Function::new_typed_async(store, move |id: i32| {
            // This is the host function itself. It runs when its future is
            // first polled: by the call, or inside the import if it is eager.
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
        });
        if eager { function.eager() } else { function }
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
    guest_of(GUEST, false)
}

/// The same guest, whose `wait` is an eager host function.
fn eager_guest() -> Guest {
    guest_of(GUEST, true)
}

/// Instantiates `wat`, which imports what it needs of the host functions above.
fn guest_of(wat: &str, eager: bool) -> Guest {
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
            "wait" => gate.function(&mut store, eager),
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
        for _ in 0..20 {
            assert!(poll_once(call).is_pending(), "the guest finished");
            if self.gate.waiting(id) {
                return;
            }
            macrotask().await;
        }
        panic!("the guest never waited for {id}: {:?}", self.log.borrow());
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

/// Between the moment a host function finishes and the moment the engine
/// resumes its guest, the store is free for other host code. If that code
/// keeps the store, the guest waits for it instead of running without it.
#[wasm_bindgen_test]
async fn a_host_job_between_the_wake_and_the_resumption_finds_the_store_free() {
    let guest = guest();
    let mut call = guest.call("once", &[7]);
    guest.until_waiting(&mut call, 7).await;

    guest.gate.release(7);
    assert!(poll_once(&mut call).is_pending());
    // The host function has finished and its guest is woken, not yet resumed.
    let job = poll_once(&mut Box::pin(guest.store.write_lock()));
    let Poll::Ready(job) = job else {
        panic!("the store must be free between the wake and the resumption");
    };
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
    assert!(poll_once(&mut call).is_pending());
    assert!(gate.waiting(7), "{:?}", log.borrow());
    // Both guests are suspended, each in its own store.
    assert!(!locked(&store));
    let (other_log, other_gate, other_call) = {
        let mut lock = store.write_lock().await;
        let other = env.as_mut(&mut lock);
        assert!(!locked(&other.guest.store));
        (
            other.guest.log.clone(),
            other.guest.gate.clone(),
            other.call.take().unwrap(),
        )
    };
    assert_eq!(*other_log.borrow(), ["1: async, locked", "wait8"]);

    // Resumed in the order they were not entered in.
    gate.release(7);
    assert_eq!(i32_of(call.await), 1);
    other_gate.release(8);
    assert_eq!(i32_of(other_call.await), 8);
    assert_eq!(other_log.borrow().last().unwrap(), "2: async, locked");
    assert!(store.into_store().is_ok());
}

/// The host future of an async import is polled by the future of the call:
/// nothing else drives it, and it is woken through the caller's own waker.
#[wasm_bindgen_test]
async fn the_caller_drives_the_host_function() {
    let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);

    let guest = guest();
    let mut call = guest.call("once", &[7]);
    assert!(call.poll_unpin(&mut cx).is_pending());
    assert!(
        guest.gate.waiting(7),
        "the first poll runs the host function"
    );
    assert_eq!(wakes.0.load(Ordering::Relaxed), 0);

    guest.gate.release(7);
    assert_eq!(
        wakes.0.load(Ordering::Relaxed),
        1,
        "the host future woke its caller"
    );
    settle().await;
    assert_eq!(guest.log().len(), 2, "the guest went on without its caller");

    assert!(call.poll_unpin(&mut cx).is_pending());
    settle().await;
    assert_eq!(guest.log().last().unwrap(), "2: async, locked");
    assert_eq!(
        wakes.0.load(Ordering::Relaxed),
        2,
        "the finished guest woke its caller"
    );
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
fn guests_with_handlers(eager: bool) -> Vec<(&'static str, Guest)> {
    let mut guests = vec![("no handler", guest_of(GUEST, eager))];
    for (name, wat) in [
        ("legacy catch_all", CATCH_ALL_LEGACY),
        ("try_table catch_all", CATCH_ALL_TRY_TABLE),
    ] {
        if Module::new(&Store::default(), wat).is_ok() {
            guests.push((name, guest_of(wat, eager)));
        }
    }
    assert!(
        guests.len() > 1,
        "no handler compiles here: nothing is tested"
    );
    guests
}

/// Those guests twice: with a `wait` that is not eager, and with one that is.
fn both_kinds_with_handlers() -> Vec<(String, Guest)> {
    let kinds = [(false, ""), (true, "eager, ")];
    let guests = kinds.into_iter().flat_map(|(eager, kind)| {
        let guests = guests_with_handlers(eager).into_iter();
        guests.map(move |(name, guest)| (format!("{kind}{name}"), guest))
    });
    guests.collect()
}

/// Dropping a call while its guest is suspended drops the host future at
/// once, gives the store back at once, and unwinds the guest without running
/// any of it: not the code after the import, and not its handlers.
#[wasm_bindgen_test]
async fn a_cancelled_guest_runs_no_more_code() {
    for (name, guest) in both_kinds_with_handlers() {
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
    for (name, guest) in both_kinds_with_handlers() {
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
    for (name, guest) in guests_with_handlers(false).into_iter().skip(1) {
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
/// which traps. An eager host function that drops the call it is polled in
/// is that import itself.
#[wasm_bindgen_test]
async fn a_call_dropped_while_its_guest_runs_stops_it_at_the_next_import() {
    /// The call to drop, and whether the guest ran past its last import.
    struct Dropper(Option<Call>, bool);
    unsafe impl Send for Dropper {}
    for eager in [0, 1] {
        let mut store = Store::default();
        let module = Module::new(
            &store,
            r#"(module
              (import "host" "wait" (func $wait (param i32) (result i32)))
              (import "host" "drop_call" (func $drop_call))
              (import "host" "drop_call_eager" (func $drop_call_eager))
              (import "host" "after" (func $after))
              (func (export "run") (param $eager i32) (result i32)
                (drop (call $wait (i32.const 7)))
                (if (local.get $eager)
                  (then (call $drop_call_eager))
                  (else (call $drop_call) (drop (call $wait (i32.const 8)))))
                (call $after)
                (i32.const 1)))"#,
        )
        .unwrap();
        let log: Log = Rc::default();
        let gate = Gate::new(&log);
        let env = FunctionEnv::new(&mut store, Dropper(None, false));
        let drop_call = |mut env: FunctionEnvMut<'_, Dropper>| drop(env.data_mut().0.take());
        let drop_call_eager = |env: AsyncFunctionEnvMut<Dropper>, _: &[Value]| async move {
            drop(env.write().await.data_mut().0.take());
            Ok(vec![])
        };
        let after = |mut env: FunctionEnvMut<'_, Dropper>| env.data_mut().1 = true;
        let nullary = FunctionType::new([], []);
        let imports = imports! {
            "host" => {
                "wait" => gate.import(&mut store),
                "drop_call" => Function::new_typed_with_env(&mut store, &env, drop_call),
                "drop_call_eager" => Function::new_with_env_async(&mut store, &env, nullary, drop_call_eager).eager(),
                "after" => Function::new_typed_with_env(&mut store, &env, after),
            }
        };
        let instance = Instance::new(&mut store, &module, &imports).unwrap();
        let run = instance.exports.get_function("run").unwrap().clone();
        let store = store.into_async();

        let mut call: Call = Box::pin(run.call_async(&store, vec![Value::I32(eager)]));
        assert!(poll_once(&mut call).is_pending());
        gate.release(7);
        assert!(poll_once(&mut call).is_pending());
        // The guest resumes from the event loop and drops its own call.
        env.as_mut(&mut store.write_lock().await).0 = Some(call);
        settle().await;
        assert_eq!(*log.borrow(), ["wait7"], "the second host function ran");
        let mut store = store.into_store().expect("something still holds the store");
        assert!(
            env.as_mut(&mut store).0.is_none(),
            "the guest did not resume"
        );
        assert!(
            !env.as_mut(&mut store).1,
            "eager: {eager}: the guest ran past its cancellation"
        );
    }
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
    for (name, guest) in both_kinds_with_handlers() {
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

// --- eager host functions ----------------------------------------------------

/// An eager host function that has nothing to wait for does not suspend its
/// guest: the guest gets its results inside the import call, and runs on
/// before anything that was queued earlier gets a turn.
#[wasm_bindgen_test]
async fn a_ready_eager_import_does_not_suspend_its_guest() {
    let guest = eager_guest();
    let log = guest.log.clone();
    let turn = Closure::once(move |_: JsValue| log.borrow_mut().push("turn".into()));
    let _ = Promise::resolve(&JsValue::UNDEFINED).then(&turn);

    let mut call = guest.call("pair", &[READY, READY + 1]);
    assert!(poll_once(&mut call).is_pending());
    // The whole guest ran in that one poll, with its store all along.
    let ran = [
        "wait2000",
        "1: async, locked",
        "wait2001",
        "2: async, locked",
    ];
    assert_eq!(guest.log(), ran);
    assert_eq!(i32_of(call.await), 2 * READY + 1);
    assert_eq!(guest.log().last().unwrap(), "turn");

    // The same in a span that was resumed.
    assert_eq!(
        i32_of(guest.call("pair", &[AUTO, READY]).await),
        AUTO + READY
    );
    assert_eq!(
        guest.log()[5..],
        [
            "wait1000",
            "1: async, locked",
            "wait2000",
            "2: async, locked"
        ]
    );
    guest.into_store();
}

/// An eager host function that has to wait suspends its guest like any other:
/// the store is free meanwhile, and the call polls the future from then on.
/// A future that wakes its caller during that first poll is polled again.
#[wasm_bindgen_test]
async fn an_eager_import_that_waits_suspends_its_guest() {
    let guest = eager_guest();
    let mut call = guest.call("once", &[7]);
    assert!(poll_once(&mut call).is_pending());
    assert!(
        guest.gate.waiting(7),
        "the host function runs in the import"
    );
    assert_eq!(guest.log(), ["1: async, locked", "wait7"]);
    assert!(!locked(&guest.store), "free while the guest is suspended");
    guest.gate.release(7);
    assert_eq!(i32_of(call.await), 7);
    assert_eq!(guest.log().last().unwrap(), "2: async, locked");

    // In the first span of a call, and in a resumed one, where nothing but
    // the wake makes the call poll again.
    assert_eq!(i32_of(guest.call("once", &[YIELD]).await), YIELD);
    assert_eq!(
        i32_of(guest.call("pair", &[AUTO, YIELD]).await),
        AUTO + YIELD
    );
    guest.into_store();
}

/// What an eager host function finds when it is polled inside the import.
#[derive(Default)]
struct Lent {
    /// The store's write lock could be taken at once.
    write_lock: bool,
    /// So could the environment, and with it the store could be lent on.
    parked: bool,
    /// A second lock, while the first was held, could not.
    exclusive: bool,
    /// How many of three callbacks that each want the store got it.
    drained: u32,
}

unsafe impl Send for Lent {}

/// During the poll inside the import call the store is lent to the host
/// function: it takes the locks it needs without waiting, one after another,
/// as host code that drains a queue of callbacks does. That holds wherever
/// the guest belongs to an async call, also beneath a synchronous call made
/// from one.
#[wasm_bindgen_test]
async fn the_store_is_lent_to_an_eager_host_function() {
    let mut store = Store::default();
    let module = Module::new(
        &store,
        r#"(module
          (import "host" "probe" (func $probe))
          (import "host" "wait" (func $wait (param i32) (result i32)))
          (import "host" "call_inner" (func $call_inner))
          (func (export "inner") (call $probe))
          (func (export "run") (param $resumed i32) (param $nested i32)
            (if (local.get $resumed) (then (drop (call $wait (i32.const 1000)))))
            (if (local.get $nested) (then (call $call_inner)) (else (call $probe)))))"#,
    )
    .unwrap();
    struct Env(Lent, Option<Function>);
    unsafe impl Send for Env {}
    let env = FunctionEnv::new(&mut store, Env(Lent::default(), None));
    let probe = |env: AsyncFunctionEnvMut<Env>, _: &[Value]| async move {
        let store = env.as_store_async();
        let mut found = Lent {
            write_lock: store.write_lock().now_or_never().is_some(),
            ..Lent::default()
        };
        if let Some(mut guard) = env.write().now_or_never() {
            found.exclusive = store.write_lock().now_or_never().is_none();
            let (_, mut lent) = guard.data_and_store_mut();
            let mut lent = lent.as_store_mut();
            found.parked = lent.parked(|| Store::with_current(|_| ()).is_some());
        }
        for _ in 0..3 {
            found.drained += store.write_lock().now_or_never().is_some() as u32;
        }
        env.write().await.data_mut().0 = found;
        Ok(vec![])
    };
    let call_inner = |mut env: FunctionEnvMut<'_, Env>| {
        let inner = env.data().1.clone().unwrap();
        inner.call(&mut env.as_store_mut(), &[]).unwrap();
    };
    let log: Log = Rc::default();
    let imports = imports! {
        "host" => {
            "probe" => Function::new_with_env_async(&mut store, &env, FunctionType::new([], []), probe).eager(),
            "wait" => Gate::new(&log).import(&mut store),
            "call_inner" => Function::new_typed_with_env(&mut store, &env, call_inner),
        }
    };
    let instance = Instance::new(&mut store, &module, &imports).unwrap();
    env.as_mut(&mut store).1 = instance.exports.get_function("inner").ok().cloned();
    let run = instance.exports.get_function("run").unwrap().clone();
    let store = store.into_async();
    for (resumed, nested) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        let case = format!("resumed: {resumed}, nested: {nested}");
        let arguments = vec![Value::I32(resumed), Value::I32(nested)];
        let mut call: Call = Box::pin(run.call_async(&store, arguments));
        if resumed == 0 {
            // The guest ran, probe and all, without being suspended.
            assert!(poll_once(&mut call).is_pending());
        }
        let found = std::mem::take(&mut env.as_mut(&mut store.write_lock().await).0);
        if resumed == 0 {
            assert!(found.write_lock, "{case}: the probe did not run yet");
        }
        call.await.unwrap();
        let found = match resumed {
            0 => found,
            _ => std::mem::take(&mut env.as_mut(&mut store.write_lock().await).0),
        };
        assert!(found.write_lock, "{case}: the store was not lent");
        assert!(found.parked, "{case}: the lent store could not be lent on");
        assert!(found.exclusive, "{case}: two write locks at once");
        assert_eq!(found.drained, 3, "{case}: callbacks were starved");
    }
    env.as_mut(&mut store.write_lock().await).1 = None;
    assert!(store.into_store().is_ok());
}

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
    let hold = move |env: AsyncFunctionEnvMut<Host>, arguments: &[Value]| {
        let (released, id) = (released.borrow_mut().take(), arguments[0].clone());
        async move {
            let guard = env.write().await;
            released.unwrap().await.unwrap();
            drop(guard);
            Ok(vec![id])
        }
    };
    let unary = FunctionType::new([Type::I32], [Type::I32]);
    let imports = imports! {
        "host" => {
            "wait" => Function::new_with_env_async(&mut store, &env, unary, hold).eager(),
            "observe" => Function::new_typed_with_env(&mut store, &env, observe),
            "call_inner" => Function::new_typed_with_env(&mut store, &env, call_inner),
            "call_inner_raw" => Function::new_typed_with_env(&mut store, &env, call_inner_raw),
        }
    };
    let instance = Instance::new(&mut store, &module, &imports).unwrap();
    let once = instance.exports.get_function("once").unwrap().clone();
    let store = store.into_async();

    let mut call: Call = Box::pin(once.call_async(&store, vec![Value::I32(7)]));
    assert!(poll_once(&mut call).is_pending());
    assert_eq!(*log.borrow(), ["1: async, locked"]);
    assert!(locked(&store), "the suspended host function lost its guard");
    release.send(()).unwrap();
    assert_eq!(i32_of(call.await), 7);
    assert_eq!(log.borrow().last().unwrap(), "2: async, locked");
    assert!(!locked(&store));
    assert!(store.into_store().is_ok());
}

/// Where a guest cannot be suspended, beneath a synchronous call made from an
/// async one, an eager host function that is ready works, and one that fails
/// at once fails the synchronous call with its own error. One that has to
/// wait is dropped at its first `Pending` and fails as any async import does
/// there. Either way the call beneath is left as it was.
#[wasm_bindgen_test]
async fn an_eager_import_beneath_a_synchronous_call() {
    for (raw, resumed) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        for within in [READY, 99, YIELD, FAIL] {
            let case = format!("raw: {raw}, resumed: {resumed}, within: {within}");
            let guest = eager_guest();
            let mut call = guest.call("reenter", &[AUTO, raw, resumed, within]);
            let inner = i32_of((&mut call).await);
            assert_eq!(inner, if within == READY { READY } else { -1 }, "{case}");

            // The host function ran inside the import, as far as it got.
            let (waits, within_log) = (format!("wait{AUTO}"), format!("wait{within}"));
            let mut expected = vec![&within_log, "1: async, locked", &waits, "2: async, locked"];
            if resumed == 1 {
                expected.insert(0, &waits);
            }
            assert_eq!(guest.log(), expected, "{case}");
            let waited = [99, YIELD].contains(&within);
            assert_eq!(guest.gate.dropped(), waited as u32, "{case}");

            let errors = guest.errors().await;
            assert_eq!(errors.len(), (within != READY) as usize, "{case}");
            if within == FAIL {
                assert_eq!(errors[0].downcast_ref(), Some(&HostExit(FAIL)), "{case}");
            } else if waited {
                assert_eq!(
                    errors[0].clone().to_trap(),
                    Some(TrapCode::YieldOutsideAsyncContext),
                    "{case}: {}",
                    errors[0]
                );
            }
            drop(call);
            guest.into_store();
        }
    }
}

/// The same with no async call at all: `Function::call`, and a start
/// function, in a store that was never made async.
#[wasm_bindgen_test]
fn an_eager_import_outside_call_async() {
    let Guest {
        store,
        instance,
        gate,
        log,
        ..
    } = eager_guest();
    let mut store = store.into_store().unwrap();
    let inner = instance.exports.get_function("inner").unwrap();
    let ready = inner.call(&mut store, &[Value::I32(READY)]).unwrap();
    assert_eq!(ready[0].unwrap_i32(), READY);

    let error = inner.call(&mut store, &[Value::I32(FAIL)]).unwrap_err();
    assert_eq!(error.downcast_ref(), Some(&HostExit(FAIL)), "{error}");

    let error = inner.call(&mut store, &[Value::I32(7)]).unwrap_err();
    assert_eq!(
        error.clone().to_trap(),
        Some(TrapCode::YieldOutsideAsyncContext),
        "{error}"
    );
    assert_eq!(gate.dropped(), 1, "the host future was kept");
    assert_eq!(*log.borrow(), ["wait2000", "wait4000", "wait7"]);

    let start = |id: i32| {
        format!(
            r#"(module
              (import "host" "wait" (func $wait (param i32) (result i32)))
              (func $start (drop (call $wait (i32.const {id}))))
              (start $start))"#
        )
    };
    for (id, works) in [(READY, true), (7, false)] {
        let module = Module::new(&store, start(id)).unwrap();
        let imports = imports! { "host" => { "wait" => gate.function(&mut store, true) } };
        let instance = Instance::new(&mut store, &module, &imports);
        assert_eq!(instance.is_ok(), works, "a start function waiting in {id}");
    }
}

/// An eager host function that calls back into its guest while it is polled
/// inside the import: synchronously, where the guest reaches the same import
/// again, and with a `call_async` of its own in the same store.
#[wasm_bindgen_test]
async fn an_eager_host_function_can_reenter_its_guest() {
    const READY_NOW: i32 = 0;
    const REENTER: i32 = 1;
    const NESTED_CALL: i32 = 2;
    const REENTER_AND_WAIT: i32 = 3;
    const WAIT_FOREVER: i32 = 4;

    let mut store = Store::default();
    let module = Module::new(
        &store,
        r#"(module
          (import "host" "nest" (func $nest (param i32) (result i32)))
          (func (export "run") (param i32) (result i32) (call $nest (local.get 0))))"#,
    )
    .unwrap();
    struct Env(Option<Function>);
    unsafe impl Send for Env {}
    let env = FunctionEnv::new(&mut store, Env(None));
    let nest = |env: AsyncFunctionEnvMut<Env>, arguments: &[Value]| {
        let mode = arguments[0].unwrap_i32();
        async move {
            let result = match mode {
                READY_NOW => 1,
                REENTER | REENTER_AND_WAIT => {
                    let mut guard = env.write().await;
                    let run = guard.data_mut().0.clone().unwrap();
                    let within = [READY_NOW, WAIT_FOREVER][(mode == REENTER_AND_WAIT) as usize];
                    match run.call(&mut guard, &[Value::I32(within)]) {
                        Ok(results) => 10 + results[0].unwrap_i32(),
                        Err(error) => match error.to_trap() {
                            Some(TrapCode::YieldOutsideAsyncContext) => 30,
                            _ => -1,
                        },
                    }
                }
                NESTED_CALL => {
                    let run = env.read().await.data().0.clone().unwrap();
                    let store = env.as_store_async();
                    let nested = run.call_async(&store, vec![Value::I32(READY_NOW)]);
                    20 + nested.await?[0].unwrap_i32()
                }
                _ => std::future::pending().await,
            };
            Ok(vec![Value::I32(result)])
        }
    };
    let unary = FunctionType::new([Type::I32], [Type::I32]);
    let nest = Function::new_with_env_async(&mut store, &env, unary, nest).eager();
    let instance = Instance::new(
        &mut store,
        &module,
        &imports! { "host" => { "nest" => nest } },
    );
    let run = instance
        .unwrap()
        .exports
        .get_function("run")
        .unwrap()
        .clone();
    env.as_mut(&mut store).0 = Some(run.clone());
    let store = store.into_async();
    for (mode, expected) in [(REENTER, 11), (NESTED_CALL, 21), (REENTER_AND_WAIT, 30)] {
        let result = run.call_async(&store, vec![Value::I32(mode)]).await;
        assert_eq!(i32_of(result), expected, "mode {mode}");
        assert!(!locked(&store), "mode {mode}");
    }
    env.as_mut(&mut store.write_lock().await).0 = None;
    drop(run);
    assert!(store.into_store().is_ok());
}

/// An error an eager host function returns at its first poll is a trap like
/// one it returns later.
#[wasm_bindgen_test]
async fn an_eager_host_error_at_once_is_a_trap() {
    for (name, guest) in guests_with_handlers(true) {
        let error = guest.call("once", &[FAIL]).await.unwrap_err();
        assert_eq!(
            error.downcast_ref(),
            Some(&HostExit(FAIL)),
            "{name}: {error}"
        );
        assert_eq!(guest.log(), ["1: async, locked", "wait4000"], "{name}");
        guest.into_store();
    }
}

/// An eager host function called with `call_async` itself.
#[wasm_bindgen_test]
async fn an_eager_host_function_can_be_called_directly() {
    let mut store = Store::default();
    let log: Log = Rc::default();
    let gate = Gate::new(&log);
    let wait = gate.function(&mut store, true);
    let store = store.into_async();
    let call = |id: i32| -> Call { Box::pin(wait.call_async(&store, vec![Value::I32(id)])) };
    for id in [READY, AUTO, YIELD] {
        assert_eq!(i32_of(call(id).await), id);
    }
    let error = call(FAIL).await.unwrap_err();
    assert_eq!(error.downcast_ref(), Some(&HostExit(FAIL)));
    assert!(call(8).now_or_never().is_none());
    assert_eq!(gate.dropped(), 1);
    assert!(store.into_store().is_ok());
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
    let kinds = signatures
        .iter()
        .flat_map(|signature| [(false, signature), (true, signature)]);
    for (eager, (params, results)) in kinds {
        let ty = FunctionType::new(params.to_vec(), results.to_vec());
        let case = format!("{ty}, eager: {eager}");
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
        let f = if eager { f.eager() } else { f };
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
        // polled, which for an eager host function is inside the import.
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
        let get = Function::new_async(store, FunctionType::new([], results), move |_| {
            let outcome = outcomes.borrow_mut().take().unwrap();
            async move {
                let function = Value::FuncRef(outcome.await.unwrap().map_err(Box::from)?);
                Ok(if two {
                    vec![Value::I64(1), function]
                } else {
                    vec![function]
                })
            }
        });
        // One of each kind of host function.
        if two { get.eager() } else { get }
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
