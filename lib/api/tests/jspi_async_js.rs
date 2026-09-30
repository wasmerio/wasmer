#![cfg(all(feature = "experimental-async", feature = "js", target_arch = "wasm32"))]

use std::{
    cell::RefCell,
    sync::{Arc, Mutex},
};

use futures::FutureExt;
use js_sys::Promise;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::wasm_bindgen_test;
use wasmer::{
    AsStoreAsync, AsyncFunctionEnvMut, Function, FunctionEnv, FunctionEnvMut, FunctionType,
    Instance, Module, Store, TypedFunction, imports,
};

#[wasm_bindgen_test]
async fn typed_async_host_and_guest_calls_use_jspi() {
    let mut store = Store::default();
    let module = Module::new(
        &store,
        r#"
        (module
          (import "host" "increment" (func $increment (param i32) (result i32)))
          (func (export "compute") (param i32) (result i32)
            local.get 0
            call $increment))
        "#,
    )
    .unwrap();
    let increment = Function::new_typed_async(&mut store, async move |value: i32| {
        JsFuture::from(Promise::resolve(&JsValue::UNDEFINED))
            .await
            .unwrap();
        value + 1
    });
    let imports = imports! {
        "host" => {
            "increment" => increment,
        }
    };
    let instance = Instance::new(&mut store, &module, &imports).unwrap();
    let compute: TypedFunction<i32, i32> = instance
        .exports
        .get_typed_function(&store, "compute")
        .unwrap();

    let result = compute.call_async(&store.into_async(), 41).await.unwrap();
    assert_eq!(result, 42);
}

/// Suspends in an async import, then calls a synchronous one, so the sync
/// import runs during the *resumed* part of the guest's execution rather than
/// the first synchronous span.
const SUSPEND_THEN_OBSERVE: &str = r#"
(module
  (import "host" "suspend" (func $suspend))
  (import "host" "observe" (func $observe))
  (func (export "run")
    call $suspend
    call $observe))
"#;

#[derive(Default)]
struct Observed {
    store_was_reachable: Option<bool>,
    observe_calls: u32,
}

/// A promise that settles in a later macrotask, so awaiting it drains every
/// microtask the runtime has queued — including a guest resumption.
fn next_macrotask() -> Promise {
    Promise::new(&mut |resolve, _reject| {
        let set_timeout: js_sys::Function =
            js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("setTimeout"))
                .unwrap()
                .unchecked_into();
        set_timeout
            .call2(&JsValue::UNDEFINED, &resolve, &JsValue::from_f64(0.0))
            .unwrap();
    })
}

fn suspend_then_observe(store: &mut Store) -> (Instance, FunctionEnv<Observed>) {
    let module = Module::new(&store, SUSPEND_THEN_OBSERVE).unwrap();

    let suspend = Function::new_typed_async(store, async move || {
        JsFuture::from(Promise::resolve(&JsValue::UNDEFINED))
            .await
            .unwrap();
    });

    let env = FunctionEnv::new(store, Observed::default());
    let observe = Function::new_with_env(
        store,
        &env,
        FunctionType::new(vec![], vec![]),
        |mut env: FunctionEnvMut<'_, Observed>, _args| {
            // Asked from inside an import, this is `Some` exactly when the
            // thread still has this store's context installed.
            let reachable = env.as_store_async().is_some();
            let data = env.data_mut();
            data.store_was_reachable = Some(reachable);
            data.observe_calls += 1;
            Ok(vec![])
        },
    );

    let imports = imports! {
        "host" => {
            "suspend" => suspend,
            "observe" => observe,
        }
    };
    let instance = Instance::new(store, &module, &imports).unwrap();
    (instance, env)
}

/// `Function::call_async` installs the store context around `Reflect::apply`
/// only, and that returns at the guest's first suspension. Everything the guest
/// runs after resuming therefore executes with no context installed and no
/// write lock held, so nothing excludes another task from the store while guest
/// code is running.
///
/// A synchronous import is the smallest way to observe it: reached before any
/// suspension it can see its store, reached after one it cannot.
#[wasm_bindgen_test]
async fn a_sync_import_after_a_suspension_still_reaches_its_store() {
    let mut store = Store::default();
    let (instance, env) = suspend_then_observe(&mut store);
    let run: TypedFunction<(), ()> = instance.exports.get_typed_function(&store, "run").unwrap();

    let store_async = store.into_async();
    run.call_async(&store_async).await.unwrap();

    let lock = store_async.read_lock().await;
    let observed = env.as_ref(&lock);
    assert_eq!(
        observed.observe_calls, 1,
        "the guest should have reached the synchronous import"
    );
    assert_eq!(
        observed.store_was_reachable,
        Some(true),
        "a synchronous import running after a suspension must still find its \
         store context installed"
    );
}

