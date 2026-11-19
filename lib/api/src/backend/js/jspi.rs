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
//!   polled with the caller's waker: first in the microtask that sees the
//!   guest wait, then by the future of the call whenever that is woken.
//! * The engine refuses to suspend a guest that was entered by a synchronous
//!   call, or that has another JavaScript frame on the stack since it was
//!   entered, and some engines run the body of the import before they
//!   refuse. So the body does nothing that cannot be undone: the store is
//!   given up, and the host future polled, only once a microtask after the
//!   guest's slice has seen it waiting ([`check`]). The engine's error for a
//!   refusal is taken for the one other backends give ([`refusal`]).
//! * The store changes hands only where this module sees the guest move: it
//!   is given up when the guest waits in an async host function, or unwinds,
//!   and when its promise settles. A guest suspended anywhere else, in a
//!   `WebAssembly.Suspending` function this module did not make, keeps its
//!   store, and so does one that returned and whose promise waits for a
//!   thenable it returned, on engines that take one for a promise.
//! * A guest woken from the future of the call is woken in a microtask,
//!   unless its call is dropped by then. A guest that must not run again,
//!   because its call was dropped or its host function failed, is woken with
//!   the error of a trap, which no exception handler of the guest catches:
//!   its stack unwinds without running guest code, and the engine frees it.
//! * A promise takes a thenable it is resolved with for a promise, so the
//!   results of an import cannot be one: the import fails instead.

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
        function::{enter_async, entered_synchronously},
        utils::convert::AsJs as _,
    },
};

/// The host's half of an async import: the host function and what it awaits.
/// Nothing of it has run until it is polled.
pub(crate) type ImportFuture = Pin<Box<dyn Future<Output = Result<Vec<Value>, RuntimeError>>>>;

type LockFuture = Pin<Box<dyn Future<Output = StoreAsyncWriteLock>>>;

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    /// The guest is on the stack, or has left it in a way this module does
    /// not see: it returned, or waits in a `WebAssembly.Suspending` function
    /// of somebody else's. The activation holds the store.
    Running,
    /// The guest waits on its promise, or is about to: once the wait is
    /// [`accept`](Activation::accept)ed, the store is free and what the guest
    /// waits for is driven by the call.
    Waiting,
    /// The guest is given its results in a microtask, with the store, which
    /// the activation holds, or was given them since: see
    /// [`Activation::wake`].
    Woken,
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
    /// What the guest waits for is being driven: a poll of the call from
    /// inside it, by a waker that polls at once, leaves it to the outer one.
    driving: Cell<bool>,
    /// The call was polled from inside its own driving: whoever woke it is
    /// woken again, once that is over.
    redrive: Cell<bool>,
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
    /// The activation of a direct call of an async host function, while
    /// [`Call::enter`] calls it: the body runs for that call, and no other.
    static DIRECT: RefCell<Option<Rc<Activation>>> = const { RefCell::new(None) };
    /// Hands the JavaScript half its Rust callbacks, once per thread.
    static SHIM: () = shim_init(
        &Closure::wrap(Box::new(done) as Box<dyn Fn(i32, i32, JsValue)>).into_js_value(),
        &Closure::wrap(Box::new(check) as Box<dyn Fn(i32, i32)>).into_js_value(),
        &Closure::wrap(Box::new(crate::js::function::mark_entered) as Box<dyn Fn(bool) -> bool>)
            .into_js_value(),
    );
}

