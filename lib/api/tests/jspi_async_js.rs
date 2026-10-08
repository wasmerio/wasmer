#![cfg(all(feature = "experimental-async", feature = "js", target_arch = "wasm32"))]

use std::{
    cell::RefCell,
    sync::{Arc, Mutex},
};

use futures::FutureExt;
use js_sys::Promise;
use std::future::Future;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::wasm_bindgen_test;
use wasmer::{
    AsStoreAsync, AsStoreMut, AsyncFunctionEnvMut, Function, FunctionEnv, FunctionEnvMut,
    FunctionType, Instance, Module, Store, TypedFunction, imports,
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

/// The part of a guest that runs after a suspension must still have its store
/// context installed, and the store's write lock held.
///
/// `Reflect::apply` returns at the guest's first suspension, so a context
/// installed around it would cover only the first span, and the guest would
/// resume in a JavaScript job with no context and no lock. So the context is
/// parked instead, and reinstalled by whichever import resumes the guest. A
/// synchronous import is the smallest way to observe it: reached after a
/// suspension, it must still find its store.
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

/// Dropping the future returned by `Function::call_async` while its guest is
/// suspended has to stop the call: the guest must never resume.
///
/// The JSPI stack and its promise chain live in the JavaScript runtime, which
/// would resume the guest once its import completed, with no call left to give
/// it a store. The call owns its imports, so dropping it drops them, and the
/// guest's promise is never settled. (A drop just after an import completed
/// cannot stop the resumption; see
/// `a_guest_resumed_after_its_call_was_dropped_finishes_with_its_store`.)
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

/// A cancelled call whose store is then reclaimed must never resume host code.
///
/// Ported from #7011, which fixed this by checking an async store's weak
/// lifetime before each poll of a suspended host future and discarding it if the
/// store had gone. Here the call *owns* its imports (see `jspi::CallState`), so
/// dropping the call drops them before the store can be reclaimed at all, and
/// the sequence this exercises cannot resume anything. The test is kept as the
/// statement of that guarantee rather than of the mechanism.
#[wasm_bindgen_test]
async fn cancelled_guest_call_does_not_resume_host_code_after_store_release() {
    use futures::{
        channel::oneshot,
        future::{Either, select},
    };
    use std::{cell::Cell, rc::Rc};

    struct Dropped(Rc<Cell<bool>>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }

    let mut store = Store::default();
    let module = Module::new(
        &store,
        r#"(module
        (import "host" "wait" (func $wait))
        (func (export "run") call $wait))"#,
    )
    .unwrap();
    let (started, started_rx) = oneshot::channel();
    let (release, release_rx) = oneshot::channel();
    let channels = Rc::new(RefCell::new(Some((started, release_rx))));
    let resumed = Rc::new(Cell::new(false));
    let dropped = Rc::new(Cell::new(false));
    let host = Function::new_async(&mut store, FunctionType::new([], []), {
        let resumed = resumed.clone();
        let dropped = dropped.clone();
        move |_| {
            let (started, release_rx) = channels.borrow_mut().take().unwrap();
            let resumed = resumed.clone();
            let dropped = Dropped(dropped.clone());
            async move {
                let _dropped = dropped;
                started.send(()).unwrap();
                release_rx.await.unwrap();
                resumed.set(true);
                Ok(vec![])
            }
        }
    });
    let instance =
        Instance::new(&mut store, &module, &imports! {"host" => {"wait" => host}}).unwrap();
    let run = instance.exports.get_function("run").unwrap();
    let store = store.into_async();
    let call = Box::pin(run.call_async(&store, vec![]));
    let pending_call = match select(call, started_rx).await {
        Either::Right((Ok(()), call)) => call,
        _ => panic!("guest call should be suspended in the host function"),
    };
    drop(pending_call);
    // Exercise the same into_store/drop sequence as WASIX teardown.
    drop(
        store
            .into_store()
            .expect("suspended imports must not retain the store"),
    );
    // #7011 asserts this send succeeds, because there the host future outlives
    // the cancellation and is only discarded at its next poll. Here the call owns
    // it, so it is already gone and its receiver with it — a closed channel is
    // the stronger outcome, not a failure.
    assert!(
        release.send(()).is_err(),
        "the cancelled host future should already have been dropped"
    );
    for _ in 0..10 {
        JsFuture::from(Promise::resolve(&JsValue::UNDEFINED))
            .await
            .unwrap();
    }
    assert!(
        !resumed.get(),
        "host code resumed with a released environment"
    );
    assert!(dropped.get(), "cancelled host future was not released");
}

/// A guest that suspends once on a fresh microtask, then either traps or
/// returns 7.
const SUSPEND_THEN_TRAP_OR_FINISH: &str = r#"
(module
  (import "host" "suspend" (func $suspend))
  (func (export "trap")
    call $suspend
    unreachable)
  (func (export "finish") (result i32)
    call $suspend
    i32.const 7))
"#;

