use std::{cell::RefCell, collections::HashMap, future::Future, pin::Pin};

use crate::{AsStoreAsync, StoreAsync};
use js_sys::{Function, Promise, Reflect};
use wasm_bindgen::{JsCast, JsValue, prelude::wasm_bindgen};
use wasmer_types::StoreId;

struct ActiveStore {
    store: StoreAsync,
    users: usize,
}

thread_local! {
    static ACTIVE_STORES: RefCell<HashMap<StoreId, ActiveStore>> =
        RefCell::new(HashMap::new());
}

/// An owned host callback, retained and polled only on its JavaScript worker.
pub type JspiFuture = Pin<Box<dyn Future<Output = Result<JsValue, JsValue>>>>;

/// Converts a host callback into the Promise consumed by JSPI. Threaded runtimes
/// must replace the default executor when their providers wake from other workers.
pub type JspiExecutor = fn(JspiFuture) -> Promise;

/// A worker-local callback driver which captures the exact Store synchronously
/// at import entry, before the callback's first microtask.
pub type JspiStoreExecutor = fn(StoreId, JspiFuture) -> Promise;

/// One worker-local physical guest invocation. Its owner is retained by the
/// browser Promise, independently of any Rust waiter.
pub trait JspiCallOwner {
    /// Called exactly once at physical settlement. An owner may normalize an
    /// error in place, but must preserve original custody and the identity of
    /// ordinary live rejections. Retirement may use an owned cancellation marker.
    fn settled(
        self: Box<Self>,
        outcome: &mut Result<JsValue, JsValue>,
    ) -> Result<(), crate::RuntimeError>;
}

/// Creates a local physical-call owner before guest entry, or vetoes the call.
/// Returning None retains ordinary JSPI behavior without an embedding owner.
pub type JspiCallOwnerFactory =
    fn(StoreId) -> Result<Option<Box<dyn JspiCallOwner>>, crate::RuntimeError>;

/// Selects this Store's callback driver before its first async import. The driver
/// is invoked synchronously at callback entry, before the future is first polled.
pub fn set_jspi_executor(
    store: &mut impl crate::AsStoreMut,
    executor: JspiExecutor,
) -> Result<(), crate::RuntimeError> {
    let mut store = store.as_store_mut();
    let backend = store.inner.store.as_js_mut();
    if backend.async_imports {
        return Err(crate::RuntimeError::new(
            "cannot change the JSPI executor after creating async imports",
        ));
    }
    backend.jspi_executor = executor;
    backend.jspi_store_executor = None;
    backend.jspi_call_owner = None;
    Ok(())
}

/// Selects a Store-aware callback driver before creating async imports.
pub fn set_jspi_store_executor(
    store: &mut impl crate::AsStoreMut,
    executor: JspiStoreExecutor,
    owner: JspiCallOwnerFactory,
) -> Result<(), crate::RuntimeError> {
    let mut store = store.as_store_mut();
    let backend = store.inner.store.as_js_mut();
    if backend.async_imports {
        return Err(crate::RuntimeError::new(
            "cannot change the JSPI executor after creating async imports",
        ));
    }
    backend.jspi_store_executor = Some(executor);
    backend.jspi_call_owner = Some(owner);
    Ok(())
}

/// The selected lane of a JavaScript Store.
pub fn async_mode(store: &impl crate::AsStoreRef) -> AsyncMode {
    store.as_store_ref().inner.store.as_js().async_mode
}

pub(crate) struct ActiveStoreGuard {
    id: StoreId,
}

#[wasm_bindgen(inline_js = r#"
const jspiPromise = Promise;
const jspiThen = Function.call.bind(jspiPromise.prototype.then);
const jspiDefineProperty = Object.defineProperty;
const jspiConstructor = Object.create(null);
jspiDefineProperty(jspiConstructor, Symbol.species, { value: jspiPromise });
Object.freeze(jspiConstructor);
export function wasmerObserveJspi(promise, settled) {
    // A genuine JSPI Promise is fresh, extensible, and still private here.
    // Pin species before intrinsic then; no user getter or constructor runs.
    jspiDefineProperty(promise, "constructor", { value: jspiConstructor });
    return jspiThen(promise,
        value => settled(false, value),
        error => settled(true, error));
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = wasmerObserveJspi)]
    fn observe_promise(promise: &Promise, settled: &Function) -> Promise;
}

