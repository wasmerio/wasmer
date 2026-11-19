//! Async calls on the JavaScript backend, through JSPI.
//!
//! One [`Activation`] per `Function::call_async`, owned by the future of that
//! call. The engine suspends and resumes the guest's stack; this module keeps
//! what the engine does not know: which call a guest belongs to, who holds
//! its store while it runs, and what it is waiting for.
//!
//! * An async host function is a JavaScript function, [`host_function`],
//!   which a guest imports as a `WebAssembly.Suspending` function. Its body,
//!   [`start`], runs on the guest's stack: it keeps the host future, and
//!   returns the promise the guest waits on. Called directly by `call_async`,
//!   the function itself stands for the guest's promising function.
//! * The store's write lock is held by the activation while its guest runs:
//!   taken by the call, given up once the guest waits, and taken back before
//!   the guest is woken with its results. The store is free while the guest
//!   waits, and nothing is kept on the thread's store context stack, which is
//!   last-in first-out when suspensions are not.
//! * The host future of an async import belongs to the activation and is
//!   polled by the future of the call, with the caller's waker.
//! * The engine refuses to suspend a guest that was entered by a synchronous
//!   call, or that has another JavaScript frame on the stack since it was
//!   entered, and some engines run the body of the import before they
//!   refuse. So the body does nothing that cannot be undone: the store is
//!   given up, and the host future polled, only once a microtask after the
//!   guest's slice has seen it waiting ([`check`]). The engine's error for a
//!   refusal is taken for the one other backends give ([`refusal`]).
//! * A guest is woken in a microtask, unless its call is dropped by then
//!   ([`woken`]). A guest that must not run again, because its call was
//!   dropped or its host function failed, is woken with the error of a trap,
//!   which no exception handler of the guest catches: its stack unwinds
//!   without running guest code, and the engine frees it.
//! * A guest that leaves the stack for good gives up its store once its
//!   slice is over, whenever its promise settles: a promise waits for a
//!   thenable the guest returns, on engines that take one for a promise.

use std::{
    cell::{Cell, RefCell},
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

use js_sys::{Array, Function, Reflect};
use wasm_bindgen::{
    JsValue,
    prelude::{Closure, wasm_bindgen},
};
use wasmer_types::{FunctionType, StoreId, TrapCode, Type};

use crate::{
    AsStoreAsync, AsStoreRef, BackendTrap, RuntimeError, StoreAsync, StoreAsyncWriteLock,
    Value,
    js::{
        error::Trap,
        utils::convert::AsJs as _,
    },
};

/// The host's half of an async import: the host function and what it awaits.
/// Nothing of it has run until it is polled.
pub(crate) type ImportFuture = Pin<Box<dyn Future<Output = Result<Vec<Value>, RuntimeError>>>>;

type LockFuture = Pin<Box<dyn Future<Output = StoreAsyncWriteLock>>>;

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    /// The guest is on the stack. The activation holds the store.
    Running,
    /// The guest waits on its promise, or is about to: once the wait is
    /// [`accept`](Activation::accept)ed, the store is free and what the guest
    /// waits for is driven by the call.
    Waiting,
    /// The guest is given its results in a microtask, with the store, which
    /// the activation holds: see [`woken`].
    Woken,
    /// The guest left the stack for good, and its promise has not settled
    /// yet. It holds nothing.
    Exited,
    /// The guest was woken with a trap. It runs no more and holds nothing.
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
    /// The import the guest waits in, until it is finished.
    import: RefCell<Option<ImportFuture>>,
    /// The results of the import, while the guest waits for its store.
    results: RefCell<Option<Vec<Value>>>,
    /// Takes the store back for the guest, when it was not free.
    relock: RefCell<Option<LockFuture>>,
    /// The error of a host function, kept as it is across the guest's trap.
    error: RefCell<Option<RuntimeError>>,
    /// How the guest's promise settled.
    result: RefCell<Option<Result<JsValue, JsValue>>>,
    /// Whoever awaits the call: the waker of its last poll.
    caller: RefCell<Option<Waker>>,
    /// The call is being polled and is entering the guest. It looks at the
    /// activation again when the guest leaves the stack, and need not be woken.
    entering: Cell<bool>,
    /// Counts the waits of the guest, so that a microtask queued for one of
    /// them knows whether it is still the current one.
    waits: Cell<u32>,
    /// The engine suspended the guest for its current wait.
    accepted: Cell<bool>,
}

struct Activations {
    slots: Vec<Option<Rc<Activation>>>,
    free: Vec<i32>,
}

