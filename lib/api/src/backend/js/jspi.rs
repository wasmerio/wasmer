use std::{
    cell::RefCell,
    collections::HashMap,
    future::Future,
    pin::Pin,
    rc::{Rc, Weak},
    task::{Context, Poll, Waker},
};

use crate::{AsStoreAsync, ForcedStoreInstallGuard, StoreAsync};
use js_sys::{Function, Promise, Reflect};
use wasm_bindgen::{
    JsCast, JsValue,
    prelude::{Closure, wasm_bindgen},
};
use wasmer_types::StoreId;

struct ActiveStore {
    store: StoreAsync,
    users: usize,

    /// The store context installed for the guest running under this call.
    ///
    /// On `sys` an install guard lives on the Rust frame that drives the guest,
    /// because there always is one: `AsyncCallFuture::poll` brackets every
    /// resume. Under JSPI the guest resumes inside a JS job with no Rust frame
    /// on the stack, so the guard is parked here instead — taken out by the
    /// import that is about to suspend, and put back by that import's
    /// completion, just before the guest resumes.
    ///
    /// Holding the guard holds the store's write lock, so parking it is also
    /// what keeps other tasks off the store while guest code runs, and taking
    /// it out at a suspension is what lets them in.
    installed: Option<ParkedCall>,
}

/// The state of the call whose guest is currently running on this store.
///
/// Only one call can be parked at a time, because holding `guard` holds the
/// store's write lock: a second call cannot install its own context until the
/// first has suspended and released.
pub(crate) struct ParkedCall {
    /// Uninstalls the context and releases the store when dropped.
    pub(crate) guard: ForcedStoreInstallGuard,

    /// The call that is running this guest, if it is still alive. Once it is
    /// gone the guest must never be resumed — not even to throw, since a
    /// rejection runs the guest's own exception handlers, which is guest code
    /// running under a call that no longer exists.
    ///
    /// Weak on purpose: the strong reference lives in the
    /// `Function::call_async` future, and the futures this holds capture a
    /// `Weak` back, so a strong one here would be a cycle that never frees.
    pub(crate) call: Weak<CallState>,

    /// The dead call's hold on the store, when this context was orphaned: see
    /// [`CallExit`]. Keeps the store active, and so reachable by the guest's
    /// trampolines, for exactly as long as the orphaned context is parked.
    pub(crate) membership: Option<ActiveStoreGuard>,
}

/// What an async import's host future reports: how to settle the suspended
/// guest's promise, or `None` to leave it unsettled and the guest inert.
type ImportOutcome = Option<Result<JsValue, JsValue>>;

