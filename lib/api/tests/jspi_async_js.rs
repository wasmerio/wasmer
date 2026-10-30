#![cfg(all(feature = "experimental-async", feature = "js", target_arch = "wasm32"))]

use js_sys::Promise;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::wasm_bindgen_test;
use wasmer::{Function, Instance, Module, Store, TypedFunction, imports};

struct NoJspi {
    webassembly: JsValue,
    suspending: JsValue,
    promising: JsValue,
}
impl NoJspi {
    fn new() -> Self {
        let webassembly = js_sys::Reflect::get(&js_sys::global(), &"WebAssembly".into()).unwrap();
        let suspending = js_sys::Reflect::get(&webassembly, &"Suspending".into()).unwrap();
        let promising = js_sys::Reflect::get(&webassembly, &"promising".into()).unwrap();
        js_sys::Reflect::set(&webassembly, &"Suspending".into(), &JsValue::UNDEFINED).unwrap();
        js_sys::Reflect::set(&webassembly, &"promising".into(), &JsValue::UNDEFINED).unwrap();
        Self {
            webassembly,
            suspending,
            promising,
        }
    }
}
impl Drop for NoJspi {
    fn drop(&mut self) {
        js_sys::Reflect::set(&self.webassembly, &"Suspending".into(), &self.suspending).unwrap();
        js_sys::Reflect::set(&self.webassembly, &"promising".into(), &self.promising).unwrap();
    }
}

#[wasm_bindgen_test]
async fn typed_async_host_and_guest_calls_use_jspi() {
    let mut store = Store::default();
    wasmer::js::set_async_mode(&mut store, wasmer::js::AsyncMode::Jspi).unwrap();
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
async fn blocking_async_calls_need_no_jspi_and_allow_nested_store_borrows() {
    let _no_jspi = NoJspi::new();
    use wasmer::{AsyncFunctionEnvMut, FunctionEnv};
    let mut store = Store::default();
    let env = FunctionEnv::new(&mut store, None::<Function>);
    let module = Module::new(
        &store,
        r#"(module
        (import "host" "nested" (func $nested (param i32) (result i32)))
        (func (export "leaf") (param i32) (result i32) local.get 0 i32.const 1 i32.add)
        (func (export "compute") (param i32) (result i32) local.get 0 call $nested))"#,
    )
    .unwrap();
    let nested = Function::new_typed_with_env_async(
        &mut store,
        &env,
        |env: AsyncFunctionEnvMut<Option<Function>>, value: i32| async move {
            let function = env.read().await.data().clone().unwrap();
            if value == 0 {
                return Ok(42);
            }
            let mut yielded = false;
            std::future::poll_fn(|cx| {
                if std::mem::replace(&mut yielded, true) {
                    std::task::Poll::Ready(())
                } else {
                    cx.waker().wake_by_ref();
                    std::task::Poll::Pending
                }
            })
            .await;
            let result = function
                .call_async(&env.as_store_async(), vec![wasmer::Value::I32(value - 1)])
                .await?;
            Ok::<i32, wasmer::RuntimeError>(result[0].i32().unwrap())
        },
    );
    let instance = Instance::new(
        &mut store,
        &module,
        &imports! {"host" => {"nested" => nested}},
    )
    .unwrap();
    *env.as_mut(&mut store) = Some(instance.exports.get_function("compute").unwrap().clone());
    let compute: TypedFunction<i32, i32> = instance
        .exports
        .get_typed_function(&store, "compute")
        .unwrap();
    assert_eq!(
        compute.call_async(&store.into_async(), 2).await.unwrap(),
        42
    );
}

#[wasm_bindgen_test]
async fn blocking_async_host_errors_keep_their_rust_type() {
    let _no_jspi = NoJspi::new();
    #[derive(Debug)]
    struct Exit;
    impl std::fmt::Display for Exit {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("exit")
        }
    }
    impl std::error::Error for Exit {}
    let mut store = Store::default();
    let exit = Function::new_typed_async(&mut store, || async {
        Err::<(), _>(wasmer::RuntimeError::user(Box::new(Exit)))
    });
    let module = Module::new(
        &store,
        r#"(module
        (import "host" "exit" (func $exit))
        (func (export "run") call $exit))"#,
    )
    .unwrap();
    let instance =
        Instance::new(&mut store, &module, &imports! {"host" => {"exit" => exit}}).unwrap();
    let run: TypedFunction<(), ()> = instance.exports.get_typed_function(&store, "run").unwrap();
    assert!(
        run.call_async(&store.into_async())
            .await
            .unwrap_err()
            .is::<Exit>()
    );
}

