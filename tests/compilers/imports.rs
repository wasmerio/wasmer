//! Testing the imports with different provided functions.
//! This tests checks that the provided functions (both native and
//! dynamic ones) work properly.

use crate::utils::get_store;
use anyhow::Result;
use std::convert::Infallible;
use std::sync::{
    atomic::{AtomicUsize, Ordering::SeqCst},
    Arc,
};
use wasmer::*;

fn get_module(store: &Store) -> Result<Module> {
    let wat = r#"
        (import "host" "0" (func))
        (import "host" "1" (func (param i32) (result i32)))
        (import "host" "2" (func (param i32) (param i64)))
        (import "host" "3" (func (param i32 i64 i32 f32 f64)))

        (func $foo
            call 0
            i32.const 0
            call 1
            i32.const 1
            i32.add
            i64.const 3
            call 2

            i32.const 100
            i64.const 200
            i32.const 300
            f32.const 400
            f64.const 500
            call 3
        )
        (start $foo)
    "#;

    let module = Module::new(&store, &wat)?;
    Ok(module)
}

#[test]
fn dynamic_function() -> Result<()> {
    let store = get_store(false);
    let module = get_module(&store)?;
    static HITS: AtomicUsize = AtomicUsize::new(0);
    Instance::new(
        &module,
        &imports! {
            "host" => {
                "0" => Function::new(&store, &FunctionType::new(vec![], vec![]), |_values| {
                    assert_eq!(HITS.fetch_add(1, SeqCst), 0);
                    Ok(vec![])
                }),
                "1" => Function::new(&store, &FunctionType::new(vec![ValType::I32], vec![ValType::I32]), |values| {
                    assert_eq!(values[0], Value::I32(0));
                    assert_eq!(HITS.fetch_add(1, SeqCst), 1);
                    Ok(vec![Value::I32(1)])
                }),
                "2" => Function::new(&store, &FunctionType::new(vec![ValType::I32, ValType::I64], vec![]), |values| {
                    assert_eq!(values[0], Value::I32(2));
                    assert_eq!(values[1], Value::I64(3));
                    assert_eq!(HITS.fetch_add(1, SeqCst), 2);
                    Ok(vec![])
                }),
                "3" => Function::new(&store, &FunctionType::new(vec![ValType::I32, ValType::I64, ValType::I32, ValType::F32, ValType::F64], vec![]), |values| {
                    assert_eq!(values[0], Value::I32(100));
                    assert_eq!(values[1], Value::I64(200));
                    assert_eq!(values[2], Value::I32(300));
                    assert_eq!(values[3], Value::F32(400.0));
                    assert_eq!(values[4], Value::F64(500.0));
                    assert_eq!(HITS.fetch_add(1, SeqCst), 3);
                    Ok(vec![])
                }),
            },
        },
    )?;
    assert_eq!(HITS.load(SeqCst), 4);
    Ok(())
}

