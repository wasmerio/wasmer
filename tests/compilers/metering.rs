use anyhow::Result;
use wasmer_middlewares::Metering;

use std::sync::Arc;
use wasmer::wasmparser::Operator;
use wasmer::FunctionEnv;
use wasmer::*;

fn cost_always_one(_: &Operator) -> u64 {
    1
}

fn run_add_with_limit(mut config: crate::Config, limit: u64) -> Result<()> {
    config
        .middlewares
        .push(Arc::new(Metering::new(limit, cost_always_one)));
    let mut store = config.store();
    let wat = r#"(module
        (func (export "add") (param i32 i32) (result i32)
           (i32.add (local.get 0)
                    (local.get 1)))
)"#;

    let import_object = imports! {};

    let module = Module::new(&store, wat).unwrap();
    let instance = Instance::new(&mut store, &module, &import_object)?;

    let f: TypedFunction<(i32, i32), i32> =
        instance.exports.get_typed_function(&mut store, "add")?;
    f.call(&mut store, 4, 6)?;
    Ok(())
}

fn run_loop(mut config: crate::Config, limit: u64, iter_count: i32) -> Result<()> {
    config
        .middlewares
        .push(Arc::new(Metering::new(limit, cost_always_one)));
    let mut store = config.store();
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

    let instance = Instance::new(&mut store, &module, &import_object)?;

    let f: TypedFunction<i32, ()> = instance.exports.get_typed_function(&mut store, "test")?;
    f.call(&mut store, iter_count)?;
    Ok(())
}

#[compiler_test(metering)]
fn metering_ok(config: crate::Config) -> Result<()> {
    assert!(run_add_with_limit(config, 4).is_ok());
    Ok(())
}

#[compiler_test(metering)]
fn metering_fail(config: crate::Config) -> Result<()> {
    assert!(run_add_with_limit(config, 3).is_err());
    Ok(())
}

#[compiler_test(metering)]
fn loop_once(config: crate::Config) -> Result<()> {
    assert!(run_loop(config.clone(), 12, 1).is_ok());
    assert!(run_loop(config, 11, 1).is_err());
    Ok(())
}

#[compiler_test(metering)]
fn loop_twice(config: crate::Config) -> Result<()> {
    assert!(run_loop(config.clone(), 19, 2).is_ok());
    assert!(run_loop(config, 18, 2).is_err());
    Ok(())
}

/// Ported from https://github.com/wasmerio/wasmer/blob/main/tests/middleware_common.rs
#[compiler_test(metering)]
fn complex_loop(mut config: crate::Config) -> Result<()> {
    // Assemblyscript
    // export function add_to(x: i32, y: i32): i32 {
    //    for(var i = 0; i < x; i++){
    //      if(i % 1 == 0){
    //        y += i;
    //      } else {
    //        y *= i
    //      }
    //    }
    //    return y;
    // }
    static WAT: &str = r#"
    (module
        (type $t0 (func (param i32 i32) (result i32)))
        (type $t1 (func))
        (func $add_to (export "add_to") (type $t0) (param $p0 i32) (param $p1 i32) (result i32)
        (local $l0 i32)
        block $B0
            i32.const 0
            local.set $l0
            loop $L1
            local.get $l0
            local.get $p0
            i32.lt_s
            i32.eqz
            br_if $B0
            local.get $l0
            i32.const 1
            i32.rem_s
            i32.const 0
            i32.eq
            if $I2
                local.get $p1
                local.get $l0
                i32.add
                local.set $p1
            else
                local.get $p1
                local.get $l0
                i32.mul
                local.set $p1
            end
            local.get $l0
            i32.const 1
            i32.add
            local.set $l0
            br $L1
            unreachable
            end
            unreachable
        end
        local.get $p1)
        (func $f1 (type $t1))
        (table $table (export "table") 1 funcref)
        (memory $memory (export "memory") 0)
        (global $g0 i32 (i32.const 8))
        (elem (i32.const 0) $f1))
    "#;
    config
        .middlewares
        .push(Arc::new(Metering::new(100, cost_always_one)));
    let mut store = config.store();
    let mut env = FunctionEnv::new(&mut store, ());

    let module = Module::new(&store, WAT).unwrap();

    let import_object = imports! {};

    let instance = Instance::new(&mut store, &module, &import_object)?;

    let f: TypedFunction<(i32, i32), i32> =
        instance.exports.get_typed_function(&mut store, "add_to")?;

    // FIXME: Since now a metering error is signaled with an `unreachable`, it is impossible to verify
    // the error type. Fix this later.
    f.call(&mut store, 10_000_000, 4).unwrap_err();
    Ok(())
}