/// The first token; tokens index the activations.
const FIRST_TOKEN: i32 = 1;

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
    /// that waits is not in here, so there are few.
    static RUNNING: RefCell<Vec<Rc<Activation>>> = const { RefCell::new(Vec::new()) };
    /// Hands the JavaScript half its Rust callbacks, once per thread.
    static SHIM: () = shim_init(
        &Closure::wrap(Box::new(done) as Box<dyn Fn(i32, i32, JsValue)>).into_js_value(),
        &Closure::wrap(Box::new(check) as Box<dyn Fn(i32, i32)>).into_js_value(),
        &Closure::wrap(Box::new(woken) as Box<dyn Fn(i32, i32) -> bool>).into_js_value(),
    );
}

// The JavaScript half: promises can only be made and settled there. It is
// static JavaScript, not made from a string: some hosts forbid that.
//
// `wait` makes the promise the guest of a token waits on, which `wake`
// resolves with the results of its import, and `unwind` rejects with the
// error of a trap. A trap's error is the one thing a guest's exception
// handlers do not catch, also when JavaScript throws it; the module that
// makes one is a constant, `(module (func (export "trap") unreachable))`.
//
// A guest suspended at a `WebAssembly.Suspending` import resumes in a
// microtask once its promise is resolved. A microtask queued two hops after
// the import's body, or one hop after the promise is resolved, runs once the
// guest's next slice is over.
#[wasm_bindgen(inline_js = r#"
const resolvers = [], rejecters = [];
let done, check, woken, trapError;
export function jspiInit(d, c, w) {
  done = d; check = c; woken = w;
}
export function jspiTrapError() {
  if (trapError === undefined) {
    const bytes = new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0, 1, 4, 1, 96, 0, 0, 3, 2, 1, 0,
      7, 8, 1, 4, 116, 114, 97, 112, 0, 0, 10, 5, 1, 3, 0, 0, 11]);
    const { trap } = new WebAssembly.Instance(new WebAssembly.Module(bytes)).exports;
    try { trap(); } catch (error) { trapError = error; }
  }
  return trapError;
}
const SUSPENDING = Symbol.for("wasmer.suspending");
export function jspiHost(start) {
  const host = (...args) => start(args);
  host[SUSPENDING] = new WebAssembly.Suspending(host);
  return host;
}
export function jspiIsHost(value) {
  return typeof value === "function" && value[SUSPENDING] !== undefined;
}
export function jspiImport(value) {
  const suspending = typeof value === "function" ? value[SUSPENDING] : undefined;
  return suspending === undefined ? value : suspending;
}
export function jspiWait(token) {
  return new Promise((resolve, reject) => { resolvers[token] = resolve; rejecters[token] = reject; });
}
export function jspiWake(token, value) {
  const resolve = resolvers[token];
  resolvers[token] = rejecters[token] = undefined;
  if (resolve !== undefined) resolve(value);
}
export function jspiUnwind(token) {
  const reject = rejecters[token];
  resolvers[token] = rejecters[token] = undefined;
  if (reject !== undefined) reject(jspiTrapError());
}
export function jspiWakeLater(token, wait, value) {
  queueMicrotask(() => {
    if (!woken(token, wait)) return;
    jspiWake(token, value);
    queueMicrotask(() => check(token, wait));
  });
}
export function jspiCheckLater(token, wait) {
  queueMicrotask(() => queueMicrotask(() => check(token, wait)));
}
export function jspiIsSuspendError(value) {
  const SuspendError = WebAssembly.SuspendError;
  return typeof SuspendError === "function" && value instanceof SuspendError;
}
export function jspiSettle(promise, token) {
  promise.then((value) => done(token, 0, value), (error) => done(token, 1, error));
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = jspiInit)]
    fn shim_init(done: &JsValue, check: &JsValue, woken: &JsValue);
    #[wasm_bindgen(js_name = jspiTrapError)]
    fn trap_error() -> JsValue;
    #[wasm_bindgen(js_name = jspiHost)]
    fn shim_host(start: &JsValue) -> Function;
    #[wasm_bindgen(js_name = jspiIsHost)]
    fn shim_is_host(value: &JsValue) -> bool;
    #[wasm_bindgen(js_name = jspiImport)]
    fn shim_import(value: &JsValue) -> JsValue;
    #[wasm_bindgen(js_name = jspiWait)]
    fn shim_wait(token: i32) -> JsValue;
    #[wasm_bindgen(js_name = jspiWakeLater)]
    fn shim_wake_later(token: i32, wait: i32, value: &JsValue);
    #[wasm_bindgen(js_name = jspiUnwind)]
    fn shim_unwind(token: i32);
    #[wasm_bindgen(js_name = jspiCheckLater)]
    fn shim_check_later(token: i32, wait: i32);
    #[wasm_bindgen(js_name = jspiIsSuspendError)]
    fn shim_is_suspend_error(value: &JsValue) -> bool;
    #[wasm_bindgen(js_name = jspiSettle)]
    fn shim_settle(promise: &JsValue, token: i32);
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = WebAssembly, js_name = promising, catch)]
    fn promising_raw(function: &Function) -> Result<Function, JsValue>;
}

