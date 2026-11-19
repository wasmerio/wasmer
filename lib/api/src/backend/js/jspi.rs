//! Async calls on the JavaScript backend, through JSPI.
//!
//! One [`Activation`] per `Function::call_async`, owned by the future of that
//! call. The engine suspends and resumes the guest's stack; this module keeps
//! what the engine does not know: which call a guest belongs to, who holds
//! its store while it runs, and what it is waiting for.
//!
//! * A guest imports an [`adapter`] in place of an async host function. Its
//!   hooks ([`start`], `resume` and `unwait`) are ordinary imports that run on
//!   the guest's own stack, so every change of state happens exactly where the
//!   guest leaves or re-enters the stack. The only `WebAssembly.Suspending`
//!   import is `wait`, which has no state.
//! * The store's write lock is held by the activation while its guest runs:
//!   taken by the call, released in `start`, taken again in `resume`. The
//!   store is free while the guest is suspended, and nothing is kept on the
//!   thread's store context stack, which is last-in first-out when
//!   suspensions are not.
//! * The host future of an async import belongs to the activation and is
//!   polled by the future of the call, with the caller's waker. When it is
//!   finished the guest is woken, and `resume` hands over the result.
//! * An *eager* host function ([`start_eager`]) is polled once in `start`
//!   itself, with the store lent to it. If it is ready the guest gets its
//!   results there and is never suspended; if not, it goes on as above.
//! * Dropping the call drops the host future at once and wakes the guest,
//!   whose adapter then traps: the stack unwinds without running guest code,
//!   and the engine frees it.

mod adapter;

use std::{
    cell::{Cell, RefCell},
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

use js_sys::{Array, Function, Reflect, WebAssembly};
use wasm_bindgen::{
    JsCast, JsValue,
    prelude::{Closure, wasm_bindgen},
};
use wasmer_types::{FunctionType, StoreId, TrapCode};

use crate::{
    AsStoreAsync, AsStoreMut, AsStoreRef, BackendTrap, RuntimeError, StoreAsync,
    StoreAsyncWriteLock, StoreMut, Value,
    js::{
        error::Trap,
        function::{lent_store, numeric},
        utils::convert::AsJs as _,
    },
};

/// The host's half of an async import: the host function and what it awaits.
/// Nothing of it has run until it is polled.
pub(crate) type ImportFuture = Pin<Box<dyn Future<Output = Result<Vec<Value>, RuntimeError>>>>;

type LockFuture = Pin<Box<dyn Future<Output = StoreAsyncWriteLock>>>;

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    /// The guest is on the stack, or has left it for good and the reaction on
    /// its promise has not run yet. The activation holds the store.
    Running,
    /// The guest is on the stack, in the `start` of an eager host function
    /// whose future is being polled. The store is lent to that poll.
    Lent,
    /// `start` was called: the guest is in `wait`, or about to be. The store
    /// is free.
    Suspended,
    /// The adapter was told to trap. The guest runs no more and holds nothing.
    Unwinding,
    /// The guest's promise settled.
    Done,
}

/// One `Function::call_async`.
struct Activation {
    token: i32,
    store_id: StoreId,
    state: Cell<State>,
    /// The call was dropped: its guest must not run again.
    cancelled: Cell<bool>,
    /// The store, until the guest can no longer need it.
    store: RefCell<Option<StoreAsync>>,
    /// The store's write lock, while the guest runs.
    lock: RefCell<Option<StoreAsyncWriteLock>>,
    /// The import the guest is suspended in, until it is finished.
    import: RefCell<Option<ImportFuture>>,
    /// What the import came to, from then until `resume` hands it over.
    outcome: RefCell<Option<Result<Vec<Value>, RuntimeError>>>,
    /// Set when `resume` found the store taken: the guest waits for it.
    relock: RefCell<Option<LockFuture>>,
    /// The error of a host function, kept as it is across the adapter's trap.
    error: RefCell<Option<RuntimeError>>,
    /// How the guest's promise settled.
    result: RefCell<Option<Result<JsValue, JsValue>>>,
    /// Whoever awaits the call: the waker of its last poll.
    caller: RefCell<Option<Waker>>,
    /// The call is being polled and is entering the guest. It looks at the
    /// activation again when the guest leaves the stack, and need not be woken.
    entering: Cell<bool>,
    /// Counts the waits of the guest, so that [`accepted`] is told which one
    /// the engine accepted.
    waits: Cell<u32>,
    /// The engine suspended the guest for its current wait: what it waits for
    /// may be driven. Until then the guest may still be on the stack, and the
    /// engine may yet refuse to suspend it.
    accepted: Cell<bool>,
}

struct Activations {
    slots: Vec<Option<Rc<Activation>>>,
    free: Vec<i32>,
}

