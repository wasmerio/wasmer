use std::{
    cell::RefCell,
    collections::HashMap,
    future::Future,
    pin::Pin,
    rc::{Rc, Weak},
    task::{Context, Poll, Waker},
};

use crate::{AsStoreAsync, StoreAsync};
use js_sys::{Function, Promise, Reflect};
use wasm_bindgen::{
    JsCast, JsValue,
    prelude::{Closure, wasm_bindgen},
};
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

#[derive(Default)]
struct PromiseState {
    result: Option<Result<JsValue, JsValue>>,
    waker: Option<Waker>,
}

/// Unlike JsFuture, an abandoned JSPI promise must not retain a Rust waker.
/// Cancelled guest contexts deliberately leave their JS stacks suspended; a
/// strong callback -> state -> waker cycle keeps those contexts alive forever.
pub(crate) struct PromiseFuture(Rc<RefCell<PromiseState>>);

impl PromiseFuture {
    pub(crate) fn new(promise: Promise) -> Result<Self, JsValue> {
        let state = Rc::new(RefCell::new(PromiseState::default()));
        let resolve = Self::callback(Rc::downgrade(&state), true);
        let reject = Self::callback(Rc::downgrade(&state), false);
        // JavaScript owns the callbacks and can collect them with an abandoned
        // promise. They hold only Weak references, so dropping this future
        // immediately releases its waker even if neither callback ever runs.
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
        if let Some(result) = state.result.take() {
            Poll::Ready(result)
        } else {
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use wasm_bindgen_test::wasm_bindgen_test;

    struct WakeFlag;
    impl std::task::Wake for WakeFlag {
        fn wake(self: Arc<Self>) {}
    }

    #[wasm_bindgen_test]
    fn dropping_a_suspended_promise_releases_its_waker() {
        let promise = Promise::new(&mut |_, _| {});
        let mut future = PromiseFuture::new(promise).unwrap();
        let flag = Arc::new(WakeFlag);
        let waker = Waker::from(flag.clone());
        assert!(
            Pin::new(&mut future)
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        assert_eq!(Arc::strong_count(&flag), 3);
        drop(future);
        assert_eq!(
            Arc::strong_count(&flag),
            2,
            "an abandoned JSPI stack retained its Rust task"
        );
    }

    #[wasm_bindgen_test]
    async fn promise_results_and_rejections_are_preserved() {
        assert_eq!(
            PromiseFuture::new(Promise::resolve(&JsValue::from(42)))
                .unwrap()
                .await
                .unwrap(),
            42
        );
        assert_eq!(
            PromiseFuture::new(Promise::reject(&JsValue::from_str("failure")))
                .unwrap()
                .await
                .unwrap_err(),
            "failure"
        );
    }

    #[wasm_bindgen_test]
    async fn late_callbacks_after_cancellation_are_harmless() {
        for reject in [false, true] {
            let mut settle = None;
            let promise = Promise::new(&mut |resolve, rejected| {
                settle = Some(if reject { rejected } else { resolve })
            });
            drop(PromiseFuture::new(promise).unwrap());
            settle
                .unwrap()
                .call1(&JsValue::UNDEFINED, &7.into())
                .unwrap();
            PromiseFuture::new(Promise::resolve(&JsValue::UNDEFINED))
                .unwrap()
                .await
                .unwrap();
        }
    }
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
                .expect("active JSPI store guard is unbalanced");
            active.users -= 1;
            if active.users == 0 {
                stores.remove(&self.id);
            }
        });
    }
}