#[test]
fn dynamic_function_with_env() -> Result<()> {
    let store = get_store(false);
    let module = get_module(&store)?;

    #[derive(Clone)]
    struct Env(Arc<AtomicUsize>);

    impl std::ops::Deref for Env {
        type Target = Arc<AtomicUsize>;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl WasmerEnv for Env {
        fn finish(&mut self, _instance: &Instance) -> Result<(), HostEnvInitError> {
            Ok(())
        }
    }

    let env: Env = Env(Arc::new(AtomicUsize::new(0)));
    Instance::new(
        &module,
        &imports! {
            "host" => {
                "0" => Function::new_with_env(&store, &FunctionType::new(vec![], vec![]), env.clone(), |env, _values| {
                    assert_eq!(env.fetch_add(1, SeqCst), 0);
                    Ok(vec![])
                }),
                "1" => Function::new_with_env(&store, &FunctionType::new(vec![ValType::I32], vec![ValType::I32]), env.clone(), |env, values| {
                    assert_eq!(values[0], Value::I32(0));
                    assert_eq!(env.fetch_add(1, SeqCst), 1);
                    Ok(vec![Value::I32(1)])
                }),
                "2" => Function::new_with_env(&store, &FunctionType::new(vec![ValType::I32, ValType::I64], vec![]), env.clone(), |env, values| {
                    assert_eq!(values[0], Value::I32(2));
                    assert_eq!(values[1], Value::I64(3));
                    assert_eq!(env.fetch_add(1, SeqCst), 2);
                    Ok(vec![])
                }),
                "3" => Function::new_with_env(&store, &FunctionType::new(vec![ValType::I32, ValType::I64, ValType::I32, ValType::F32, ValType::F64], vec![]), env.clone(), |env, values| {
                    assert_eq!(values[0], Value::I32(100));
                    assert_eq!(values[1], Value::I64(200));
                    assert_eq!(values[2], Value::I32(300));
                    assert_eq!(values[3], Value::F32(400.0));
                    assert_eq!(values[4], Value::F64(500.0));
                    assert_eq!(env.fetch_add(1, SeqCst), 3);
                    Ok(vec![])
                }),
            },
        },
    )?;
    assert_eq!(env.load(SeqCst), 4);
    Ok(())
}

#[test]
fn static_function() -> Result<()> {
    let store = get_store(false);
    let module = get_module(&store)?;

    static HITS: AtomicUsize = AtomicUsize::new(0);
    Instance::new(
        &module,
        &imports! {
            "host" => {
                "0" => Function::new_native(&store, || {
                    assert_eq!(HITS.fetch_add(1, SeqCst), 0);
                }),
                "1" => Function::new_native(&store, |x: i32| -> i32 {
                    assert_eq!(x, 0);
                    assert_eq!(HITS.fetch_add(1, SeqCst), 1);
                    1
                }),
                "2" => Function::new_native(&store, |x: i32, y: i64| {
                    assert_eq!(x, 2);
                    assert_eq!(y, 3);
                    assert_eq!(HITS.fetch_add(1, SeqCst), 2);
                }),
                "3" => Function::new_native(&store, |a: i32, b: i64, c: i32, d: f32, e: f64| {
                    assert_eq!(a, 100);
                    assert_eq!(b, 200);
                    assert_eq!(c, 300);
                    assert_eq!(d, 400.0);
                    assert_eq!(e, 500.0);
                    assert_eq!(HITS.fetch_add(1, SeqCst), 3);
                }),
            },
        },
    )?;
    assert_eq!(HITS.load(SeqCst), 4);
    Ok(())
}

#[test]
fn static_function_with_results() -> Result<()> {
    let store = get_store(false);
    let module = get_module(&store)?;

    static HITS: AtomicUsize = AtomicUsize::new(0);
    Instance::new(
        &module,
        &imports! {
            "host" => {
                "0" => Function::new_native(&store, || {
                    assert_eq!(HITS.fetch_add(1, SeqCst), 0);
                }),
                "1" => Function::new_native(&store, |x: i32| -> Result<i32, Infallible> {
                    assert_eq!(x, 0);
                    assert_eq!(HITS.fetch_add(1, SeqCst), 1);
                    Ok(1)
                }),
                "2" => Function::new_native(&store, |x: i32, y: i64| {
                    assert_eq!(x, 2);
                    assert_eq!(y, 3);
                    assert_eq!(HITS.fetch_add(1, SeqCst), 2);
                }),
                "3" => Function::new_native(&store, |a: i32, b: i64, c: i32, d: f32, e: f64| {
                    assert_eq!(a, 100);
                    assert_eq!(b, 200);
                    assert_eq!(c, 300);
                    assert_eq!(d, 400.0);
                    assert_eq!(e, 500.0);
                    assert_eq!(HITS.fetch_add(1, SeqCst), 3);
                }),
            },
        },
    )?;
    assert_eq!(HITS.load(SeqCst), 4);
    Ok(())
}

#[test]
fn static_function_with_env() -> Result<()> {
    let store = get_store(false);
    let module = get_module(&store)?;

    #[derive(Clone)]
    struct Env(Arc<AtomicUsize>);

    impl std::ops::Deref for Env {
        type Target = Arc<AtomicUsize>;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl WasmerEnv for Env {
        fn finish(&mut self, _instance: &Instance) -> Result<(), HostEnvInitError> {
            Ok(())
        }
    }

    let env: Env = Env(Arc::new(AtomicUsize::new(0)));
    Instance::new(
        &module,
        &imports! {
            "host" => {
                "0" => Function::new_native_with_env(&store, env.clone(), |env: &mut Env| {
                    assert_eq!(env.fetch_add(1, SeqCst), 0);
                }),
                "1" => Function::new_native_with_env(&store, env.clone(), |env: &mut Env, x: i32| -> i32 {
                    assert_eq!(x, 0);
                    assert_eq!(env.fetch_add(1, SeqCst), 1);
                    1
                }),
                "2" => Function::new_native_with_env(&store, env.clone(), |env: &mut Env, x: i32, y: i64| {
                    assert_eq!(x, 2);
                    assert_eq!(y, 3);
                    assert_eq!(env.fetch_add(1, SeqCst), 2);
                }),
                "3" => Function::new_native_with_env(&store, env.clone(), |env: &mut Env, a: i32, b: i64, c: i32, d: f32, e: f64| {
                    assert_eq!(a, 100);
                    assert_eq!(b, 200);
                    assert_eq!(c, 300);
                    assert_eq!(d, 400.0);
                    assert_eq!(e, 500.0);
                    assert_eq!(env.fetch_add(1, SeqCst), 3);
                }),
            },
        },
    )?;
    assert_eq!(env.load(SeqCst), 4);
    Ok(())
}

#[test]
fn static_function_that_fails() -> Result<()> {
    let store = get_store(false);
    let wat = r#"
        (import "host" "0" (func))

        (func $foo
            call 0
        )
        (start $foo)
    "#;

    let module = Module::new(&store, &wat)?;

    let result = Instance::new(
        &module,
        &imports! {
            "host" => {
                "0" => Function::new_native(&store, || -> Result<Infallible, RuntimeError> {
                    Err(RuntimeError::new("oops"))
                }),
            },
        },
    );

    assert!(result.is_err());

    match result {
        Err(InstantiationError::Start(runtime_error)) => {
            assert_eq!(runtime_error.message(), "oops")
        }
        _ => assert!(false),
    }

    Ok(())
}