thread_local! {
    /// Every activation that is not done, by token.
    static ACTIVATIONS: RefCell<Activations> = const {
        RefCell::new(Activations {
            slots: Vec::new(),
            free: Vec::new(),
        })
    };
    /// The activations that hold a store: those whose guest is running. The
    /// write lock is exclusive, so there is at most one per store, and a guest
    /// that is suspended is not in here, so there are few.
    static RUNNING: RefCell<Vec<Rc<Activation>>> = const { RefCell::new(Vec::new()) };
    static SHIM: Shim = Shim::new();
}

/// The JavaScript half: promises can only be made and settled there. One per
/// thread.
struct Shim {
    status: WebAssembly::Global,
    not_ready: JsValue,
    undefined: JsValue,
    wait: JsValue,
    unwait: JsValue,
    /// `resume` for a host function on the numeric lane, which the adapter
    /// imports as it is.
    resume_number: JsValue,
    /// Wraps a guest's result in an object JavaScript does not take for a
    /// promise; see [`adapter::entry`].
    envelope: JsValue,
    hooks: ShimFunction,
    prepare: ShimFunction,
    prepare_woken: ShimFunction,
    accept_later: ShimFunction,
    wake: ShimFunction,
    settle: ShimFunction,
}

/// `wait` suspends the guest on the promise `prepare` made for its token,
/// which `wake` resolves. It keeps nothing: an engine that calls it and then
/// refuses to suspend has changed nothing. `prepareWoken` makes a promise
/// that is resolved already. `acceptLater` calls `accepted` in a microtask,
/// which runs once the guest has left the stack: suspended, or, if the engine
/// refused, after `unwait` has undone the wait.
///
/// `hooks` makes the `start` and `resume` imports of one host function from
/// their Rust halves, which do not always have results to return:
///
/// * `start` returns nothing: the guest always waits, as the status set;
/// * the `start` of an eager host function returns its results, or `undefined`
///   after setting the status itself (`isUndefined` stands for a result that
///   is `undefined`);
/// * `resume` returns its results, or `notReady` after setting the status.
#[wasm_bindgen(inline_js = r#"
export function makeShim(done, resume, accepted) {
  const promises = [], resolvers = [], woken = Promise.resolve();
  const notReady = Symbol(), isUndefined = Symbol();
  return {
    notReady,
    isUndefined,
    wait: new WebAssembly.Suspending((token) => promises[token]),
    prepare(token) {
      promises[token] = new Promise((resolve) => { resolvers[token] = resolve; });
    },
    prepareWoken(token) {
      promises[token] = woken;
      resolvers[token] = undefined;
    },
    acceptLater(token, wait) {
      queueMicrotask(() => accepted(token, wait));
    },
    envelope: (value) => ({ __proto__: null, value }),
    wake(token) {
      const resolve = resolvers[token];
      resolvers[token] = undefined;
      if (resolve !== undefined) resolve();
    },
    settle(promise, token) {
      promise.then((value) => done(token, 0, value), (error) => done(token, 1, error));
    },
    hooks(start, placeholder, eager) {
      return {
        start: eager
          ? (...args) => {
              const results = start(args);
              if (results === undefined) return placeholder;
              return results === isUndefined ? undefined : results;
            }
          : (...args) => {
              start(args);
              return placeholder;
            },
        resume(token) {
          const results = resume(token);
          return results === notReady ? placeholder : results;
        },
      };
    },
  };
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = makeShim)]
    fn make_shim(done: &JsValue, resume: &JsValue, accepted: &JsValue) -> JsValue;
}

impl Shim {
    fn new() -> Self {
        let descriptor = js_sys::Object::new();
        Reflect::set(&descriptor, &"value".into(), &"i32".into()).unwrap();
        Reflect::set(&descriptor, &"mutable".into(), &true.into()).unwrap();
        let status = WebAssembly::Global::new(&descriptor, &adapter::READY.into()).unwrap();
        let done = Closure::wrap(Box::new(done) as Box<dyn Fn(i32, i32, JsValue)>).into_js_value();
        let resume =
            Closure::wrap(Box::new(resume) as Box<dyn Fn(i32) -> JsValue>).into_js_value();
        let accepted =
            Closure::wrap(Box::new(accepted) as Box<dyn Fn(i32, i32)>).into_js_value();
        // Static JavaScript, not made from a string: some hosts forbid that.
        let shim = make_shim(&done, &resume, &accepted);
        let get = |name: &str| Reflect::get(&shim, &name.into()).unwrap();
        Self {
            status,
            not_ready: get("notReady"),
            undefined: get("isUndefined"),
            wait: get("wait"),
            envelope: get("envelope"),
            unwait: Closure::wrap(Box::new(unwait) as Box<dyn Fn(i32) -> Result<(), JsValue>>)
                .into_js_value(),
            resume_number: Closure::wrap(Box::new(resume_number) as Box<dyn Fn(i32) -> f64>)
                .into_js_value(),
            hooks: get("hooks").unchecked_into(),
            prepare: get("prepare").unchecked_into(),
            prepare_woken: get("prepareWoken").unchecked_into(),
            accept_later: get("acceptLater").unchecked_into(),
            wake: get("wake").unchecked_into(),
            settle: get("settle").unchecked_into(),
        }
    }

