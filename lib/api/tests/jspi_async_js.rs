#![cfg(all(feature = "experimental-async", feature = "js", target_arch = "wasm32"))]

use js_sys::Promise;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::wasm_bindgen_test;
use wasmer::{Function, Instance, Module, Store, TypedFunction, imports};

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

#[wasm_bindgen_test]
async fn cancelled_guest_call_does_not_resume_host_code_after_store_release() {
    use futures::{
        channel::oneshot,
        future::{Either, select},
    };
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    use wasmer::FunctionType;

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
    release.send(()).unwrap();
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