/// Dropping the future returned by `Function::call_async` has to stop the call.
/// The JSPI stack and its promise chain live in the JS runtime, so today the
/// guest resumes regardless — and with the future gone, so is the
/// `ActiveStoreGuard` that was the last thing pinning the store between host
/// tasks, which is what turns a cancelled call into a use-after-free.
#[wasm_bindgen_test]
async fn a_dropped_call_async_future_stops_the_guest() {
    let mut store = Store::default();
    let (instance, env) = suspend_then_observe(&mut store);
    let run: TypedFunction<(), ()> = instance.exports.get_typed_function(&store, "run").unwrap();

    let store_async = store.into_async();

    // One poll takes the guest as far as its first suspension; `now_or_never`
    // then drops the future, which is the cancellation.
    let settled = run.call_async(&store_async).now_or_never();
    assert!(
        settled.is_none(),
        "the guest should have suspended in the async import"
    );

    // Give the runtime every chance to resume the cancelled call.
    JsFuture::from(next_macrotask()).await.unwrap();
    JsFuture::from(next_macrotask()).await.unwrap();

    let lock = store_async.read_lock().await;
    assert_eq!(
        env.as_ref(&lock).observe_calls,
        0,
        "a dropped call_async future must not go on running guest code"
    );
}

/// A cancelled call must not strand the store.
///
/// The async imports a guest suspends on hold store clones. While they were
/// handed to `wasm_bindgen_futures`, nothing owned them: an abandoned suspension
/// kept its clones for the life of the page, so `StoreAsync::into_store` could
/// never reclaim the store — which is how WASIX tears a context down, and it
/// panicked instead. Owning them in the call fixes that, and this is the
/// difference being pinned.
#[wasm_bindgen_test]
async fn a_dropped_call_async_future_releases_the_store() {
    let mut store = Store::default();
    let (instance, _env) = suspend_then_observe(&mut store);
    let run: TypedFunction<(), ()> = instance.exports.get_typed_function(&store, "run").unwrap();

    let store_async = store.into_async();
    assert!(
        run.call_async(&store_async).now_or_never().is_none(),
        "the guest should have suspended in the async import"
    );
    JsFuture::from(next_macrotask()).await.unwrap();

    drop(run);
    drop(instance);
    assert!(
        store_async.into_store().is_ok(),
        "a dropped call_async future must leave no clone of the store behind"
    );
}

/// The chain a suspendable dynamic call needs on this backend: an *async* import
/// that re-enters the guest with [`TypedFunction::call_async`], and a suspension
/// inside that nested call.
///
/// ```text
/// call_async -> async import -> call_async -> async import -> await point
/// ```
///
/// WASIX's dynamic-call and lazy-binding trampolines use a *synchronous* import
/// in the middle, which JSPI cannot carry: V8 refuses to suspend past the host
/// frame ("trying to suspend JS frames") because the nearest
/// `WebAssembly.promising` boundary sits below it. Making the middle import
/// async puts a boundary above that frame. The suspension is then legal to V8,
/// so what is left is whether this backend's own bookkeeping nests — the parked
/// context, the store lock and the borrow count all have to survive one
/// `call_async` running inside another.
#[wasm_bindgen_test]
async fn a_nested_call_async_can_suspend() {
    const WAT: &str = r#"
    (module
      (import "host" "reenter" (func $reenter (result i32)))
      (import "host" "suspend" (func $suspend (result i32)))
      (func (export "entry") (result i32)
        call $reenter)
      (func (export "inner") (result i32)
        call $suspend))
    "#;

    let mut store = Store::default();
    let module = Module::new(&store, WAT).unwrap();

    struct Env {
        inner: RefCell<Option<TypedFunction<(), i32>>>,
        order: Arc<Mutex<Vec<&'static str>>>,
    }

    let order: Arc<Mutex<Vec<&'static str>>> = Arc::default();
    let env = FunctionEnv::new(
        &mut store,
        Env {
            inner: RefCell::new(None),
            order: Arc::clone(&order),
        },
    );

    let reenter = Function::new_typed_with_env_async(
        &mut store,
        &env,
        async move |env: AsyncFunctionEnvMut<Env>| {
            // The read handle holds the store lock, so it has to be released
            // before the nested call, which takes that lock for itself.
            let (order, inner) = {
                let handle = env.read().await;
                let data = handle.data();
                (
                    Arc::clone(&data.order),
                    data.inner
                        .borrow()
                        .clone()
                        .expect("inner function to be set"),
                )
            };
            order.lock().unwrap().push("reenter:enter");
            let store = env.as_store_async();
            let result = inner
                .call_async(&store)
                .await
                .expect("nested async call to succeed");
            order.lock().unwrap().push("reenter:leave");
            result
        },
    );

    let suspend = Function::new_typed_with_env_async(
        &mut store,
        &env,
        async move |env: AsyncFunctionEnvMut<Env>| {
            let order = Arc::clone(&env.read().await.data().order);
            order.lock().unwrap().push("suspend:before");
            JsFuture::from(next_macrotask()).await.unwrap();
            order.lock().unwrap().push("suspend:after");
            42
        },
    );

    let instance = Instance::new(
        &mut store,
        &module,
        &imports! {
            "host" => {
                "reenter" => reenter,
                "suspend" => suspend,
            }
        },
    )
    .unwrap();

    let inner: TypedFunction<(), i32> = instance
        .exports
        .get_typed_function(&store, "inner")
        .unwrap();
    env.as_mut(&mut store).inner.borrow_mut().replace(inner);

    let entry: TypedFunction<(), i32> = instance
        .exports
        .get_typed_function(&store, "entry")
        .unwrap();

    let result = entry.call_async(&store.into_async()).await.unwrap();

    assert_eq!(result, 42);
    assert_eq!(
        *order.lock().unwrap(),
        vec![
            "reenter:enter",
            "suspend:before",
            "suspend:after",
            "reenter:leave"
        ],
        "the nested call must finish before the outer import returns"
    );
}
