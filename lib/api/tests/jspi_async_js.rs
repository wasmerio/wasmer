#![cfg(all(feature = "experimental-async", feature = "js", target_arch = "wasm32"))]

use js_sys::Promise;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::wasm_bindgen_test;
use wasmer::{Function, Instance, Module, Store, TypedFunction, imports};

mod common;
use common::Exit;

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

/// Calls a guest whose import is an async host function that fails with
/// `Exit(code)`, after awaiting a JS promise or at once. (The guest suspends
/// either way: the import's promise settles only after the call.)
async fn exit_through_async_import(code: i32, await_js: bool) -> wasmer::RuntimeError {
    let mut store = Store::default();
    let module = Module::new(
        &store,
        r#"
        (module
          (import "host" "exit" (func $exit (param i32)))
          (func (export "run") (param i32)
            local.get 0
            call $exit))
        "#,
    )
    .unwrap();
    let exit = Function::new_typed_async(&mut store, move |code: i32| async move {
        if await_js {
            JsFuture::from(Promise::resolve(&JsValue::UNDEFINED))
                .await
                .unwrap();
        }
        Err::<(), _>(Exit(code))
    });
    let imports = imports! {
        "host" => {
            "exit" => exit,
        }
    };
    let instance = Instance::new(&mut store, &module, &imports).unwrap();
    let run: TypedFunction<i32, ()> = instance.exports.get_typed_function(&store, "run").unwrap();
    run.call_async(&store.into_async(), code).await.unwrap_err()
}

#[wasm_bindgen_test]
async fn an_async_import_fails_with_its_own_error_after_awaiting_js() {
    let error = exit_through_async_import(7, true).await;
    assert_eq!(
        error.downcast_ref::<Exit>().map(|exit| exit.0),
        Some(7),
        "{error}"
    );
}

#[wasm_bindgen_test]
async fn an_async_import_fails_with_its_own_error_at_once() {
    let error = exit_through_async_import(9, false).await;
    assert_eq!(
        error.downcast_ref::<Exit>().map(|exit| exit.0),
        Some(9),
        "{error}"
    );
}