pub(crate) fn settle(
    owner: Option<Box<dyn JspiCallOwner>>,
    mut outcome: Result<JsValue, JsValue>,
) -> Result<JsValue, JsValue> {
    let observed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        owner.map_or(Ok(()), |owner| owner.settled(&mut outcome))
    }));
    match outcome {
        // Observer and Waker failure must never replace an already-owned guest
        // rejection. Dispose observer payloads after physical metadata retired.
        Err(error) => {
            if let Err(failure) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                drop(observed);
            })) {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(failure)));
            }
            Err(error)
        }
        Ok(value) => {
            match observed {
                Ok(result) => result.map_err(JsValue::from)?,
                Err(failure) => std::panic::resume_unwind(failure),
            }
            Ok(value)
        }
    }
}

pub(crate) fn watch(
    promise: Promise,
    active: ActiveStoreGuard,
    owner: Option<Box<dyn JspiCallOwner>>,
) -> Promise {
    let settled =
        wasm_bindgen::closure::Closure::once_into_js(move |rejected: bool, value: JsValue| {
            let outcome = if rejected { Err(value) } else { Ok(value) };
            let result = settle(owner, outcome);
            let retired = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(active)));
            match result {
                Err(error) => {
                    if let Err(failure) = retired {
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            drop(failure)
                        }));
                    }
                    Err(error)
                }
                Ok(value) => {
                    if let Err(failure) = retired {
                        std::panic::resume_unwind(failure);
                    }
                    Ok(value)
                }
            }
        });
    observe_promise(&promise, settled.unchecked_ref())
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

/// Reports whether the JavaScript host exposes both JSPI entry points.
pub fn is_supported() -> bool {
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
            .or_insert(ActiveStore { store, users: 1 });
    });
    ActiveStoreGuard { id }
}

pub(crate) fn active_store(id: StoreId) -> Option<StoreAsync> {
    ACTIVE_STORES.with(|stores| stores.borrow().get(&id).map(|active| active.store.store()))
}

impl Drop for ActiveStoreGuard {
    fn drop(&mut self) {
        let retired = ACTIVE_STORES.with(|stores| {
            let mut stores = stores.borrow_mut();
            let active = stores
                .get_mut(&self.id)
                .expect("active JavaScript store guard is unbalanced");
            active.users -= 1;
            if active.users == 0 {
                stores.remove(&self.id)
            } else {
                None
            }
        });
        // Last Store roots may run user destructors; the registry is unlocked.
        drop(retired);
    }
}

/// How async imports and guest calls execute in one JavaScript store.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AsyncMode {
    /// Synchronous imports poll Rust futures and park the dedicated task worker on Pending.
    #[default]
    Blocking,
    /// Optional JSPI stack switching, selected only after runtime feature probing.
    Jspi,
}

/// Selects a store's lane before creating its first async import. Blocking is the default.
/// JSPI requires both `WebAssembly.Suspending` and `WebAssembly.promising`.
pub fn set_async_mode(
    store: &mut impl crate::AsStoreMut,
    mode: AsyncMode,
) -> Result<(), crate::RuntimeError> {
    if mode == AsyncMode::Jspi && !is_supported() {
        return Err(crate::RuntimeError::new(
            "the JavaScript host does not support WebAssembly JSPI",
        ));
    }
    let mut store = store.as_store_mut();
    let backend = store.inner.store.as_js_mut();
    if backend.async_imports && backend.async_mode != mode {
        return Err(crate::RuntimeError::new(
            "cannot change the async lane after creating async imports",
        ));
    }
    backend.async_mode = mode;
    Ok(())
}