#[wasm_bindgen_test]
async fn wasm_exception_crosses_a_nested_blocking_host_frame() {
    let _no_jspi = NoJspi::new();
    use wasmer::{AsyncFunctionEnvMut, FunctionEnv};
    let mut store = Store::default();
    let env = FunctionEnv::new(&mut store, None::<Function>);
    let module = Module::new(
        &store,
        r#"(module
        (import "host" "nested" (func $nested (result i32)))
        (tag $jump (param i32))
        (func (export "raise") (result i32) i32.const 42 throw $jump)
        (func (export "compute") (result i32)
            (block $caught (result i32)
                (try_table (result i32) (catch $jump $caught) (call $nested)))))"#,
    )
    .unwrap();
    let nested = Function::new_typed_with_env_async(
        &mut store,
        &env,
        |env: AsyncFunctionEnvMut<Option<Function>>| async move {
            let function = env.read().await.data().clone().unwrap();
            let results = function.call_async(&env.as_store_async(), vec![]).await?;
            Ok::<i32, wasmer::RuntimeError>(results[0].i32().unwrap())
        },
    );
    let instance = Instance::new(
        &mut store,
        &module,
        &imports! {"host" => {"nested" => nested}},
    )
    .unwrap();
    *env.as_mut(&mut store) = Some(instance.exports.get_function("raise").unwrap().clone());
    let compute: TypedFunction<(), i32> = instance
        .exports
        .get_typed_function(&store, "compute")
        .unwrap();
    assert_eq!(compute.call_async(&store.into_async()).await.unwrap(), 42);
}

#[wasm_bindgen_test]
fn async_mode_is_probed_and_fixed_before_imports() {
    let absent = NoJspi::new();
    let mut store = Store::default();
    assert!(!wasmer::js::is_jspi_supported());
    assert!(wasmer::js::set_async_mode(&mut store, wasmer::js::AsyncMode::Jspi).is_err());
    let _import = Function::new_typed_async(&mut store, || async {});
    drop(absent);
    if wasmer::js::is_jspi_supported() {
        let error =
            wasmer::js::set_async_mode(&mut store, wasmer::js::AsyncMode::Jspi).unwrap_err();
        assert!(error.to_string().contains("cannot change the async lane"));
    }
}

#[wasm_bindgen_test]
async fn blocking_dynamic_host_rejects_the_wrong_result_count() {
    let _absent = NoJspi::new();
    let mut store = Store::default();
    let import = Function::new_async(
        &mut store,
        wasmer::FunctionType::new([], [wasmer::Type::I32]),
        |_| async { Ok(vec![]) },
    );
    let error = import
        .call_async(&store.into_async(), vec![])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("wrong number of results"));
}

#[wasm_bindgen_test]
async fn blocking_i64_arguments_and_host_results_keep_all_bits() {
    let _no_jspi = NoJspi::new();
    let mut store = Store::default();
    let module = Module::new(
        &store,
        r#"(module
        (import "host" "pair" (func $pair (param i64 i64) (result i64 i64)))
        (func (export "roundtrip") (param i64 i64) (result i64 i64)
          local.get 0 local.get 1 call $pair))"#,
    )
    .unwrap();
    let pair = Function::new_typed_async(&mut store, async |a: i64, b: i64| (a, b));
    let instance = Instance::new(&mut store, &module, &imports! {"host"=>{"pair"=>pair}}).unwrap();
    let function = instance.exports.get_function("roundtrip").unwrap();
    // Linux fs/read_write.c:ksys_lseek/vfs_llseek transports signed 64-bit offsets.
    // These values also catch Number's rounding above 53 bits and negative errno.
    let values = [
        wasmer::Value::I64(9_007_199_254_740_993),
        wasmer::Value::I64(i64::MIN + 1),
    ];
    let result = function
        .call_async(&store.into_async(), values.to_vec())
        .await
        .unwrap();
    assert_eq!(result.as_ref(), &values);
}