thread_local! {
    /// How many synchronous calls into the guest are on this thread's stack
    /// since the innermost entry through `WebAssembly.promising`.
    static SYNC_GUEST_ENTRIES: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Marks a guest entered through a synchronous call, for as long as it lives.
///
/// Such a guest cannot suspend: the call entered it through `Reflect.apply`, a
/// JavaScript frame that a suspension would have to cross, and the engine
/// refuses. Async imports check [`beneath_sync_entry`] to refuse first, before
/// they release anything the synchronous call still relies on.
pub(crate) struct SyncGuestEntry(());

impl SyncGuestEntry {
    pub(crate) fn enter() -> Self {
        SYNC_GUEST_ENTRIES.with(|entries| entries.set(entries.get() + 1));
        Self(())
    }
}

impl Drop for SyncGuestEntry {
    fn drop(&mut self) {
        SYNC_GUEST_ENTRIES.with(|entries| entries.set(entries.get() - 1));
    }
}

/// Whether the guest now running was entered through a synchronous call, and
/// so cannot suspend.
pub(crate) fn beneath_sync_entry() -> bool {
    SYNC_GUEST_ENTRIES.with(|entries| entries.get() > 0)
}

/// Runs `enter`, which enters a guest through `WebAssembly.promising`, as a
/// fresh stack: a guest entered that way may suspend whatever synchronous
/// calls are further down, because the suspension stops at its own entry.
///
/// Only the initial entry needs this. A suspended guest resumes inside a
/// JavaScript job, with no synchronous call of ours on the stack.
pub(crate) fn enter_promising<R>(enter: impl FnOnce() -> R) -> R {
    let outer = SYNC_GUEST_ENTRIES.with(|entries| entries.replace(0));
    let result = enter();
    SYNC_GUEST_ENTRIES.with(|entries| entries.set(outer));
    result
}

#[derive(Default)]
struct PromiseState {
    result: Option<Result<JsValue, JsValue>>,
    waker: Option<Waker>,
}

/// A future over a JS promise which lets go of its waker when dropped.
///
/// `JsFuture` cannot be used for a guest's promise. Its settle callbacks hold a
/// *strong* reference to the state holding the waker, and JavaScript owns those
/// callbacks for as long as the promise is alive — so a promise that never
/// settles keeps them, and the waker, forever. Cancelling a call deliberately
/// leaves its guest's JS stack suspended on exactly such a promise, which would
/// make every cancellation leak.
///
/// Holding only `Weak` references inverts that: dropping this future releases the
/// state immediately, and a callback that fires afterwards finds nothing to wake
/// and does nothing.
pub(crate) struct PromiseFuture(Rc<RefCell<PromiseState>>);

impl PromiseFuture {
    pub(crate) fn new(promise: Promise) -> Result<Self, JsValue> {
        let state = Rc::new(RefCell::new(PromiseState::default()));
        let resolve = Self::callback(Rc::downgrade(&state), true);
        let reject = Self::callback(Rc::downgrade(&state), false);
        // `then` rather than `Promise::then`, which would need the closures to
        // outlive this call; these are handed to JavaScript to own and collect.
        let then: Function = Reflect::get(&promise, &"then".into())?.dyn_into()?;
        let _ = then.call2(&promise, &resolve, &reject)?;
        Ok(Self(state))
    }

    fn callback(state: Weak<RefCell<PromiseState>>, resolved: bool) -> JsValue {
        Closure::wrap(Box::new(move |value: JsValue| {
            let Some(state) = state.upgrade() else {
                return;
            };
            let waker = {
                let mut state = state.borrow_mut();
                state.result = Some(if resolved { Ok(value) } else { Err(value) });
                state.waker.take()
            };
            if let Some(waker) = waker {
                waker.wake();
            }
        }) as Box<dyn FnMut(JsValue)>)
        .into_js_value()
    }
}

impl Future for PromiseFuture {
    type Output = Result<JsValue, JsValue>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.0.borrow_mut();
        match state.result.take() {
            Some(result) => Poll::Ready(result),
            None => {
                state.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// An async import whose guest is suspended, waiting for the host to finish.
struct PendingImport {
    future: Pin<Box<dyn Future<Output = ImportOutcome>>>,
    /// The suspended guest's `(resolve, reject)`. Dropping these without
    /// calling either leaves the guest suspended for good, which is how an
    /// abandoned call is left inert.
    settle: Option<(Function, Function)>,
}

/// Everything a single `Function::call_async` owns while its guest runs.
///
/// The async imports that guest reaches are *owned here* rather than spawned:
/// `sys` polls a host future from the call's own `AsyncCallFuture::poll`, so
/// whoever drives the call drives the imports, and dropping the call drops them.
/// Handing them to `wasm_bindgen_futures` instead would detach them onto an
/// executor the caller never chose and that nothing can cancel — which also
/// strands the store clones those futures hold, so a WASIX context teardown
/// could never reclaim the store.
#[derive(Default)]
pub(crate) struct CallState {
    pending: RefCell<Vec<PendingImport>>,

    /// The waker of whoever drives this call.
    ///
    /// A guest suspends on an import from a JS job, not from inside a poll, so
    /// the push has to reach the call's future itself: nothing else will. `sys`
    /// has no equivalent because there a host future is first polled in the same
    /// `AsyncCallFuture::poll` that resumed the guest.
    waker: RefCell<Option<std::task::Waker>>,
}

impl CallState {
    /// Suspends the guest on a fresh promise and takes ownership of `future`,
    /// which settles that promise once [`Self::drive`] sees it finish.
    pub(crate) fn suspend_guest_on<F>(&self, future: F) -> Promise
    where
        F: Future<Output = ImportOutcome> + 'static,
    {
        let mut settle = None;
        let promise = Promise::new(&mut |resolve, reject| settle = Some((resolve, reject)));
        self.pending.borrow_mut().push(PendingImport {
            future: Box::pin(future),
            settle,
        });
        // Nothing has polled this future yet, so it has registered no waker of
        // its own; without this the call would never look at it again.
        if let Some(waker) = self.waker.borrow().as_ref() {
            waker.wake_by_ref();
        }
        promise
    }

    /// Polls every async import this call is waiting on, resuming the guest for
    /// each one that finished. Called from the call's own future, so `cx` is the
    /// caller's waker and no other executor is involved.
    pub(crate) fn drive(&self, cx: &mut Context<'_>) {
        self.waker.borrow_mut().replace(cx.waker().clone());

        // The borrow is released around each poll: polling an import can re-enter
        // the guest synchronously, and that guest can suspend on an import of its
        // own.
        let mut unfinished = Vec::new();
        while let Some(mut import) = self.pending.borrow_mut().pop() {
            match import.future.as_mut().poll(cx) {
                Poll::Pending => unfinished.push(import),
                Poll::Ready(outcome) => {
                    if let (Some((resolve, reject)), Some(result)) = (import.settle.take(), outcome)
                    {
                        let _ = match result {
                            Ok(value) => resolve.call1(&JsValue::UNDEFINED, &value),
                            Err(error) => reject.call1(&JsValue::UNDEFINED, &error),
                        };
                    }
                }
            }
        }
        self.pending.borrow_mut().append(&mut unfinished);
    }
}

thread_local! {
    static ACTIVE_STORES: RefCell<HashMap<StoreId, ActiveStore>> =
        RefCell::new(HashMap::new());
}

pub(crate) struct ActiveStoreGuard {
    id: StoreId,
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = WebAssembly, js_name = promising, catch)]
    fn promising_raw(function: &Function) -> Result<Function, JsValue>;

    #[wasm_bindgen(js_namespace = WebAssembly, js_name = Suspending)]
    type Suspending;

    #[wasm_bindgen(constructor, js_namespace = WebAssembly, catch)]
    fn new(function: &Function) -> Result<Suspending, JsValue>;
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

pub(crate) fn promising(function: &Function) -> Result<Function, JsValue> {
    promising_raw(function)
}

pub(crate) fn suspending(function: &Function) -> Result<Function, JsValue> {
    Suspending::new(function).map(JsCast::unchecked_into)
}

pub(crate) fn install_store(store: StoreAsync) -> ActiveStoreGuard {
    let id = store.store_id();
    ACTIVE_STORES.with(|stores| {
        let mut stores = stores.borrow_mut();
        stores
            .entry(id)
            .and_modify(|active| active.users += 1)
            .or_insert(ActiveStore {
                store,
                users: 1,
                installed: None,
            });
    });
    ActiveStoreGuard { id }
}

/// Parks the context installed for `id`'s guest until the next suspension.
///
/// Dropped immediately if the call has already finished, which uninstalls the
/// context and releases the store, as it would have done anyway.
pub(crate) fn park_context(id: StoreId, parked: ParkedCall) {
    // Dropped outside the borrow: an orphan's membership reaches back into
    // `ACTIVE_STORES` when it goes.
    let unparked = ACTIVE_STORES.with(|stores| {
        let mut stores = stores.borrow_mut();
        match stores.get_mut(&id) {
            Some(active) => {
                active.installed = Some(parked);
                None
            }
            None => Some(parked),
        }
    });
    drop(unparked);
}

/// Takes back the parked context, if there is one. Dropping the result
/// uninstalls the context and releases the store's write lock.
pub(crate) fn take_context(id: StoreId) -> Option<ParkedCall> {
    ACTIVE_STORES.with(|stores| stores.borrow_mut().get_mut(&id)?.installed.take())
}

/// Takes back the parked context only if it belongs to `call`.
///
/// A call that is finishing must not take whatever happens to be parked: once
/// its own guest has suspended for the last time, the store may already be
/// running another call's guest, whose context is the one parked.
fn take_context_of(id: StoreId, call: &Weak<CallState>) -> Option<ParkedCall> {
    ACTIVE_STORES.with(|stores| {
        let mut stores = stores.borrow_mut();
        let active = stores.get_mut(&id)?;
        if !active.installed.as_ref()?.call.ptr_eq(call) {
            return None;
        }
        active.installed.take()
    })
}

/// Cleans up after a `Function::call_async`, however it ends.
///
/// A call parks its guest's context before entering it, and whichever of its
/// imports completes last re-parks it before resuming the guest. How that
/// context is released depends on how the call ends:
///
/// * The guest finished, trapped, or never started: the context, if it is
///   still this call's, is released. Without this an error leaves it parked,
///   holding the store's write lock, and every other call on the store waits
///   forever.
/// * The call was dropped while its guest is suspended: nothing of this
///   call's is parked, and the import the guest suspended on leaves it inert.
/// * The call was dropped just after one of its imports completed — its
///   promise is resolved, so the guest *will* resume, in a JavaScript job that
///   nothing can cancel. Its context is parked for that resumption. Releasing it
///   would resume the guest with no context at all, so instead the context is
///   *orphaned*: it stays parked, together with the call's hold on the store,
///   until the guest next suspends — where the import sees the call is gone,
///   releases both and leaves the guest inert — or finishes, where a handler on
///   its promise releases them. In between the guest runs host code for a call
///   that no longer exists, but only synchronously, and only up to that point.
pub(crate) struct CallExit {
    store_id: StoreId,
    membership: Option<ActiveStoreGuard>,
    call: Weak<CallState>,
    guest: Option<Promise>,
    finished: bool,
}

impl CallExit {
    pub(crate) fn new(store: StoreAsync) -> Self {
        Self {
            store_id: store.store_id(),
            membership: Some(install_store(store)),
            call: Weak::new(),
            guest: None,
            finished: false,
        }
    }

    /// The call about to enter its guest.
    pub(crate) fn entering(&mut self, call: &Rc<CallState>) {
        self.call = Rc::downgrade(call);
    }

    /// The guest is running, and settles `guest` when it stops.
    pub(crate) fn running(&mut self, guest: &Promise) {
        self.guest = Some(guest.clone());
    }

    /// The guest has finished; dropping this releases its context.
    pub(crate) fn finished(mut self) {
        self.finished = true;
    }
}

impl Drop for CallExit {
    fn drop(&mut self) {
        let parked = take_context_of(self.store_id, &self.call);
        let guest = match (self.finished, self.guest.take()) {
            (false, Some(guest)) => guest,
            // Finished, or never got as far as running: release.
            _ => return drop(parked),
        };
        // Dropped while the guest is still running. With nothing of ours
        // parked the guest is suspended, and stays so.
        let Some(mut parked) = parked else {
            return;
        };

        // Orphaned: see the type's documentation.
        parked.membership = self.membership.take();
        let store_id = self.store_id;
        let call = self.call.clone();
        let release = Closure::wrap(Box::new(move |_: JsValue| {
            drop(take_context_of(store_id, &call));
        }) as Box<dyn FnMut(JsValue)>)
        .into_js_value();
        // Handed to JavaScript to own, as in `PromiseFuture::new`, so a guest
        // that never finishes does not leak it.
        if let Ok(then) = Reflect::get(&guest, &"then".into()).and_then(|then| {
            then.dyn_into::<Function>()
        }) {
            let _ = then.call2(&guest, &release, &release);
        }
        park_context(store_id, parked);
    }
}

pub(crate) fn active_store(id: StoreId) -> Option<StoreAsync> {
    ACTIVE_STORES.with(|stores| stores.borrow().get(&id).map(|active| active.store.store()))
}

impl Drop for ActiveStoreGuard {
    fn drop(&mut self) {
        // Dropped outside the borrow, since a parked context can hold a
        // membership of its own.
        let removed = ACTIVE_STORES.with(|stores| {
            let mut stores = stores.borrow_mut();
            let active = stores
                .get_mut(&self.id)
                .expect("active JSPI store guard is unbalanced");
            active.users -= 1;
            if active.users == 0 {
                stores.remove(&self.id)
            } else {
                None
            }
        });
        drop(removed);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use futures::task::{ArcWake, waker};
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::*;

    /// Counts the live wakers of its kind.
    struct CountedWaker(Arc<AtomicUsize>);

    impl Drop for CountedWaker {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    impl ArcWake for CountedWaker {
        fn wake_by_ref(_: &Arc<Self>) {}
    }

    /// The reason this type exists instead of `JsFuture`: a cancelled call leaves
    /// its guest suspended on a promise that never settles, and JavaScript holds
    /// the callbacks attached to it for as long as the promise lives. Holding the
    /// waker strongly from there would leak it — and a `Waker` keeps its whole
    /// task alive — on every cancellation.
    #[wasm_bindgen_test]
    fn dropping_a_promise_future_releases_its_waker() {
        let never_settles = Promise::new(&mut |_, _| {});

        let alive = Arc::new(AtomicUsize::new(1));
        let counted = Arc::new(CountedWaker(Arc::clone(&alive)));
        let waker = waker(Arc::clone(&counted));
        drop(counted);

        {
            let mut future = std::pin::pin!(PromiseFuture::new(never_settles).unwrap());
            assert!(
                future
                    .as_mut()
                    .poll(&mut Context::from_waker(&waker))
                    .is_pending(),
                "a promise that never settles cannot be ready"
            );
            assert_eq!(
                alive.load(Ordering::SeqCst),
                1,
                "the future holds the waker while it is alive"
            );
        }
        drop(waker);

        assert_eq!(
            alive.load(Ordering::SeqCst),
            0,
            "the waker outlived the future that registered it"
        );
    }
}