    /// Makes the promise the guest of `token` is to wait on: a `wake` from
    /// now on resolves it.
    fn prepare(token: i32) {
        SHIM.with(|shim| shim.prepare.call_token(&JsValue::UNDEFINED, token));
    }

    /// As `prepare`, for a guest that has nothing to wait for: the promise is
    /// resolved already, and the guest resumed in a later microtask.
    fn prepare_woken(token: i32) {
        SHIM.with(|shim| shim.prepare_woken.call_token(&JsValue::UNDEFINED, token));
    }

    /// Has [`accepted`] told, in a microtask, about the wait `wait` of the
    /// guest of `token`.
    fn accept_later(token: i32, wait: u32) {
        SHIM.with(|shim| {
            let accept = &shim.accept_later;
            accept.call_accept(&JsValue::UNDEFINED, token, wait as i32)
        });
    }

    /// Tells the adapter that its guest is to wait as `token`. The last thing
    /// a hook does: the adapter reads `$status` next.
    fn set_status(token: i32) {
        SHIM.with(|shim| shim.status.set_value(&token.into()));
    }

    /// Resumes the guest of `token`, in a later microtask. Does nothing when
    /// it was woken already.
    fn wake(token: i32) {
        SHIM.with(|shim| shim.wake.call_token(&JsValue::UNDEFINED, token));
    }

    /// Tells the adapter to trap.
    fn trap() {
        SHIM.with(|shim| shim.status.set_value(&adapter::TRAP.into()));
    }

    /// What the Rust half of `resume` returns after `pend` or `trap`.
    fn not_ready() -> JsValue {
        SHIM.with(|shim| shim.not_ready.clone())
    }

    /// The results of an import, for the JavaScript half of a hook.
    fn results(values: &[Value], store: &impl AsStoreRef) -> JsValue {
        match values {
            [] => JsValue::UNDEFINED,
            [value] => value.as_jsvalue(store),
            values => Array::from_iter(values.iter().map(|value| value.as_jsvalue(store))).into(),
        }
    }

}

/// What the JavaScript half of the `start` of an eager host function is
/// given for the results [`start_eager`] got; `undefined` stands for none
/// yet.
pub(crate) fn eager_results(values: &[Value], store: &impl AsStoreRef) -> JsValue {
    let results = Shim::results(values, store);
    if results.is_undefined() && !values.is_empty() {
        // A result that is `undefined` itself.
        return SHIM.with(|shim| shim.undefined.clone());
    }
    results
}

impl Activation {
    fn set_state(&self, from: State, to: State) {
        debug_assert_eq!(self.state.get(), from, "JSPI activation {}", self.token);
        self.state.set(to);
    }

    /// The guest goes on, with the store.
    fn switch_in(self: &Rc<Self>, lock: StoreAsyncWriteLock) {
        debug_assert!(running(self.store_id).is_none(), "two guests run in one store");
        *self.lock.borrow_mut() = Some(lock);
        RUNNING.with(|running| running.borrow_mut().push(self.clone()));
        self.state.set(State::Running);
    }

    /// The guest leaves the stack, or will not use the store again.
    fn switch_out(self: &Rc<Self>, to: State) {
        let this = RUNNING.with(|running| {
            let mut running = running.borrow_mut();
            let index = running.iter().position(|other| Rc::ptr_eq(other, self))?;
            Some(running.swap_remove(index))
        });
        drop(this);
        self.state.set(to);
        // Last: this wakes whoever waits for the store.
        drop(self.lock.take());
    }

    /// The store's write lock, if nobody holds it.
    fn try_lock(&self) -> Option<StoreAsyncWriteLock> {
        let inner = self.store.borrow().as_ref()?.inner.try_write()?;
        Some(StoreAsyncWriteLock { inner })
    }

    fn wake_caller(&self) {
        if self.entering.get() {
            return;
        }
        // Not woken under the borrow: a waker may do anything.
        let caller = self.caller.borrow().clone();
        if let Some(caller) = caller {
            caller.wake();
        }
    }

    /// Switches the guest out with nothing to wait for but its own
    /// resumption, where `resume` deals with `outcome`: there, and not here,
    /// because only a guest that is resumed is known to be one that can be
    /// unwound.
    fn park(self: &Rc<Self>, outcome: Option<Result<Vec<Value>, RuntimeError>>) {
        *self.outcome.borrow_mut() = outcome;
        self.wait(true);
    }

