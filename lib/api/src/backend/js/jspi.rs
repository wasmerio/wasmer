use std::{
    cell::RefCell,
    collections::HashMap,
    future::Future,
    pin::Pin,
    rc::{Rc, Weak},
    task::{Context, Poll},
};

use crate::{AsStoreAsync, ForcedStoreInstallGuard, StoreAsync};
use js_sys::{Function, Promise, Reflect};
use wasm_bindgen::{JsCast, JsValue, prelude::wasm_bindgen};
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
}

/// What an async import's host future reports: how to settle the suspended
/// guest's promise, or `None` to leave it unsettled and the guest inert.
type ImportOutcome = Option<Result<JsValue, JsValue>>;

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
    ACTIVE_STORES.with(|stores| {
        let mut stores = stores.borrow_mut();
        match stores.get_mut(&id) {
            Some(active) => active.installed = Some(parked),
            None => drop(parked),
        }
    });
}

/// Takes back the parked context, if there is one. Dropping the result
/// uninstalls the context and releases the store's write lock.
pub(crate) fn take_context(id: StoreId) -> Option<ParkedCall> {
    ACTIVE_STORES.with(|stores| stores.borrow_mut().get_mut(&id)?.installed.take())
}

pub(crate) fn active_store(id: StoreId) -> Option<StoreAsync> {
    ACTIVE_STORES.with(|stores| stores.borrow().get(&id).map(|active| active.store.store()))
}

impl Drop for ActiveStoreGuard {
    fn drop(&mut self) {
        ACTIVE_STORES.with(|stores| {
            let mut stores = stores.borrow_mut();
            let active = stores
                .get_mut(&self.id)
                .expect("active JSPI store guard is unbalanced");
            active.users -= 1;
            if active.users == 0 {
                stores.remove(&self.id);
            }
        });
    }
}
