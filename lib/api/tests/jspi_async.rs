#![cfg(feature = "experimental-async")]

use std::{cell::RefCell, sync::OnceLock};

use anyhow::Result;
use futures::future;
use wasmer::{
    AsyncFunctionEnvMut, Function, FunctionEnv, FunctionEnvMut, FunctionType, Instance, Module,
    Store, StoreAsync, Type, TypedFunction, Value, imports,
};
use wasmer_vm::TrapCode;

#[derive(Default)]
struct DeltaState {
    deltas: Vec<f64>,
    index: usize,
}

impl DeltaState {
    fn next(&mut self) -> f64 {
        let value = self.deltas.get(self.index).copied().unwrap_or(0.0);
        self.index += 1;
        value
    }
}

fn jspi_module() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    const JSPI_WAT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/examples/jspi.wat");
    BYTES.get_or_init(|| wat::parse_file(JSPI_WAT).expect("valid example module"))
}

#[test]
fn async_state_updates_follow_jspi_example() -> Result<()> {
    let wasm = jspi_module();
    let mut store = Store::default();
    let module = Module::new(&store, wasm)?;

    let init_state = Function::new_async(
        &mut store,
        FunctionType::new(vec![], vec![Type::F64]),
        |_values| async move {
            // Note: future::ready doesn't actually suspend. It's important
            // to note that, while we're in an async import here, it's
            // impossible to suspend during module instantiation, which is
            // where this import is called.
            // To see this in action, uncomment the following line:
            // tokio::task::yield_now().await;
            future::ready(()).await;
            Ok(vec![Value::F64(1.0)])
        },
    );

    let delta_env = FunctionEnv::new(
        &mut store,
        DeltaState {
            deltas: vec![0.5, -1.0, 2.5],
            index: 0,
        },
    );
    let compute_delta = Function::new_with_env_async(
        &mut store,
        &delta_env,
        FunctionType::new(vec![], vec![Type::F64]),
        |env: AsyncFunctionEnvMut<DeltaState>, _values| async move {
            // Note: holding a lock across an await point prevents
            // other coroutines from progressing, so it's a good
            // idea to drop the lock before awaiting.
            let delta = {
                let mut env_write = env.write().await;
                env_write.data_mut().next()
            };
            // We can, however, actually suspend whenever
            // `Function::call_async` is used to call WASM functions.
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            Ok(vec![Value::F64(delta)])
        },
    );

    let import_object = imports! {
        "js" => {
            "init_state" => init_state,
            "compute_delta" => compute_delta,
        }
    };

    let instance = Instance::new(&mut store, &module, &import_object)?;
    let get_state = instance.exports.get_function("get_state")?;
    let update_state = instance.exports.get_function("update_state")?;

    fn as_f64(values: &[Value]) -> f64 {
        match &values[0] {
            Value::F64(v) => *v,
            other => panic!("expected f64 value, got {other:?}"),
        }
    }

    assert_eq!(as_f64(&get_state.call(&mut store, &[])?), 1.0);

    let step = |store: &StoreAsync, func: &wasmer::Function| -> Result<f64> {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(func.call_async(store, vec![]))?;
        Ok(as_f64(&result))
    };

    let store_async = store.into_async();

    assert_eq!(step(&store_async, update_state)?, 1.5);
    assert_eq!(step(&store_async, update_state)?, 0.5);
    assert_eq!(step(&store_async, update_state)?, 3.0);

    Ok(())
}