/// The results of an import, as a `WebAssembly.Suspending` function returns
/// them, or as its promise resolves with them.
pub(crate) fn results(values: &[Value], store: &impl AsStoreRef) -> JsValue {
    match values {
        [] => JsValue::UNDEFINED,
        [value] => value.as_jsvalue(store),
        values => Array::from_iter(values.iter().map(|value| value.as_jsvalue(store))).into(),
    }
}

/// The function of an async host function: it calls `start` with its
/// arguments in an array, which returns the promise the guest waits on. A
/// guest imports it as a `WebAssembly.Suspending` function, which it keeps
/// (see [`import`]).
///
/// # Panics
/// Panics if the signature has a type JavaScript has no value of.
pub(crate) fn host_function(ty: &FunctionType, start: &JsValue) -> Function {
    for ty in ty.params().iter().chain(ty.results()) {
        if matches!(ty, Type::V128 | Type::ExceptionRef) {
            panic!(
                "an async host function cannot take or return a `{ty}` on the JavaScript \
                 backend: JavaScript has no value of that type"
            );
        }
    }
    SHIM.with(|_| ());
    shim_host(start)
}

/// What a guest imports for `value`: the `WebAssembly.Suspending` form of an
/// async host function, and anything else as it is.
pub(crate) fn import(value: &JsValue) -> JsValue {
    shim_import(value)
}

impl Activation {
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

    /// The guest is to wait for its import. Returns the promise it waits on,
    /// and has [`check`] look at the activation once the guest's slice is
    /// over. Nothing else changes until then: some engines refuse to suspend
    /// the guest after this ran.
    fn wait(self: &Rc<Self>) -> JsValue {
        let promise = shim_wait(self.token);
        let wait = self.waits.get().wrapping_add(1);
        self.waits.set(wait);
        self.accepted.set(false);
        self.state.set(State::Waiting);
        shim_check_later(self.token, wait as i32);
        promise
    }

    /// Resolves the promise of the guest's current wait with `value` in a
    /// microtask, unless the call is dropped by then: the guest is unwound
    /// instead. Then [`check`] looks at the activation once the guest's slice
    /// after the wake is over. The guest has its store.
    fn wake(&self, value: &JsValue) {
        self.state.set(State::Woken);
        shim_wake_later(self.token, self.waits.get() as i32, value);
    }

    /// The guest waits: the store is given up, and what the guest waits for
    /// is driven from now on.
    fn accept(self: &Rc<Self>) {
        self.accepted.set(true);
        self.switch_out(State::Waiting);
    }

    /// The guest is running again, so a wait the engine never accepted was
    /// refused: what the guest was to wait for is dropped. The activation
    /// still holds the store.
    fn forget_refused_wait(&self) {
        if self.state.get() == State::Waiting && !self.accepted.get() {
            drop(self.import.take());
            self.state.set(State::Running);
        }
    }

    /// Drives what a guest waits for. Called by the future of the call, so
    /// with the caller's waker, on the caller's stack.
    fn drive(self: &Rc<Self>, cx: &mut Context<'_>) {
        if self.state.get() != State::Waiting || !self.accepted.get() {
            return;
        }
        if let Some(mut import) = self.import.take() {
            match import.as_mut().poll(cx) {
                Poll::Ready(Ok(results)) => *self.results.borrow_mut() = Some(results),
                Poll::Ready(Err(error)) => {
                    *self.error.borrow_mut() = Some(error);
                    return self.unwind();
                }
                Poll::Pending => {
                    *self.import.borrow_mut() = Some(import);
                    return;
                }
            }
        }
        let lock = match self.relock.take() {
            Some(mut relock) => match relock.as_mut().poll(cx) {
                Poll::Ready(lock) => Some(lock),
                Poll::Pending => {
                    *self.relock.borrow_mut() = Some(relock);
                    return;
                }
            },
            None => self.try_lock(),
        };
        match lock {
            Some(lock) => self.resume(lock),
            None => {
                // Somebody has the store: the guest waits for it, which the
                // call acquires for it.
                let store = self.store.borrow().as_ref().map(StoreAsync::store);
                let store = store.expect("a guest that can run has its store");
                *self.relock.borrow_mut() = Some(Box::pin(async move { store.write_lock().await }));
                self.drive(cx);
            }
        }
    }