    /// The guest is to wait for what the activation holds: an import, a
    /// relock, or, `woken`, nothing but its resumption. In this order, which
    /// matters when what runs in between wakes or polls the call: the promise
    /// the guest waits on is made, so that any wake from then on reaches it;
    /// the store is given up, which wakes whoever waits for it; the call is to
    /// be woken to drive the wait once the engine has suspended the guest; and
    /// the adapter is told to wait, last, as it reads that next.
    fn wait(self: &Rc<Self>, woken: bool) {
        if woken {
            Shim::prepare_woken(self.token);
        } else {
            Shim::prepare(self.token);
        }
        let wait = self.waits.get().wrapping_add(1);
        self.waits.set(wait);
        self.accepted.set(false);
        self.switch_out(State::Suspended);
        if !woken {
            Shim::accept_later(self.token, wait);
        }
        Shim::set_status(self.token);
    }

    /// Drives what a suspended guest waits for. Called by the future of the
    /// call, so with the caller's waker, on the caller's stack.
    fn drive(&self, cx: &mut Context<'_>) {
        // A wait the engine has not accepted is not driven: the guest may
        // still be on the stack, and the engine may yet refuse to suspend it,
        // in which case nothing of the host function may have run.
        if self.state.get() != State::Suspended || !self.accepted.get() {
            return;
        }
        if let Some(mut relock) = self.relock.take() {
            match relock.as_mut().poll(cx) {
                Poll::Ready(lock) => {
                    *self.lock.borrow_mut() = Some(lock);
                    Shim::wake(self.token);
                }
                Poll::Pending => *self.relock.borrow_mut() = Some(relock),
            }
        } else if let Some(mut import) = self.import.take() {
            match import.as_mut().poll(cx) {
                Poll::Ready(outcome) => {
                    *self.outcome.borrow_mut() = Some(outcome);
                    Shim::wake(self.token);
                }
                Poll::Pending => *self.import.borrow_mut() = Some(import),
            }
        }
    }

    /// The end of the activation: its guest returned, trapped or was unwound.
    fn retire(self: &Rc<Self>, result: Result<JsValue, JsValue>) {
        ACTIVATIONS.with(|activations| {
            let mut activations = activations.borrow_mut();
            activations.slots[(self.token - adapter::FIRST_TOKEN) as usize] = None;
            activations.free.push(self.token);
        });
        self.switch_out(State::Done);
        drop(self.import.take());
        drop(self.outcome.take());
        drop(self.relock.take());
        drop(self.store.take());
        *self.result.borrow_mut() = Some(result);
        self.wake_caller();
    }
}

fn activation(token: i32) -> Option<Rc<Activation>> {
    ACTIVATIONS.with(|activations| {
        activations
            .borrow()
            .slots
            .get((token - adapter::FIRST_TOKEN) as usize)?
            .clone()
    })
}

/// The activation whose guest is running in the store `id`.
fn running(id: StoreId) -> Option<Rc<Activation>> {
    RUNNING.with(|running| {
        let running = running.borrow();
        running.iter().find(|activation| activation.store_id == id).cloned()
    })
}

/// The store of `id`, if a guest entered by `call_async` is running in it.
pub(crate) fn current_store(id: StoreId) -> Option<StoreAsync> {
    running(id)?.store.borrow().as_ref().map(StoreAsync::store)
}

/// The error of an async host function whose guest cannot be suspended: it
/// was not entered by `call_async`, or it was, with a synchronous call or a
/// JavaScript frame on the stack since. The same error as on other backends.
fn cannot_suspend() -> JsValue {
    let code = TrapCode::YieldOutsideAsyncContext;
    let source = BackendTrap::Js(Trap::user(code.message().into()));
    let error = RuntimeError::new_from_source(source, Vec::new(), Some(code));
    // Thrown as host functions throw their errors, so that the caller gets
    // this very error back.
    Trap::user(Box::new(error)).into()
}

/// The error of an eager host function whose guest was entered synchronously
/// with another store than the function's: the function's store is not lent
/// to the guest.
pub(crate) fn not_entered_with_its_store() -> RuntimeError {
    RuntimeError::new(
        "an async host function was called by a guest that was not entered with its store",
    )
}

/// The Rust half of the adapter's `start` import: a guest called an async host
/// function of the store `id`. Switches the guest out, and returns the token
/// of the activation it is to wait as, negated when it has nothing to wait
/// for. Nothing of the host function runs here.
pub(crate) fn start(
    id: StoreId,
    import: impl FnOnce(StoreAsync) -> ImportFuture,
) -> Result<(), JsValue> {
    // Whoever holds the store is the call this guest belongs to.
    let Some(activation) = running(id) else {
        return Err(cannot_suspend());
    };
    debug_assert_eq!(activation.state.get(), State::Running);
    let store = activation.store.borrow().as_ref().map(StoreAsync::store);
    let Some(store) = store.filter(|_| !activation.cancelled.get()) else {
        // The call was dropped while its guest ran.
        activation.park(None);
        return Ok(());
    };
    *activation.import.borrow_mut() = Some(import(store));
    activation.wait(false);
    Ok(())
}

