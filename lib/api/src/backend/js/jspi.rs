use std::{cell::RefCell, collections::HashMap, rc::Weak};

use crate::{AsStoreAsync, ForcedStoreInstallGuard, StoreAsync};
use js_sys::{Function, Reflect};
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

    /// Alive while the `Function::call_async` future that started this call is
    /// alive. Once it is gone the guest must never be resumed — not even to
    /// throw, since a rejection runs the guest's own exception handlers, which
    /// is guest code running under a call that no longer exists.
    pub(crate) alive: Weak<()>,
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
