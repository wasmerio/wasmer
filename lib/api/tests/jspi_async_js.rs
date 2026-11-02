#![cfg(all(feature = "experimental-async", feature = "js", target_arch = "wasm32"))]

use js_sys::Promise;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::wasm_bindgen_test;
use wasmer::{Function, Instance, Module, Store, TypedFunction, imports};

#[wasm_bindgen_test]
async fn jspi_executors_belong_to_their_store_and_freeze_before_imports() {
    use std::cell::Cell;
    thread_local! {
        static LEFT: Cell<u32> = const { Cell::new(0) };
        static RIGHT: Cell<u32> = const { Cell::new(0) };
    }
    fn left(future: wasmer::js::JspiFuture) -> Promise {
        LEFT.set(LEFT.get() + 1);
        wasm_bindgen_futures::future_to_promise(future)
    }
    fn right(future: wasmer::js::JspiFuture) -> Promise {
        RIGHT.set(RIGHT.get() + 1);
        wasm_bindgen_futures::future_to_promise(future)
    }
    fn make(
        executor: Option<wasmer::js::JspiExecutor>,
    ) -> (wasmer::StoreAsync, TypedFunction<(), i32>) {
        let mut store = Store::default();
        wasmer::js::set_async_mode(&mut store, wasmer::js::AsyncMode::Jspi).unwrap();
        if let Some(executor) = executor {
            wasmer::js::set_jspi_executor(&mut store, executor).unwrap();
        }
        let module = Module::new(
            &store,
            r#"(module
            (import "host" "answer" (func $answer (result i32)))
            (func (export "run") (result i32) call $answer))"#,
        )
        .unwrap();
        let answer = Function::new_typed_async(&mut store, || async { 42i32 });
        assert!(wasmer::js::set_jspi_executor(&mut store, right).is_err());
        let instance = Instance::new(
            &mut store,
            &module,
            &imports! {"host" => {"answer" => answer}},
        )
        .unwrap();
        let run = instance.exports.get_typed_function(&store, "run").unwrap();
        (store.into_async(), run)
    }
    LEFT.set(0);
    RIGHT.set(0);
    let (left_store, left_run) = make(Some(left));
    let (default_store, default_run) = make(None);
    let (right_store, right_run) = make(Some(right));
    assert_eq!(left_run.call_async(&left_store).await.unwrap(), 42);
    assert_eq!(default_run.call_async(&default_store).await.unwrap(), 42);
    assert_eq!(right_run.call_async(&right_store).await.unwrap(), 42);
    assert_eq!(left_run.call_async(&left_store).await.unwrap(), 42);
    assert_eq!(LEFT.get(), 2);
    assert_eq!(RIGHT.get(), 1);
}

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
async fn host_abort_keeps_its_type_through_jspi_and_nested_guest_catches() {
    use std::sync::Arc;
    use wasmer::{AsyncFunctionEnvMut, FunctionEnv, RuntimeError};
    #[derive(Debug, thiserror::Error)]
    #[error("host aborted")]
    struct Aborted(Arc<()>);
    let payload = Arc::new(());
    let original = RuntimeError::host_abort(Box::new(Aborted(payload.clone())));
    let mut store = Store::default();
    wasmer::js::set_async_mode(&mut store, wasmer::js::AsyncMode::Jspi).unwrap();
    let module = Module::new(
        &store,
        r#"(module
          (import "host" "leaf" (func $leaf))
          (import "host" "nested" (func $nested))
          (global $continued (mut i32) (i32.const 0))
          (func (export "leaf_guest")
            (block $caught
              (try_table (catch_all $caught) (call $leaf))
              (global.set $continued (i32.const 1)) (return))
            (global.set $continued (i32.const 2)))
          (func (export "run")
            (block $caught
              (try_table (catch_all $caught) (call $nested))
              (global.set $continued (i32.const 3)) (return))
            (global.set $continued (i32.const 4)))
          (func (export "continued") (result i32) global.get $continued))"#,
    )
    .unwrap();
    let leaf = Function::new_typed_async(&mut store, {
        let error = original.clone();
        move || {
            let error = error.clone();
            async move {
                JsFuture::from(Promise::resolve(&JsValue::UNDEFINED))
                    .await
                    .unwrap();
                Err::<(), _>(error)
            }
        }
    });
    let env = FunctionEnv::new(&mut store, None::<Function>);
    let nested = Function::new_typed_with_env_async(
        &mut store,
        &env,
        async |env: AsyncFunctionEnvMut<Option<Function>>| -> Result<(), RuntimeError> {
            let function = env.read().await.data().clone().unwrap();
            function.call_async(&env.as_store_async(), vec![]).await?;
            Ok(())
        },
    );
    let instance = Instance::new(
        &mut store,
        &module,
        &imports! {"host"=>{"leaf"=>leaf, "nested"=>nested}},
    )
    .unwrap();
    *env.as_mut(&mut store) = Some(instance.exports.get_function("leaf_guest").unwrap().clone());
    let store = store.into_async();
    for name in ["leaf_guest", "run"] {
        let error = instance
            .exports
            .get_function(name)
            .unwrap()
            .call_async(&store, vec![])
            .await
            .unwrap_err();
        assert!(error.is_host_abort());
        assert!(Arc::ptr_eq(
            &error.downcast_ref::<Aborted>().unwrap().0,
            &payload
        ));
    }
    let continued = instance
        .exports
        .get_function("continued")
        .unwrap()
        .call_async(&store, vec![])
        .await
        .unwrap();
    assert_eq!(continued[0].i32(), Some(0));
}

#[wasm_bindgen_test]
fn synchronous_host_abort_bypasses_guest_catches_and_preserves_ordinary_errors() {
    use std::sync::Arc;
    use wasmer::RuntimeError;
    #[derive(Debug, thiserror::Error)]
    #[error("synchronous host error")]
    struct Payload(Arc<()>);
    for abort in [false, true] {
        let payload = Arc::new(());
        let error: Box<dyn std::error::Error + Send + Sync> = Box::new(Payload(payload.clone()));
        let error = if abort {
            RuntimeError::host_abort(error)
        } else {
            RuntimeError::user(error)
        };
        let mut store = Store::default();
        let env = wasmer::FunctionEnv::new(&mut store, error);
        let host = Function::new_typed_with_env(
            &mut store,
            &env,
            |env: wasmer::FunctionEnvMut<RuntimeError>| Err::<(), _>(env.data().clone()),
        );
        let module = Module::new(
            &store,
            r#"(module
          (import "host" "raise" (func $raise))
          (global $continued (mut i32) (i32.const 0))
          (func (export "run")
            (block $caught (try_table (catch_all $caught) call $raise))
            i32.const 1 global.set $continued)
          (func (export "continued") (result i32) global.get $continued))"#,
        )
        .unwrap();
        let instance =
            Instance::new(&mut store, &module, &imports! {"host"=>{"raise"=>host}}).unwrap();
        let result = instance
            .exports
            .get_function("run")
            .unwrap()
            .call(&mut store, &[]);
        if abort {
            let error = result.unwrap_err();
            assert!(error.is_host_abort());
            assert!(Arc::ptr_eq(
                &error.downcast_ref::<Payload>().unwrap().0,
                &payload
            ));
        } else {
            result.unwrap();
        }
        let continued = instance
            .exports
            .get_function("continued")
            .unwrap()
            .call(&mut store, &[])
            .unwrap();
        assert_eq!(continued[0].i32(), Some(i32::from(!abort)));
    }
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
    // Linux fs/read_write.c:vfs_llseek transports signed 64-bit offsets.
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