/// The Rust half of the `start` import of an eager host function: polls the
/// host future once, here, on the guest's stack. Returns the results if it is
/// ready, and nothing after switching the guest out if it is not.
///
/// The store is lent to that poll: the activation gives up the write lock for
/// it, so the host function takes whatever lock it needs at once, and takes
/// the lock back when the future is ready. A future that is not ready keeps
/// the locks it holds, and the guest waits for the store in `resume`.
///
/// With no call the guest could be suspended as, the future is polled all
/// the same, and dropped if it is not ready: the import fails as that of any
/// guest that cannot wait. `import` is then given no store, and borrows the
/// one the synchronous call that entered the guest lent it (see
/// [`lent_store`]). If that is another store, the import fails: the guest was
/// entered with another store.
///
/// The results go to `results` with the store, which the guest holds again.
pub(crate) fn start_eager<R>(
    id: StoreId,
    import: impl FnOnce(Option<StoreAsync>) -> ImportFuture,
    results: impl FnOnce(&[Value], &mut StoreMut<'_>) -> R,
) -> Result<Option<R>, JsValue> {
    let Some(activation) = running(id) else {
        let mut import = import(None);
        let outcome = import.as_mut().poll(&mut Context::from_waker(Waker::noop()));
        drop(import);
        return match outcome {
            Poll::Ready(Ok(values)) => {
                let Some(store) = lent_store(id) else {
                    return Err(Trap::user(Box::new(not_entered_with_its_store())).into());
                };
                // SAFETY: the pointer of the borrow of the call that entered
                // the guest, which is still on the stack.
                Ok(Some(results(&values, &mut unsafe { StoreMut::from_raw(store) })))
            }
            // Thrown, as a synchronous host function throws its error.
            Poll::Ready(Err(error)) => Err(Trap::user(Box::new(error)).into()),
            Poll::Pending => Err(cannot_suspend()),
        };
    };
    debug_assert_eq!(activation.state.get(), State::Running);
    let store = activation.store.borrow().as_ref().map(StoreAsync::store);
    let Some(store) = store.filter(|_| !activation.cancelled.get()) else {
        activation.park(None);
        return Ok(None);
    };
    // The caller's waker, so that the future wakes whoever will poll it from
    // here on. A call always has one by now: it is polled to be entered.
    let caller = activation.caller.borrow().clone();
    let caller = caller.unwrap_or_else(|| Waker::noop().clone());
    activation.switch_out(State::Lent);
    let mut import = import(Some(store));
    let outcome = match import.as_mut().poll(&mut Context::from_waker(&caller)) {
        Poll::Pending if !activation.cancelled.get() => {
            // From here on as any host future: the call polls it.
            *activation.import.borrow_mut() = Some(import);
            activation.wait(false);
            return Ok(None);
        }
        // The call was dropped during the poll.
        Poll::Pending => None,
        Poll::Ready(_) if activation.cancelled.get() => None,
        Poll::Ready(Ok(values)) => match activation.try_lock() {
            Some(mut lock) => {
                let results = results(&values, &mut lock.as_store_mut());
                activation.switch_in(lock);
                return Ok(Some(results));
            }
            // The store was not given back: the guest waits for it.
            None => Some(Ok(values)),
        },
        Poll::Ready(Err(error)) => Some(Err(error)),
    };
    drop(import);
    activation.park(outcome);
    Ok(None)
}

/// The adapter's `resume` import, less the results' way back: the guest is
/// back on the stack. Returns the results of its import with the activation
/// that holds the store again, or nothing after telling the adapter to trap
/// or to wait again.
fn resumed(token: i32) -> Option<(Rc<Activation>, Vec<Value>)> {
    let activation = activation(token).expect("a suspended guest outlived its call");
    debug_assert_eq!(activation.state.get(), State::Suspended);
    activation.accepted.set(false);
    let outcome = activation.outcome.take();
    debug_assert!(outcome.is_some() || activation.cancelled.get());
    let results = match outcome {
        Some(Ok(results)) if !activation.cancelled.get() => results,
        outcome => {
            // Cancelled, or the host function failed. Either way the guest
            // is unwound from here, without the store.
            if let Some(Err(error)) = outcome {
                *activation.error.borrow_mut() = Some(error);
            }
            activation.set_state(State::Suspended, State::Unwinding);
            Shim::trap();
            return None;
        }
    };
    let lock = activation.lock.take().or_else(|| activation.try_lock());
    let Some(lock) = lock else {
        // Somebody has the store. The guest waits again, this time for the
        // write lock, which the call acquires for it.
        let store = activation.store.borrow().as_ref().map(StoreAsync::store);
        let store = store.expect("a guest that can run has its store");
        *activation.outcome.borrow_mut() = Some(Ok(results));
        *activation.relock.borrow_mut() = Some(Box::pin(async move { store.write_lock().await }));
        activation.wait(false);
        return None;
    };
    activation.switch_in(lock);
    Some((activation, results))
}

/// The Rust half of the adapter's `resume` import. Returns the results of the
/// import, or `notReady`.
fn resume(token: i32) -> JsValue {
    let Some((activation, results)) = resumed(token) else {
        return Shim::not_ready();
    };
    let lock = activation.lock.borrow();
    Shim::results(&results, lock.as_ref().expect("a resumed guest has its store"))
}

/// The adapter's `resume` import for a host function on the numeric lane.
/// Returns the result of the import, if it has one and it is there.
fn resume_number(token: i32) -> f64 {
    match resumed(token) {
        Some((_, results)) if !results.is_empty() => numeric::result(&results),
        _ => 0.0,
    }
}

/// The adapter's `unwait` import: the engine refused to suspend the guest,
/// which is still running. Takes back what `start` did, and fails the import.
fn unwait(token: i32) -> Result<(), JsValue> {
    let Some(activation) = activation(token) else {
        return Ok(());
    };
    debug_assert_eq!(activation.state.get(), State::Suspended);
    activation.accepted.set(false);
    // The host future has not run, or has been polled once if it is that of
    // an eager host function. It goes first: it may hold the store.
    drop(activation.import.take());
    drop(activation.relock.take());
    let outcome = activation.outcome.take();
    match activation.lock.take().or_else(|| activation.try_lock()) {
        Some(lock) => activation.switch_in(lock),
        // Nothing runs between `start` and here that could take the store,
        // but a host future may have kept it. The guest then goes on as one
        // that was not entered by `call_async`.
        None => activation.state.set(State::Unwinding),
    }
    Err(match outcome {
        // The error of an eager host function, which nothing waited for: it
        // is thrown, as a synchronous host function throws its error.
        Some(Err(error)) => Trap::user(Box::new(error)).into(),
        _ => cannot_suspend(),
    })
}

/// The microtask of [`Shim::accept_later`]: if the guest of `token` is still
/// in its wait `wait`, the engine suspended it there, and the call is woken to
/// drive what it waits for.
fn accepted(token: i32, wait: i32) {
    let Some(activation) = activation(token) else {
        return;
    };
    let current = activation.waits.get() == wait as u32;
    if activation.state.get() != State::Suspended || !current || activation.accepted.get() {
        return;
    }
    activation.accepted.set(true);
    activation.wake_caller();
}

/// The reaction on the promise of every guest: `token` says whose it is.
fn done(token: i32, rejected: i32, value: JsValue) {
    if let Some(activation) = activation(token) {
        activation.retire(if rejected == 0 { Ok(value) } else { Err(value) });
    }
}

/// The caller's side of an activation. Dropping it cancels the call.
pub(crate) struct Call(Rc<Activation>);

impl Call {
    /// Enters a guest of `store` through `enter`, which calls a promising
    /// function. The guest runs with `lock`, the store's write lock.
    ///
    /// `caller` is the waker the future that makes this call is being polled
    /// with: the guest may reach an eager host function before the call is
    /// polled itself.
    pub(crate) fn enter(
        store: &StoreAsync,
        lock: StoreAsyncWriteLock,
        caller: Waker,
        enter: impl FnOnce() -> Result<JsValue, JsValue>,
    ) -> Self {
        let activation = ACTIVATIONS.with(|activations| {
            let mut activations = activations.borrow_mut();
            let token = activations.free.pop().unwrap_or_else(|| {
                activations.slots.push(None);
                activations.slots.len() as i32 - 1 + adapter::FIRST_TOKEN
            });
            let activation = Rc::new(Activation {
                token,
                store_id: store.id,
                state: Cell::new(State::Running),
                cancelled: Cell::new(false),
                store: RefCell::new(Some(store.store())),
                lock: RefCell::new(None),
                import: RefCell::new(None),
                outcome: RefCell::new(None),
                relock: RefCell::new(None),
                error: RefCell::new(None),
                result: RefCell::new(None),
                caller: RefCell::new(Some(caller)),
                entering: Cell::new(true),
                waits: Cell::new(0),
                accepted: Cell::new(false),
            });
            activations.slots[(token - adapter::FIRST_TOKEN) as usize] = Some(activation.clone());
            activation
        });
        activation.switch_in(lock);
        // A promising function returns a promise, or throws before it enters
        // the guest. The reaction is attached here, before anything else can
        // run: a rejection nobody handles ends the process on some hosts.
        match enter() {
            Ok(promise) => SHIM.with(|shim| {
                shim.settle
                    .call_settle(&JsValue::UNDEFINED, &promise, activation.token)
            }),
            Err(error) => activation.retire(Err(error)),
        }
        // Back from the guest: if it waits, the engine suspended it.
        if activation.state.get() == State::Suspended {
            activation.accepted.set(true);
        }
        activation.entering.set(false);
        Self(activation)
    }
}

impl Future for Call {
    type Output = Result<JsValue, RuntimeError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let activation = &self.0;
        {
            let mut caller = activation.caller.borrow_mut();
            if !caller.as_ref().is_some_and(|caller| caller.will_wake(cx.waker())) {
                *caller = Some(cx.waker().clone());
            }
        }
        activation.drive(cx);
        match activation.result.take() {
            Some(result) => Poll::Ready(result.map_err(|error| {
                // The trap of a failed host function stands for its error.
                activation.error.take().unwrap_or_else(|| error.into())
            })),
            None => Poll::Pending,
        }
    }
}