#[test]
fn typed_async_host_and_calls_work() -> Result<()> {
    let wasm = wat::parse_str(
        r#"
        (module
          (import "host" "async_add" (func $async_add (param i32 i32) (result i32)))
          (import "host" "async_double" (func $async_double (param i32) (result i32)))
          (func (export "compute") (param i32) (result i32)
            local.get 0
            i32.const 10
            call $async_add
            local.get 0
            call $async_double
            i32.add))
        "#,
    )?;

    #[derive(Clone, Copy)]
    struct AddBias {
        bias: i32,
    }

    let mut store = Store::default();
    let module = Module::new(&store, wasm)?;

    let add_env = FunctionEnv::new(&mut store, AddBias { bias: 5 });
    let async_add = Function::new_typed_with_env_async(
        &mut store,
        &add_env,
        async move |env: AsyncFunctionEnvMut<AddBias>, a: i32, b: i32| {
            let env_read = env.read().await;
            let bias = env_read.data().bias;
            tokio::task::yield_now().await;
            a + b + bias
        },
    );
    let async_double = Function::new_typed_async(&mut store, async move |value: i32| {
        tokio::task::yield_now().await;
        value * 2
    });

    let import_object = imports! {
        "host" => {
            "async_add" => async_add,
            "async_double" => async_double,
        }
    };

    let instance = Instance::new(&mut store, &module, &import_object)?;
    let compute: TypedFunction<i32, i32> =
        instance.exports.get_typed_function(&store, "compute")?;

    let store_async = store.into_async();

    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(compute.call_async(&store_async, 4))?;
    assert_eq!(result, 27);

    Ok(())
}

#[test]
fn cannot_yield_when_not_in_async_context() -> Result<()> {
    const WAT: &str = r#"
    (module
        (import "env" "yield_now" (func $yield_now))
        (func (export "yield_outside")
            call $yield_now
        )
    )
    "#;
    let wasm = wat::parse_str(WAT).expect("valid WAT module");

    let mut store = Store::default();
    let module = Module::new(&store, wasm)?;

    let yield_now = Function::new_async(
        &mut store,
        FunctionType::new(vec![], vec![]),
        |_values| async move {
            // Attempting to yield when not in an async context should trap.
            tokio::task::yield_now().await;
            Ok(vec![])
        },
    );

    let import_object = imports! {
        "env" => {
            "yield_now" => yield_now,
        }
    };
    let instance = Instance::new(&mut store, &module, &import_object)?;
    let yield_outside = instance.exports.get_function("yield_outside")?;

    let trap = yield_outside
        .call(&mut store, &[])
        .expect_err("expected trap calling yield outside async context");

    // TODO: wasm trace generation appears to be broken?
    // assert!(!trap.trace().is_empty(), "should have a stack trace");
    let trap_code = trap.to_trap().expect("expected trap code");
    assert_eq!(
        trap_code,
        TrapCode::YieldOutsideAsyncContext,
        "expected YieldOutsideAsyncContext trap code"
    );

    Ok(())
}

#[test]
fn nested_async_in_sync() -> Result<()> {
    const WAT: &str = r#"
    (module
        (import "env" "sync" (func $sync (result i32)))
        (import "env" "async" (func $async (result i32)))
        (func (export "entry") (result i32)
            call $sync
        )
        (func (export "inner_async") (result i32)
            call $async
        )
    )
    "#;
    let wasm = wat::parse_str(WAT).expect("valid WAT module");

    let mut store = Store::default();
    let module = Module::new(&store, wasm)?;

    struct Env {
        inner_async: RefCell<Option<wasmer::TypedFunction<(), i32>>>,
    }
    let env = FunctionEnv::new(
        &mut store,
        Env {
            inner_async: RefCell::new(None),
        },
    );

    let sync = Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>| {
        let (env, mut store) = env.data_and_store_mut();
        env.inner_async
            .borrow()
            .as_ref()
            .expect("inner_async function to be set")
            .call(&mut store)
            .expect("inner async call to succeed")
    });

    let async_ = Function::new_typed_async(&mut store, async || {
        tokio::task::yield_now().await;
        42
    });

    let imports = imports! {
        "env" => {
            "sync" => sync,
            "async" => async_,
        }
    };

    let instance = Instance::new(&mut store, &module, &imports)?;

    let inner_async = instance
        .exports
        .get_typed_function::<(), i32>(&store, "inner_async")
        .unwrap();
    env.as_mut(&mut store)
        .inner_async
        .borrow_mut()
        .replace(inner_async);

    let entry = instance
        .exports
        .get_typed_function::<(), i32>(&store, "entry")?;
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(entry.call_async(&store.into_async()))?;

    assert_eq!(result, 42);

    Ok(())
}
