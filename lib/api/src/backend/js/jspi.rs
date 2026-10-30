use std::{cell::RefCell, collections::HashMap};

use crate::{AsStoreAsync, StoreAsync};
use js_sys::{Function, Reflect};
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
        ACTIVE_STORES.with(|stores| {
            let mut stores = stores.borrow_mut();
            let active = stores
                .get_mut(&self.id)
                .expect("active JavaScript store guard is unbalanced");
            active.users -= 1;
            if active.users == 0 {
                stores.remove(&self.id);
            }
        });
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