impl Drop for Call {
    fn drop(&mut self) {
        let activation = &self.0;
        drop(activation.caller.take());
        if activation.state.get() == State::Done {
            return;
        }
        activation.cancelled.set(true);
        if activation.state.get() == State::Suspended {
            // The guest will not run again: `resume` makes its adapter trap.
            // So the host future goes now, and with it everything that keeps
            // the store from being taken back.
            drop(activation.import.take());
            drop(activation.outcome.take());
            drop(activation.relock.take());
            drop(activation.lock.take());
            drop(activation.store.take());
            Shim::wake(activation.token);
        }
        // A guest that is running keeps the store until it returns or calls
        // an async import, which traps.
    }
}

/// Wraps `start`, the function that begins an async host function of type `ty`
/// by calling [`start`], or [`start_eager`] if it is `eager`, in the adapter a
/// guest imports.
///
/// A `start` on the numeric lane (`numeric`) takes and returns numbers and
/// sets the status itself, with [`pend`]: the adapter imports it as it is.
pub(crate) fn adapter(
    ty: &FunctionType,
    start: &JsValue,
    eager: bool,
    numeric: bool,
) -> Result<Function, JsValue> {
    let module = adapter::module(ty)?;
    SHIM.with(|shim| {
        if numeric {
            let hooks = [start, &shim.wait, &shim.resume_number, &shim.unwait];
            return adapter::instantiate(&module, hooks, &shim.status);
        }
        let placeholder = adapter::placeholder(ty);
        let hooks = shim
            .hooks
            .call_hooks(&JsValue::UNDEFINED, start, &placeholder, eager);
        let hook = |name: &str| Reflect::get(&hooks, &name.into());
        let hooks = [&hook("start")?, &shim.wait, &hook("resume")?, &shim.unwait];
        adapter::instantiate(&module, hooks, &shim.status)
    })
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = WebAssembly, js_name = promising, catch)]
    fn promising_raw(function: &Function) -> Result<Function, JsValue>;

    /// A function of the shim, called with typed arguments: a token crosses
    /// as a number, without a `JsValue` being made for it.
    type ShimFunction;
    #[wasm_bindgen(method, js_name = call)]
    fn call_token(this: &ShimFunction, receiver: &JsValue, token: i32);
    #[wasm_bindgen(method, js_name = call)]
    fn call_settle(this: &ShimFunction, receiver: &JsValue, promise: &JsValue, token: i32);
    #[wasm_bindgen(method, js_name = call)]
    fn call_accept(this: &ShimFunction, receiver: &JsValue, token: i32, wait: i32);
    #[wasm_bindgen(method, js_name = call)]
    fn call_hooks(
        this: &ShimFunction,
        receiver: &JsValue,
        start: &JsValue,
        placeholder: &JsValue,
        eager: bool,
    ) -> JsValue;
}

