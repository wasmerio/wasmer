use crate::utils::get_store_with_middlewares;
use anyhow::Result;
use wasmer_middlewares::Metering;

use std::sync::Arc;
use wasmer::wasmparser::{Operator, Result as WpResult};
use wasmer::*;

fn cost_always_one(_: &Operator) -> u64 {
    1
}

fn run_add_with_limit(limit: u64) -> Result<()> {
    let store = get_store_with_middlewares(std::iter::once(Arc::new(Metering::new(
        limit,
        cost_always_one,
    )) as Arc<dyn ModuleMiddleware>));
    let wat = r#"(module
        (func (export "add") (param i32 i32) (result i32)
           (i32.add (local.get 0)
                    (local.get 1)))
)"#;
    let module = Module::new(&store, wat).unwrap();

    let import_object = imports! {};

    let instance = Instance::new(&module, &import_object)?;

    let f: NativeFunc<(i32, i32), i32> = instance.exports.get_native_function("add")?;
    let result = f.call(4, 6)?;
    Ok(())
}

fn run_loop(limit: u64, iter_count: i32) -> Result<()> {
    let store = get_store_with_middlewares(std::iter::once(Arc::new(Metering::new(
        limit,
        cost_always_one,
    )) as Arc<dyn ModuleMiddleware>));
    let wat = r#"(module
        (func (export "test") (param i32)
           (local i32)
           (local.set 1 (i32.const 0))
           (loop
            (local.get 1)
            (i32.const 1)
            (i32.add)
            (local.tee 1)
            (local.get 0)
            (i32.ne)
            (br_if 0)
           )
        )
)"#;
    let module = Module::new(&store, wat).unwrap();

    let import_object = imports! {};

    let instance = Instance::new(&module, &import_object)?;

    let f: NativeFunc<i32, ()> = instance.exports.get_native_function("test")?;
    f.call(iter_count)?;
    Ok(())
}

#[test]
fn metering_ok() -> Result<()> {
    assert!(run_add_with_limit(4).is_ok());
    Ok(())
}

#[test]
fn metering_fail() -> Result<()> {
    assert!(run_add_with_limit(3).is_err());
    Ok(())
}

#[test]
fn loop_once() -> Result<()> {
    assert!(run_loop(12, 1).is_ok());
    assert!(run_loop(11, 1).is_err());
    Ok(())
}

#[test]
fn loop_twice() -> Result<()> {
    assert!(run_loop(19, 2).is_ok());
    assert!(run_loop(18, 2).is_err());
    Ok(())
}