    /// Wakes the guest with the results of its import, with its store.
    fn resume(self: &Rc<Self>, lock: StoreAsyncWriteLock) {
        let values = self.results.take().unwrap_or_default();
        let value = results(&values, &lock);
        self.switch_in(lock);
        self.accepted.set(false);
        self.wake(&value);
    }

    /// Wakes the guest with a trap: it unwinds without running guest code.
    fn unwind(self: &Rc<Self>) {
        drop(self.import.take());
        drop(self.results.take());
        drop(self.relock.take());
        self.switch_out(State::Unwinding);
        shim_unwind(self.token);
    }

    /// The end of the activation: its promise settled.
    fn retire(self: &Rc<Self>, result: Result<JsValue, JsValue>) {
        ACTIVATIONS.with(|activations| {
            let mut activations = activations.borrow_mut();
            activations.slots[(self.token - FIRST_TOKEN) as usize] = None;
            activations.free.push(self.token);
        });
        self.switch_out(State::Done);
        drop(self.import.take());
        drop(self.results.take());
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
            .get((token - FIRST_TOKEN) as usize)?
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
/// was not entered by `call_async`, or it was, with a synchronous call or
/// another JavaScript frame on the stack since. The same error as on other
/// backends.
fn cannot_suspend_error() -> RuntimeError {
    let code = TrapCode::YieldOutsideAsyncContext;
    let source = BackendTrap::Js(Trap::user(code.message().into()));
    RuntimeError::new_from_source(source, Vec::new(), Some(code))
}

/// [`cannot_suspend_error`], thrown as host functions throw their errors, so
/// that the caller gets this very error back.
fn cannot_suspend() -> JsValue {
    Trap::user(Box::new(cannot_suspend_error())).into()
}

/// The error of a guest the engine refused to suspend, if `error` is the
/// engine's `WebAssembly.SuspendError`: the engine refuses before the body
/// of an async host function runs, or after, on some engines.
pub(crate) fn refusal(error: &JsValue) -> Option<RuntimeError> {
    shim_is_suspend_error(error).then(cannot_suspend_error)
}

/// The activation of a guest of the store `id` that calls an async host
/// function: one entered by `call_async`. A guest whose call was dropped is
/// unwound instead.
///
/// The engine refuses to suspend a guest that was entered by a synchronous
/// call since, or that has any other JavaScript frame on the stack since it
/// was entered. Some engines refuse before the body of the import runs, and
/// others after: then the wait the body made is forgotten once the guest goes
/// on, and the store, which the activation holds until the guest waits, is
/// never given up for it.
fn suspendable(id: StoreId) -> Result<Rc<Activation>, JsValue> {
    let activation = running(id).ok_or_else(cannot_suspend)?;
    activation.forget_refused_wait();
    if activation.cancelled.get() {
        activation.switch_out(State::Unwinding);
        return Err(trap_error());
    }
    Ok(activation)
}

/// The body of an async host function of the store `id`. Returns the promise
/// the guest waits on; nothing of the host function runs here.
pub(crate) fn start(
    id: StoreId,
    import: impl FnOnce(StoreAsync) -> ImportFuture,
) -> Result<JsValue, JsValue> {
    let activation = suspendable(id)?;
    let store = activation.store.borrow().as_ref().map(StoreAsync::store);
    let store = store.expect("a guest that can run has its store");
    *activation.import.borrow_mut() = Some(import(store));
    Ok(activation.wait())
}

/// The microtask after the slice in which a guest started its `wait`th wait,
/// or was woken from it: by then the guest has left the stack.
/// Unless it was suspended again since, it still waits there, and the engine
/// accepted its wait, or it was given its results and did not wait again, so
/// it left the stack for good and gives up its store. The promise of a guest
/// that returned has settled by then.
fn check(token: i32, wait: i32) {
    let Some(activation) = activation(token) else {
        return;
    };
    if activation.waits.get() != wait as u32 {
        return;
    }
    match activation.state.get() {
        State::Waiting if !activation.accepted.get() => {
            activation.accept();
            activation.wake_caller();
        }
        State::Running => activation.switch_out(State::Exited),
        _ => {}
    }
}

/// The microtask in which a guest is woken from its `wait`th suspension. It
/// is not if its call was dropped since, which unwound it instead.
fn woken(token: i32, wait: i32) -> bool {
    let Some(activation) = activation(token) else {
        return false;
    };
    if activation.waits.get() != wait as u32 || activation.state.get() != State::Woken {
        return false;
    }
    activation.state.set(State::Running);
    true
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
    /// With `direct`, `enter` calls an async host function itself, which
    /// returns the promise it waits on as a guest would.
    pub(crate) fn enter(
        store: &StoreAsync,
        lock: StoreAsyncWriteLock,
        direct: bool,
        enter: impl FnOnce() -> Result<JsValue, JsValue>,
    ) -> Self {
        SHIM.with(|_| ());
        let activation = ACTIVATIONS.with(|activations| {
            let mut activations = activations.borrow_mut();
            let token = activations.free.pop().unwrap_or_else(|| {
                activations.slots.push(None);
                activations.slots.len() as i32 - 1 + FIRST_TOKEN
            });
            let activation = Rc::new(Activation {
                token,
                store_id: store.id,
                state: Cell::new(State::Running),
                cancelled: Cell::new(false),
                store: RefCell::new(Some(store.store())),
                lock: RefCell::new(None),
                import: RefCell::new(None),
                results: RefCell::new(None),
                relock: RefCell::new(None),
                error: RefCell::new(None),
                result: RefCell::new(None),
                caller: RefCell::new(None),
                entering: Cell::new(true),
                waits: Cell::new(0),
                accepted: Cell::new(false),
            });
            activations.slots[(token - FIRST_TOKEN) as usize] = Some(activation.clone());
            activation
        });
        activation.switch_in(lock);
        let waits = activation.waits.get();
        // A promising function returns a promise, or throws before it enters
        // the guest. The reaction is attached here, before anything else can
        // run: a rejection nobody handles ends the process on some hosts.
        match enter() {
            Ok(promise) => shim_settle(&promise, activation.token),
            Err(error) => activation.retire(Err(error)),
        }
        match activation.state.get() {
            // Back from a guest that was never suspended: it left the stack
            // for good.
            State::Running if activation.waits.get() == waits => {
                activation.switch_out(State::Exited)
            }
            // No engine is involved in calling a host function directly, so
            // it waits: what it waits for is driven from the first poll. A
            // guest that waits is looked at once its slice is over: an engine
            // may have refused to suspend it after the import ran.
            State::Waiting if direct => activation.accept(),
            _ => {}
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
        match activation.state.get() {
            State::Done => return,
            // The guest will not run again: it is woken with a trap, also if
            // it was about to be given its results. So the host future goes
            // now, and with it everything that keeps the store from being
            // taken back.
            State::Waiting | State::Woken => activation.unwind(),
            // A guest that is running keeps the store until it leaves the
            // stack or calls an async import, which traps.
            _ => {}
        }
        activation.cancelled.set(true);
        if activation.state.get() != State::Running {
            drop(activation.lock.take());
            drop(activation.store.take());
        }
    }
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

/// The function `call_async` calls to enter `function`: its promising form,
/// or, for an async host function, the function itself.
pub(crate) fn promising(function: &Function) -> Result<Function, JsValue> {
    if shim_is_host(function) {
        return Ok(function.clone());
    }
    promising_raw(function)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen::JsCast;
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
        let store = store.into_async();
        assert_eq!(live(), (0, 0));

        for round in 0..2 {
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

        // Ready at once: given its results in a microtask, with its store.
        let ready = Function::new_typed_async(&mut store.write_lock().await, async || 7);
        let mut call = Box::pin(ready.call_async(&store, vec![]));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(call.as_mut().poll(&mut cx).is_pending());
        assert_eq!(live(), (1, 1), "returned without waiting");
        assert_eq!(call.await.unwrap().to_vec(), [Value::I32(7)]);
        assert_eq!(live(), (0, 0));
    }

    /// `WebAssembly.promising` is called for the first `call_async` of a
    /// function, and never again.
    #[cfg(feature = "wat")]
    #[wasm_bindgen_test]
    async fn the_promising_wrapper_of_a_function_is_kept() {
        let mut store = Store::default();
        let module = crate::Module::new(
            &store,
            r#"(module (func (export "id") (param i32) (result i32) (local.get 0)))"#,
        )
        .unwrap();
        let instance = crate::Instance::new(&mut store, &module, &crate::Imports::new()).unwrap();
        let host = instance.exports.get_function("id").unwrap().clone();
        let again = instance.exports.get_function("id").unwrap().clone();
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