// The JavaScript half: promises can only be made and settled there. It is
// static JavaScript, not made from a string: some hosts forbid that. An async
// host function and its `WebAssembly.Suspending` form find each other in
// maps of this module, which run no code of the values they are given, as
// reading a property of a `Proxy` would.
//
// `wait` makes the promise the guest of a token waits on, which `wake`
// resolves with the results of its import, and `unwind` rejects with the
// error of a trap. A trap's error is the one thing a guest's exception
// handlers do not catch, also when JavaScript throws it; the module that
// makes one is a constant, `(module (func (export "trap") unreachable))`.
//
// A guest suspended at a `WebAssembly.Suspending` import resumes in a
// microtask once its promise is resolved. A microtask queued two hops after
// the import's body runs once the guest's slice is over.
#[wasm_bindgen(inline_js = r#"
const resolvers = [], rejecters = [];
let done, check, mark, trapError, wakes = 0;
const pendingWakes = [], pendingValues = [];
export function jspiInit(d, c, m) {
  done = d; check = c; mark = m;
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
const suspendingOf = new WeakMap(), hostOf = new WeakMap(), storeOf = new WeakMap();
export function jspiHost(start, array, store) {
  const host = array ? (...args) => start(args) : start;
  const suspending = new WebAssembly.Suspending(host);
  suspendingOf.set(host, suspending);
  hostOf.set(suspending, host);
  storeOf.set(host, store);
  return host;
}
export function jspiOwn(value, store) {
  storeOf.set(value, store);
}
export function jspiOwner(value) {
  return storeOf.get(embedderOf.get(value) ?? value);
}
export function jspiHostOf(value) {
  return hostOf.get(value) ?? value;
}
export function jspiIsHost(value) {
  return suspendingOf.has(value);
}
export function jspiImport(value) {
  return suspendingOf.get(value) ?? value;
}
const apply = Reflect.apply, markedOf = new WeakMap(), embedderOf = new WeakMap();
const wasmFunctions = new WeakSet();
let probe;
function isWasmFunction(value) {
  if (wasmFunctions.has(value)) return true;
  probe ??= new WebAssembly.Table({ element: "anyfunc", initial: 1 });
  try { probe.set(0, value); } catch { return false; }
  probe.set(0, null);
  wasmFunctions.add(value);
  return true;
}
const toResult = (kind, value) =>
  kind === "n" ? +value : kind === "b" ? BigInt.asIntN(64, value) : value;
const iterator = Symbol.iterator;
// Results the engine iterates without running anything of the embedder's:
// no prototype to look anything up on, and an iterator of its own.
function tuple(values) {
  let index = 0;
  const next = () => index < values.length
    ? { __proto__: null, value: values[index++], done: false }
    : { __proto__: null, value: undefined, done: true };
  const results = { __proto__: null, next };
  return { __proto__: null, [iterator]: () => results };
}
export function jspiEmbedderFunction(value, kinds) {
  if (typeof value !== "function" || suspendingOf.has(value)) return value;
  // A function this made, given back: the embedder's own, for these results.
  value = embedderOf.get(value) ?? value;
  let byKinds = markedOf.get(value);
  let marked = byKinds?.get(kinds);
  if (marked === undefined) {
    if (isWasmFunction(value)) return value;
    marked = function (...args) {
      const previous = mark(true);
      try {
        const result = apply(value, this, args);
        if (kinds.length === 0) return result;
        if (kinds.length === 1) return toResult(kinds, result);
        // As the engine does with what a host function returns: iterated
        // once, its length checked, and then each value converted.
        const values = [];
        for (const item of result) values[values.length] = item;
        if (values.length !== kinds.length) {
          throw new TypeError(`expected ${kinds.length} results, got ${values.length}`);
        }
        for (let i = 0; i < kinds.length; i++) values[i] = toResult(kinds[i], values[i]);
        return tuple(values);
      } finally {
        mark(previous);
      }
    };
    if (byKinds === undefined) markedOf.set(value, byKinds = new Map());
    byKinds.set(kinds, marked);
    embedderOf.set(marked, value);
  }
  return marked;
}
export function jspiIsWasmFunction(value) {
  return isWasmFunction(value);
}
export function jspiPublic(value) {
  return suspendingOf.get(value) ?? embedderOf.get(value) ?? value;
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
export function jspiWakeLater(token, value) {
  const wake = ++wakes;
  pendingWakes[token] = wake;
  pendingValues[token] = value;
  queueMicrotask(() => {
    if (pendingWakes[token] !== wake) return;
    const value = pendingValues[token];
    pendingWakes[token] = 0;
    pendingValues[token] = undefined;
    jspiWake(token, value);
  });
}
export function jspiCancelWake(token) {
  const pending = pendingWakes[token] > 0;
  pendingWakes[token] = 0;
  pendingValues[token] = undefined;
  return pending;
}
export function jspiIsThenable(value) {
  return (typeof value === "object" || typeof value === "function") && value !== null &&
    typeof value.then === "function";
}
export function jspiCheckLater(token, wait) {
  queueMicrotask(() => queueMicrotask(() => check(token, wait)));
}
export function jspiIsSuspendError(value) {
  const SuspendError = WebAssembly.SuspendError;
  try {
    return typeof SuspendError === "function" && value instanceof SuspendError;
  } catch {
    // A revoked `Proxy` has no prototype to look at: it is not one.
    return false;
  }
}
export function jspiSettle(promise, token) {
  promise.then((value) => done(token, 0, value), (error) => done(token, 1, error));
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = jspiInit)]
    fn shim_init(done: &JsValue, check: &JsValue, mark: &JsValue);
    #[wasm_bindgen(js_name = jspiTrapError)]
    fn trap_error() -> JsValue;
    #[wasm_bindgen(js_name = jspiHost)]
    fn shim_host(start: &JsValue, array: bool, store: f64) -> Function;
    #[wasm_bindgen(js_name = jspiOwn)]
    fn shim_own(value: &JsValue, store: f64);
    #[wasm_bindgen(js_name = jspiOwner)]
    fn shim_owner(value: &JsValue) -> Option<f64>;
    #[wasm_bindgen(js_name = jspiIsWasmFunction)]
    fn shim_is_wasm_function(value: &JsValue) -> bool;
    #[wasm_bindgen(js_name = jspiIsHost)]
    fn shim_is_host(value: &JsValue) -> bool;
    #[wasm_bindgen(js_name = jspiImport)]
    fn shim_import(value: &JsValue) -> JsValue;
    #[wasm_bindgen(js_name = jspiHostOf)]
    fn shim_host_of(value: &JsValue) -> JsValue;
    #[wasm_bindgen(js_name = jspiEmbedderFunction)]
    fn shim_embedder_function(value: &JsValue, kinds: &str) -> JsValue;
    #[wasm_bindgen(js_name = jspiPublic)]
    fn shim_public(value: &JsValue) -> JsValue;
    #[wasm_bindgen(js_name = jspiWait)]
    fn shim_wait(token: i32) -> JsValue;
    #[wasm_bindgen(js_name = jspiWake)]
    fn shim_wake(token: i32, value: &JsValue);
    #[wasm_bindgen(js_name = jspiWakeLater)]
    fn shim_wake_later(token: i32, value: &JsValue);
    #[wasm_bindgen(js_name = jspiCancelWake)]
    fn shim_cancel_wake(token: i32) -> bool;
    #[wasm_bindgen(js_name = jspiIsThenable, catch)]
    fn shim_is_thenable(value: &JsValue) -> Result<bool, JsValue>;
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

/// Whether the results of an import can reach the guest as they are: a
/// promise takes a thenable it is resolved with for a promise, waits for it,
/// and gives the guest what it settles with. So an import whose result is one
/// fails instead, as one whose result cannot be read: reading `then` runs
/// JavaScript, which may throw. It runs before the store is taken back for
/// the guest, and may drop the call. The promise reads `then` again when it
/// is resolved; an object that answers differently, as a `Proxy` can, is
/// taken for what it answers then. If that is a thenable, the guest waits for
/// it with its store, also once its call is dropped: the engine resumes the
/// guest when the thenable settles, and nothing can stop that, so the store
/// is not given up before.
///
/// The engine gives a guest no function as a `funcref` but a WebAssembly
/// one, and throws an exception at the guest that the guest can catch
/// otherwise. That fails the import too.
fn deliverable(values: &[Value]) -> Result<(), RuntimeError> {
    for value in values {
        if let Value::FuncRef(Some(function)) = value
            && !shim_is_wasm_function(&function.as_js().handle.function)
        {
            return Err(RuntimeError::new(
                "an async host function returned a function that is not a WebAssembly \
                 function, which a guest cannot be given as a `funcref`",
            ));
        }
    }
    let value: JsValue = match values {
        [Value::ExternRef(Some(reference))] => match &reference.0 {
            crate::BackendExternRef::Js(reference) => reference.as_js_value(),
            #[allow(unreachable_patterns)]
            _ => return Ok(()),
        },
        [Value::FuncRef(Some(function))] => function.as_js().handle.function.clone().into(),
        // Nothing else is ever a thenable: several results reach the guest
        // as an array.
        _ => return Ok(()),
    };
    match shim_is_thenable(&value) {
        Ok(false) => Ok(()),
        Ok(true) => Err(RuntimeError::new(
            "an async host function returned a thenable, which a guest cannot be given \
             through JSPI as it is",
        )),
        Err(error) => Err(RuntimeError::new(format!(
            "reading `then` of what an async host function returned threw: {}",
            RuntimeError::from(error).message()
        ))),
    }
}

/// The function of an async host function of the store `store`: `start`,
/// which returns the promise the guest waits on, or, if it takes an `array`,
/// a function that calls it with its arguments in one. A guest imports it as
/// a `WebAssembly.Suspending` function, which it keeps (see [`import`]).
///
/// # Panics
/// Panics if the signature has a type JavaScript has no value of, or takes a
/// `funcref`, which this backend cannot make a function of.
pub(crate) fn host_function(
    ty: &FunctionType,
    store: StoreId,
    start: &JsValue,
    array: bool,
) -> Function {
    for ty in ty.params().iter().chain(ty.results()) {
        if matches!(ty, Type::V128 | Type::ExceptionRef) {
            panic!(
                "an async host function cannot take or return a `{ty}` on the JavaScript \
                 backend: JavaScript has no value of that type"
            );
        }
    }
    if ty.params().contains(&Type::FuncRef) {
        panic!(
            "an async host function cannot take a `funcref` on the JavaScript backend: \
             it cannot make a function of what a guest passes"
        );
    }
    SHIM.with(|_| ());
    shim_host(start, array, store.as_raw().get() as f64)
}

/// Records that `function`, a host function this crate made for the store
/// `id`, belongs to it: what it runs reaches that store through a pointer.
pub(crate) fn own(function: &JsValue, id: StoreId) {
    shim_own(function, id.as_raw().get() as f64);
}

/// Whether `function` can be used with the store `id`: a host function this
/// crate made, also given back as an embedder's function, with the store it
/// was made for only, which, being the caller's, is alive; any other
/// function with any store.
pub(crate) fn belongs_to(function: &JsValue, id: StoreId) -> bool {
    shim_owner(function).is_none_or(|store| store == id.as_raw().get() as f64)
}

/// The error of a host function called with another store than its own.
pub(crate) fn foreign_store_error() -> RuntimeError {
    RuntimeError::new("a host function was called with a store it does not belong to")
}

/// What a guest imports for `value`: the `WebAssembly.Suspending` form of an
/// async host function, and anything else as it is. This is what a function
/// is given to JavaScript as: the function itself returns a promise to a
/// guest that calls it, and fails outside of one.
pub(crate) fn import(value: &JsValue) -> JsValue {
    shim_import(value)
}

/// The async host function whose `WebAssembly.Suspending` form `value` is, as
/// [`import`] gives it; anything else as it is.
pub(crate) fn host_of(value: &JsValue) -> JsValue {
    shim_host_of(value)
}

/// A JavaScript function an embedder gives, `value`, as a guest is to call
/// it with the type `ty`: a function that marks what it runs as entered
/// synchronously (see [`suspendable`]), calls `value` with its receiver and
/// arguments, and makes of what it returns the primitives the engine takes
/// for the results of `ty`. All the JavaScript the call runs, the engine's
/// conversion of the results included (`valueOf`, iteration), runs marked.
/// The same one for the same function and results. WebAssembly functions,
/// and the functions of this crate, are given as they are.
pub(crate) fn embedder_function(value: &JsValue, ty: &FunctionType) -> JsValue {
    SHIM.with(|_| ());
    // The conversion each result takes from the engine: to a number, to a
    // BigInt, or none, which runs no JavaScript.
    let kinds: String = ty
        .results()
        .iter()
        .map(|ty| match ty {
            Type::I32 | Type::F32 | Type::F64 => 'n',
            Type::I64 => 'b',
            _ => 'r',
        })
        .collect();
    shim_embedder_function(value, &kinds)
}

/// Whether `function` is a WebAssembly function, or an async host function:
/// one `call_async` enters a guest through. Anything else is JavaScript that
/// runs to the end.
pub(crate) fn enters_a_guest(function: &JsValue) -> bool {
    shim_is_host(function) || shim_is_wasm_function(function)
}

/// What a function is given to JavaScript as: the `WebAssembly.Suspending`
/// form of an async host function, and the embedder's own function for one
/// [`embedder_function`] made.
pub(crate) fn public(value: &JsValue) -> JsValue {
    shim_public(value)
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

    /// Resolves the promise of the guest's current wait with `value`, with
    /// the guest's store. In a microtask of its own the guest goes on in the
    /// next one: nothing else runs before it but what was queued already.
    /// From the future of the call, whose caller may drop it as soon as the
    /// poll returns, it is resolved a microtask later, unless the call is
    /// dropped by then: the guest is unwound instead.
    fn wake(&self, value: &JsValue, now: bool) {
        if now {
            self.state.set(State::Running);
            shim_wake(self.token, value);
        } else {
            self.state.set(State::Woken);
            shim_wake_later(self.token, value);
        }
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

    /// Drives what a guest waits for, with the caller's waker: from the
    /// microtask that sees the guest wait (`now`), and from the future of the
    /// call.
    fn drive(self: &Rc<Self>, cx: &mut Context<'_>, now: bool) {
        if self.driving.get() {
            // Polled from inside the poll of the host future, or of the lock:
            // the import is taken out for it, and is not finished. The outer
            // driving goes on, and passes the wake on (see below).
            self.redrive.set(true);
            return;
        }
        self.driving.set(true);
        self.redrive.set(false);
        self.drive_once(cx, now);
        self.driving.set(false);
        if self.redrive.take() && self.state.get() == State::Waiting {
            // A wake that came during the poll was taken by a poll of the call
            // that did nothing: whoever polls the call now, which that poll
            // may have changed, is woken again.
            self.wake_caller();
        }
    }

    fn drive_once(self: &Rc<Self>, cx: &mut Context<'_>, now: bool) {
        if self.state.get() != State::Waiting || !self.accepted.get() {
            return;
        }
        if let Some(mut import) = self.import.take() {
            let outcome = import.as_mut().poll(cx);
            if self.state.get() != State::Waiting {
                // The poll dropped the call.
                return;
            }
            match outcome {
                Poll::Ready(Ok(results)) => {
                    let deliverable = deliverable(&results);
                    if self.state.get() != State::Waiting {
                        // Reading the results dropped the call.
                        return;
                    }
                    if let Err(error) = deliverable {
                        *self.error.borrow_mut() = Some(error);
                        return self.unwind();
                    }
                    *self.results.borrow_mut() = Some(results);
                }
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
            Some(lock) => self.resume(lock, now),
            None => {
                // Somebody has the store: the guest waits for it, which the
                // call acquires for it.
                let store = self.store.borrow().as_ref().map(StoreAsync::store);
                let store = store.expect("a guest that can run has its store");
                *self.relock.borrow_mut() = Some(Box::pin(async move { store.write_lock().await }));
                self.drive_once(cx, now);
            }
        }
    }

    /// Wakes the guest with the results of its import, with its store.
    fn resume(self: &Rc<Self>, lock: StoreAsyncWriteLock, now: bool) {
        let values = self.results.take().unwrap_or_default();
        let value = results(&values, &lock);
        self.switch_in(lock);
        self.accepted.set(false);
        self.wake(&value, now);
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
/// others after. A guest entered by a synchronous call of this crate, or from
/// inside a host function it made or a JavaScript function it was given,
/// refuses itself here, before it makes a wait. Only JavaScript this crate
/// never saw, an import of a module instantiated without it, can enter a
/// guest that makes a wait the engine then refuses. That wait is forgotten
/// once the guest goes on, and the store, which the activation holds until
/// the wait is confirmed, is never given up for it, unless the guest is
/// suspended next somewhere else before its slice is over: nothing tells
/// that from a wait. Entering guests from such JavaScript while a guest of
/// the store runs is not supported (see `Function::call_async`).
fn suspendable(id: StoreId) -> Result<Rc<Activation>, JsValue> {
    if entered_synchronously() {
        return Err(cannot_suspend());
    }
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
    let activation = match DIRECT.with(|direct| direct.borrow_mut().take()) {
        Some(activation) if activation.store_id == id => activation,
        Some(_) => return Err(Trap::user(Box::new(foreign_store_error())).into()),
        None => suspendable(id)?,
    };
    let store = activation.store.borrow().as_ref().map(StoreAsync::store);
    let store = store.expect("a guest that can run has its store");
    *activation.import.borrow_mut() = Some(import(store));
    Ok(activation.wait())
}

/// The microtask after the slice in which a guest started its `wait`th wait:
/// by then the guest has left the stack. If it still waits there, the engine
/// suspended it: the store is given up, and what the guest waits for is
/// driven at once. The promise of a guest that was refused and returned has
/// settled by then.
fn check(token: i32, wait: i32) {
    let Some(activation) = activation(token) else {
        return;
    };
    if activation.waits.get() != wait as u32
        || activation.state.get() != State::Waiting
        || activation.accepted.get()
    {
        return;
    }
    if activation.cancelled.get() {
        // The call was dropped before it was known that the engine had
        // suspended the guest: it is unwound now that it is.
        activation.accepted.set(true);
        return activation.unwind();
    }
    activation.accept();
    let caller = activation.caller.borrow().clone();
    match caller {
        Some(caller) => activation.drive(&mut Context::from_waker(&caller), true),
        None => activation.wake_caller(),
    }
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
                driving: Cell::new(false),
                redrive: Cell::new(false),
            });
            activations.slots[(token - FIRST_TOKEN) as usize] = Some(activation.clone());
            activation
        });
        activation.switch_in(lock);
        // A promising function returns a promise, or throws before it enters
        // the guest. The reaction is attached here, before anything else can
        // run: a rejection nobody handles ends the process on some hosts.
        let entered = {
            let _entered = enter_async();
            if direct {
                DIRECT.with(|slot| *slot.borrow_mut() = Some(activation.clone()));
            }
            let entered = enter();
            DIRECT.with(|slot| slot.borrow_mut().take());
            entered
        };
        match entered {
            Ok(promise) => shim_settle(&promise, activation.token),
            Err(error) => activation.retire(Err(error)),
        }
        // No engine is involved in calling a host function directly, so it
        // waits: what it waits for is driven from the first poll. A guest
        // that waits is looked at once its slice is over: an engine may have
        // refused to suspend it after the import ran. A guest that returned
        // keeps its store until its promise settles.
        if direct && activation.state.get() == State::Waiting {
            activation.accept();
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
        activation.drive(cx, false);
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
        // Whether the engine suspended a guest that waits is known only once
        // its slice is over: until then it may be running on, refused, and
        // keeps its store. `check` unwinds it if it was suspended.
        let unconfirmed = activation.state.get() == State::Waiting && !activation.accepted.get();
        match activation.state.get() {
            State::Done => return,
            // The guest will not run again: it is woken with a trap, also if
            // it was about to be given its results. So the host future goes
            // now, and with it everything that keeps the store from being
            // taken back.
            State::Waiting if !unconfirmed => activation.unwind(),
            State::Woken if shim_cancel_wake(activation.token) => activation.unwind(),
            // A guest that was given its results is running: it keeps the
            // store until it leaves the stack or calls an async import, which
            // traps.
            State::Woken => activation.state.set(State::Running),
            _ => {}
        }
        activation.cancelled.set(true);
        if activation.state.get() != State::Running && !unconfirmed {
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