/// Races `future` against a generous number of macrotasks, so a call that would
/// wait forever fails the test instead of hanging it.
async fn within_macrotasks<F: Future>(future: F) -> Option<F::Output> {
    let deadline = async {
        for _ in 0..50 {
            JsFuture::from(next_macrotask()).await.unwrap();
        }
    };
    futures::pin_mut!(future, deadline);
    match futures::future::select(future, deadline).await {
        futures::future::Either::Left((output, _)) => Some(output),
        futures::future::Either::Right(((), _)) => None,
    }
}

/// A guest that traps must not leave its call's context parked.
///
/// The context holds the store's write lock. Left behind, every other call on
/// the store waits on that lock forever — here, the one that suspended while the
/// trapping guest was running.
#[wasm_bindgen_test]
async fn a_trapping_guest_does_not_wedge_the_other_calls_on_its_store() {
    let mut store = Store::default();
    let module = Module::new(&store, SUSPEND_THEN_TRAP_OR_FINISH).unwrap();
    let suspend = Function::new_typed_async(&mut store, async move || {
        JsFuture::from(next_macrotask()).await.unwrap();
    });
    let imports = imports! { "host" => { "suspend" => suspend } };
    let instance = Instance::new(&mut store, &module, &imports).unwrap();
    let trap: TypedFunction<(), ()> = instance.exports.get_typed_function(&store, "trap").unwrap();
    let finish: TypedFunction<(), i32> = instance
        .exports
        .get_typed_function(&store, "finish")
        .unwrap();

    let store_async = store.into_async();
    let both = futures::future::join(
        trap.call_async(&store_async),
        finish.call_async(&store_async),
    );
    let (trapped, finished) = within_macrotasks(both)
        .await
        .expect("a call on the store never finished after another call's guest trapped");

    assert!(trapped.is_err(), "the guest should have trapped");
    assert_eq!(finished.unwrap(), 7);
}

/// Polls `call` by hand until `ready_to_resume` reports that the import the
/// guest suspended on has finished — so its promise is resolved and the guest
/// will resume in a JavaScript job — then drops it before that job runs. That is
/// the one cancellation the call cannot take back.
async fn cancel_after_resolve_before_resume<F: Future>(
    call: F,
    ready_to_resume: impl Fn() -> bool,
) {
    let mut call = Box::pin(call);
    let mut cx = std::task::Context::from_waker(futures::task::noop_waker_ref());
    for _ in 0..20 {
        assert!(
            call.as_mut().poll(&mut cx).is_pending(),
            "the guest finished before it could be cancelled"
        );
        if ready_to_resume() {
            drop(call);
            return;
        }
        // Let the import's own promise settle before polling again.
        JsFuture::from(Promise::resolve(&JsValue::UNDEFINED))
            .await
            .unwrap();
    }
    panic!("the import never finished");
}

/// A guest whose call was dropped after its import finished, and which then
/// runs to completion.
///
/// The resumption cannot be stopped, so the guest's context stays installed for
/// it: the synchronous import it reaches must find its store rather than panic
/// for want of a context — which used to leave the thread unusable. Once the
/// guest finishes, nothing may still hold the store.
#[wasm_bindgen_test]
async fn a_guest_resumed_after_its_call_was_dropped_finishes_with_its_store() {
    let mut store = Store::default();
    let module = Module::new(&store, SUSPEND_THEN_OBSERVE).unwrap();
    let resolved = Arc::new(Mutex::new(false));
    let suspend = Function::new_typed_async(&mut store, {
        let resolved = resolved.clone();
        move || {
            let resolved = resolved.clone();
            async move {
                JsFuture::from(Promise::resolve(&JsValue::UNDEFINED))
                    .await
                    .unwrap();
                *resolved.lock().unwrap() = true;
            }
        }
    });
    let env = FunctionEnv::new(&mut store, Observed::default());
    let observe = Function::new_with_env(
        &mut store,
        &env,
        FunctionType::new(vec![], vec![]),
        |mut env: FunctionEnvMut<'_, Observed>, _args| {
            let reachable = env.as_store_async().is_some();
            let data = env.data_mut();
            data.store_was_reachable = Some(reachable);
            data.observe_calls += 1;
            Ok(vec![])
        },
    );
    let imports = imports! { "host" => { "suspend" => suspend, "observe" => observe } };
    let instance = Instance::new(&mut store, &module, &imports).unwrap();
    let run: TypedFunction<(), ()> = instance.exports.get_typed_function(&store, "run").unwrap();

    let store_async = store.into_async();
    cancel_after_resolve_before_resume(run.call_async(&store_async), || *resolved.lock().unwrap())
        .await;
    JsFuture::from(next_macrotask()).await.unwrap();

    {
        let lock = store_async.read_lock().await;
        let observed = env.as_ref(&lock);
        assert_eq!(
            observed.observe_calls, 1,
            "the resumed guest should have run on"
        );
        assert_eq!(
            observed.store_was_reachable,
            Some(true),
            "the resumed guest must still find its store"
        );
    }
    drop((run, instance));
    assert!(
        store_async.into_store().is_ok(),
        "the finished guest must leave no clone of the store behind"
    );
}