pub(crate) fn is_supported() -> bool {
    let Ok(webassembly) = Reflect::get(&js_sys::global(), &JsValue::from_str("WebAssembly")) else {
        return false;
    };
    Reflect::get(&webassembly, &JsValue::from_str("promising"))
        .is_ok_and(|value| value.is_function())
        && Reflect::get(&webassembly, &JsValue::from_str("Suspending"))
            .is_ok_and(|value| value.is_function())
}

/// The function `call_async` calls to enter `function`, of type `ty`:
/// `WebAssembly.promising` of it, or of the [`adapter::entry`] around it if
/// its result could be taken for a promise. [`result`] reads what the call
/// returns.
pub(crate) fn promising(function: &Function, ty: &FunctionType) -> Result<Function, JsValue> {
    if !adapter::enters_through_an_entry(ty) {
        return promising_raw(function);
    }
    let entry = SHIM.with(|shim| adapter::entry(function, ty, &shim.envelope))?;
    promising_raw(&entry)
}

/// What a guest of type `ty` returned, from what its promising function
/// settled with.
pub(crate) fn result(ty: &FunctionType, settled: JsValue) -> Result<JsValue, JsValue> {
    if !adapter::enters_through_an_entry(ty) {
        return Ok(settled);
    }
    Reflect::get(&settled, &"value".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Function, Store, Type};
    use js_sys::Promise;
    use wasm_bindgen_futures::JsFuture;
    use wasm_bindgen_test::wasm_bindgen_test;

    async fn tick() {
        JsFuture::from(Promise::resolve(&JsValue::UNDEFINED))
            .await
            .unwrap();
    }

    /// The activations there are, and how many of them hold their store.
    fn live() -> (usize, usize) {
        let activations =
            ACTIVATIONS.with(|activations| activations.borrow().slots.iter().flatten().count());
        (activations, RUNNING.with(|running| running.borrow().len()))
    }

    /// However a call ends, nothing of it stays in this module's state, and
    /// its token is used again. A call that never waits holds its store until
    /// the engine has settled its promise.
    #[wasm_bindgen_test]
    async fn a_call_that_ended_leaves_nothing_behind() {
        let mut store = Store::default();
        // Fails when asked to, after waiting for a tick.
        let ty = FunctionType::new([Type::I32], []);
        let host = Function::new_async(&mut store, ty, |arguments| {
            let fail = arguments[0].unwrap_i32() != 0;
            async move {
                tick().await;
                if fail {
                    return Err(RuntimeError::new("failed"));
                }
                Ok(vec![])
            }
        });
        let eager = host.clone().eager();
        let store = store.into_async();
        assert_eq!(live(), (0, 0));

        for round in 0..4 {
            // As it is, and eager, by turns.
            let host = [&host, &eager][round % 2];
            let call = |fail: i32| Box::pin(host.call_async(&store, vec![Value::I32(fail)]));

            // Returns.
            let mut returning = call(0);
            let mut cx = Context::from_waker(Waker::noop());
            assert!(returning.as_mut().poll(&mut cx).is_pending());
            assert_eq!(live(), (1, 0), "round {round}: suspended, the store is free");
            returning.await.unwrap();
            assert_eq!(live(), (0, 0), "round {round}: returned");

            // Fails.
            call(1).await.unwrap_err();
            assert_eq!(live(), (0, 0), "round {round}: failed");

            // Is cancelled: its guest is still to be unwound.
            let mut cancelled = call(0);
            assert!(cancelled.as_mut().poll(&mut cx).is_pending());
            drop(cancelled);
            assert_eq!(live(), (1, 0), "round {round}: cancelled");
            for _ in 0..3 {
                tick().await;
            }
            assert_eq!(live(), (0, 0), "round {round}: unwound");

            // Cannot be suspended.
            let mut lock = store.write_lock().await;
            host.call(&mut lock, &[Value::I32(0)]).unwrap_err();
            drop(lock);
            assert_eq!(live(), (0, 0), "round {round}: refused");
        }
        let tokens = ACTIVATIONS.with(|activations| activations.borrow().slots.len());
        assert_eq!(tokens, 1, "tokens are not used again");

        let ready = Function::new_typed_async(&mut store.write_lock().await, async || 7);
        let mut call = Box::pin(ready.eager().call_async(&store, vec![]));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(call.as_mut().poll(&mut cx).is_pending());
        assert_eq!(live(), (1, 1), "returned without waiting");
        assert_eq!(call.await.unwrap().to_vec(), [Value::I32(7)]);
        assert_eq!(live(), (0, 0));
    }

    /// `WebAssembly.promising` is called for the first `call_async` of a
    /// function, and never again.
    #[wasm_bindgen_test]
    async fn the_promising_wrapper_of_a_function_is_kept() {
        let mut store = Store::default();
        let host = Function::new_typed_async(&mut store, async |value: i32| value);
        let again = host.clone();
        let store = store.into_async();
        let key = js_sys::Symbol::for_("wasmer.promising");
        let promising = |function: &Function| {
            Reflect::get(&function.as_js().handle.function, &key)
                .unwrap()
                .dyn_into::<js_sys::Function>()
                .ok()
        };
        assert!(promising(&host).is_none());

        host.call_async(&store, vec![Value::I32(1)]).await.unwrap();
        let first = promising(&host).expect("made by the first call");
        again.call_async(&store, vec![Value::I32(2)]).await.unwrap();
        assert!(js_sys::Object::is(&first, &promising(&host).unwrap()));
        assert!(js_sys::Object::is(&first, &promising(&again).unwrap()));
    }
}