/// As above, but the resumed guest suspends again instead of finishing: it runs
/// up to that suspension and no further, and gives the store back there.
const SUSPEND_OBSERVE_SUSPEND_OBSERVE: &str = r#"
(module
  (import "host" "suspend" (func $suspend))
  (import "host" "observe" (func $observe))
  (func (export "run")
    call $suspend
    call $observe
    call $suspend
    call $observe))
"#;

#[wasm_bindgen_test]
async fn a_guest_resumed_after_its_call_was_dropped_stops_at_its_next_suspension() {
    let mut store = Store::default();
    let module = Module::new(&store, SUSPEND_OBSERVE_SUSPEND_OBSERVE).unwrap();
    let resolved = Arc::new(Mutex::new(false));
    let suspend = Function::new_typed_async(&mut store, {
        let resolved = resolved.clone();
        move || {
            let resolved = resolved.clone();
            async move {
                JsFuture::from(Promise::resolve(&JsValue::UNDEFINED))
                    .await
                    .unwrap();
                *resolved.lock().unwrap() = true;
            }
        }
    });
    let env = FunctionEnv::new(&mut store, Observed::default());
    let observe = Function::new_with_env(
        &mut store,
        &env,
        FunctionType::new(vec![], vec![]),
        |mut env: FunctionEnvMut<'_, Observed>, _args| {
            env.data_mut().observe_calls += 1;
            Ok(vec![])
        },
    );
    let imports = imports! { "host" => { "suspend" => suspend, "observe" => observe } };
    let instance = Instance::new(&mut store, &module, &imports).unwrap();
    let run: TypedFunction<(), ()> = instance.exports.get_typed_function(&store, "run").unwrap();

    let store_async = store.into_async();
    cancel_after_resolve_before_resume(run.call_async(&store_async), || *resolved.lock().unwrap())
        .await;
    JsFuture::from(next_macrotask()).await.unwrap();
    JsFuture::from(next_macrotask()).await.unwrap();

    {
        let lock = store_async.read_lock().await;
        assert_eq!(
            env.as_ref(&lock).observe_calls,
            1,
            "the resumed guest must stop at its next suspension"
        );
    }
    drop((run, instance));
    assert!(
        store_async.into_store().is_ok(),
        "the guest left inert must leave no clone of the store behind"
    );
}

thread_local! {
    /// The module and imports `instantiate_from_a_sync_import` builds, kept here
    /// because JavaScript-backed imports cannot live in a `FunctionEnv`.
    static TO_INSTANTIATE: RefCell<Option<(Module, wasmer::Imports)>> = const { RefCell::new(None) };
}

/// A start function is guest code entered synchronously: one that reaches an
/// async import cannot suspend, and must be refused rather than allowed to
/// release the store context from under the instantiating frame — which used to
/// trip the "still borrowed" assertion and leave the thread unusable.
#[wasm_bindgen_test]
async fn a_start_function_reaching_an_async_import_is_refused() {
    const OUTER: &str = r#"
    (module
      (import "host" "instantiate" (func $instantiate (result i32)))
      (func (export "run") (result i32)
        call $instantiate))
    "#;
    const STARTS_BY_SUSPENDING: &str = r#"
    (module
      (import "host" "suspend" (func $suspend))
      (func $start call $suspend)
      (start $start))
    "#;

    let mut store = Store::default();
    let inner = Module::new(&store, STARTS_BY_SUSPENDING).unwrap();
    let suspend = Function::new_typed_async(&mut store, async move || {
        JsFuture::from(next_macrotask()).await.unwrap();
    });
    TO_INSTANTIATE.with_borrow_mut(|slot| {
        *slot = Some((inner, imports! { "host" => { "suspend" => suspend } }));
    });

    let env = FunctionEnv::new(&mut store, ());
    let instantiate =
        Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<'_, ()>| -> i32 {
            let (module, imports) = TO_INSTANTIATE.with_borrow(|slot| slot.clone().unwrap());
            match Instance::new(&mut env.as_store_mut(), &module, &imports) {
                Ok(_) => 1,
                Err(_) => 0,
            }
        });
    let outer = Module::new(&store, OUTER).unwrap();
    let instance = Instance::new(
        &mut store,
        &outer,
        &imports! { "host" => { "instantiate" => instantiate } },
    )
    .unwrap();
    let run: TypedFunction<(), i32> = instance.exports.get_typed_function(&store, "run").unwrap();

    let store_async = store.into_async();
    let instantiated = within_macrotasks(run.call_async(&store_async))
        .await
        .expect("the outer call never finished")
        .expect("the outer call should finish even though the instantiation failed");
    assert_eq!(
        instantiated, 0,
        "a start function that reaches an async import cannot run to completion"
    );
}
