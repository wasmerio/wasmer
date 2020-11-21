use crate::utils::get_store;
use anyhow::Result;
use std::cell::RefCell;
use std::convert::Infallible;
use std::rc::Rc;

use wasmer::*;

fn long_f(a: u32, b: u32, c: u32, d: u32, e: u32, f: u16, g: u64, h: u64, i: u16, j: u32) -> u64 {
    j as u64
        + i as u64 * 10
        + h * 100
        + g * 1000
        + f as u64 * 10000
        + e as u64 * 100000
        + d as u64 * 1000000
        + c as u64 * 10000000
        + b as u64 * 100000000
        + a as u64 * 1000000000
}

fn long_f_dynamic(values: &[Value]) -> Result<Vec<Value>, RuntimeError> {
    Ok(vec![Value::I64(
        values[9].unwrap_i32() as i64
            + values[8].unwrap_i32() as i64 * 10
            + values[7].unwrap_i64() * 100
            + values[6].unwrap_i64() * 1000
            + values[5].unwrap_i32() as i64 * 10000
            + values[4].unwrap_i32() as i64 * 100000
            + values[3].unwrap_i32() as i64 * 1000000
            + values[2].unwrap_i32() as i64 * 10000000
            + values[1].unwrap_i32() as i64 * 100000000
            + values[0].unwrap_i32() as i64 * 1000000000,
    )])
}

#[test]
fn native_function_works_for_wasm() -> Result<()> {
    let store = get_store(false);
    let wat = r#"(module
        (func $multiply (import "env" "multiply") (param i32 i32) (result i32))
        (func (export "add") (param i32 i32) (result i32)
           (i32.add (local.get 0)
                    (local.get 1)))
        (func (export "double_then_add") (param i32 i32) (result i32)
           (i32.add (call $multiply (local.get 0) (i32.const 2))
                    (call $multiply (local.get 1) (i32.const 2))))
)"#;
    let module = Module::new(&store, wat).unwrap();

    let import_object = imports! {
        "env" => {
            "multiply" => Function::new_native(&store, |a: i32, b: i32| a * b),
        },
    };

    let instance = Instance::new(&module, &import_object)?;

    {
        let f: NativeFunc<(i32, i32), i32> = instance.exports.get_native_function("add")?;
        let result = f.call(4, 6)?;
        assert_eq!(result, 10);
    }

    {
        let f: &Function = instance.exports.get("double_then_add")?;
        let result = f.call(&[Val::I32(4), Val::I32(6)])?;
        assert_eq!(result[0], Val::I32(20));
    }

    {
        let dyn_f: &Function = instance.exports.get("double_then_add")?;
        let f: NativeFunc<(i32, i32), i32> = dyn_f.native().unwrap();
        let result = f.call(4, 6)?;
        assert_eq!(result, 20);
    }

    Ok(())
}

// The native ABI for functions fails when defining a function natively in
// macos (Darwin) with the Apple Silicon ARM chip
// TODO: Cranelift should have a good ABI for the ABI
#[test]
#[cfg_attr(
    all(
        feature = "test-cranelift",
        target_os = "macos",
        target_arch = "aarch64",
    ),
    ignore
)]
fn native_function_works_for_wasm_function_manyparams() -> Result<()> {
    let store = get_store(false);
    let wat = r#"(module
        (func $longf (import "env" "longf") (param i32 i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i64))
        (func (export "longf_pure") (param i32 i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i64)
           (call $longf (local.get 0) (local.get 1) (local.get 2) (local.get 3) (local.get 4) (local.get 5) (local.get 6) (local.get 7) (local.get 8) (local.get 9)))
        (func (export "longf") (result i64)
           (call $longf (i32.const 1) (i32.const 2) (i32.const 3) (i32.const 4) (i32.const 5) (i32.const 6) (i64.const 7) (i64.const 8) (i32.const 9) (i32.const 0)))
)"#;
    let module = Module::new(&store, wat).unwrap();

    let import_object = imports! {
        "env" => {
            "longf" => Function::new_native(&store, long_f),
        },
    };

    let instance = Instance::new(&module, &import_object)?;

    {
        let dyn_f: &Function = instance.exports.get("longf")?;
        let f: NativeFunc<(), i64> = dyn_f.native().unwrap();
        let result = f.call()?;
        assert_eq!(result, 1234567890);
    }

    {
        let dyn_f: &Function = instance.exports.get("longf_pure")?;
        let f: NativeFunc<(u32, u32, u32, u32, u32, u16, u64, u64, u16, u32), i64> =
            dyn_f.native().unwrap();
        let result = f.call(1, 2, 3, 4, 5, 6, 7, 8, 9, 0)?;
        assert_eq!(result, 1234567890);
    }

    Ok(())
}

#[test]
fn native_function_works_for_wasm_function_manyparams_dynamic() -> Result<()> {
    let store = get_store(false);
    let wat = r#"(module
        (func $longf (import "env" "longf") (param i32 i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i64))
        (func (export "longf_pure") (param i32 i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i64)
           (call $longf (local.get 0) (local.get 1) (local.get 2) (local.get 3) (local.get 4) (local.get 5) (local.get 6) (local.get 7) (local.get 8) (local.get 9)))
        (func (export "longf") (result i64)
           (call $longf (i32.const 1) (i32.const 2) (i32.const 3) (i32.const 4) (i32.const 5) (i32.const 6) (i64.const 7) (i64.const 8) (i32.const 9) (i32.const 0)))
)"#;
    let module = Module::new(&store, wat).unwrap();

    let import_object = imports! {
        "env" => {
            "longf" => Function::new(&store, &FunctionType::new(vec![ValType::I32, ValType::I32, ValType::I32, ValType::I32, ValType::I32, ValType::I32, ValType::I64 , ValType::I64 ,ValType::I32, ValType::I32], vec![ValType::I64]), long_f_dynamic),
        },
    };

    let instance = Instance::new(&module, &import_object)?;

    {
        let dyn_f: &Function = instance.exports.get("longf")?;
        let f: NativeFunc<(), i64> = dyn_f.native().unwrap();
        let result = f.call()?;
        assert_eq!(result, 1234567890);
    }

    {
        let dyn_f: &Function = instance.exports.get("longf_pure")?;
        let f: NativeFunc<(u32, u32, u32, u32, u32, u16, u64, u64, u16, u32), i64> =
            dyn_f.native().unwrap();
        let result = f.call(1, 2, 3, 4, 5, 6, 7, 8, 9, 0)?;
        assert_eq!(result, 1234567890);
    }

    Ok(())
}

#[test]
fn static_host_function_without_env() -> anyhow::Result<()> {
    let store = get_store(false);

    fn f(a: i32, b: i64, c: f32, d: f64) -> (f64, f32, i64, i32) {
        (d * 4.0, c * 3.0, b * 2, a * 1)
    }

    fn f_ok(a: i32, b: i64, c: f32, d: f64) -> Result<(f64, f32, i64, i32), Infallible> {
        Ok((d * 4.0, c * 3.0, b * 2, a * 1))
    }

    fn long_f(
        a: u32,
        b: u32,
        c: u32,
        d: u32,
        e: u32,
        f: u16,
        g: u64,
        h: u64,
        i: u16,
        j: u32,
    ) -> (u32, u64, u32) {
        (
            a + b * 10 + c * 100 + d * 1000 + e * 10000 + f as u32 * 100000,
            g + h * 10,
            i as u32 + j * 10,
        )
    }

    // Native static host function that returns a tuple.
    {
        let f = Function::new_native(&store, f);
        let f_native: NativeFunc<(i32, i64, f32, f64), (f64, f32, i64, i32)> = f.native().unwrap();
        let result = f_native.call(1, 3, 5.0, 7.0)?;
        assert_eq!(result, (28.0, 15.0, 6, 1));
    }

    // Native static host function that returns a tuple.
    {
        let long_f = Function::new_native(&store, long_f);
        let long_f_native: NativeFunc<
            (u32, u32, u32, u32, u32, u16, u64, u64, u16, u32),
            (u32, u64, u32),
        > = long_f.native().unwrap();
        let result = long_f_native.call(1, 2, 3, 4, 5, 6, 7, 8, 9, 0)?;
        assert_eq!(result, (654321, 87, 09));
    }

    // Native static host function that returns a result of a tuple.
    {
        let f = Function::new_native(&store, f_ok);
        let f_native: NativeFunc<(i32, i64, f32, f64), (f64, f32, i64, i32)> = f.native().unwrap();
        let result = f_native.call(1, 3, 5.0, 7.0)?;
        assert_eq!(result, (28.0, 15.0, 6, 1));
    }

    Ok(())
}

#[test]
fn static_host_function_with_env() -> anyhow::Result<()> {
    let store = get_store(false);

    fn f(env: &Env, a: i32, b: i64, c: f32, d: f64) -> (f64, f32, i64, i32) {
        assert_eq!(*env.0.borrow(), 100);
        env.0.replace(101);

        (d * 4.0, c * 3.0, b * 2, a * 1)
    }

    fn f_ok(env: &Env, a: i32, b: i64, c: f32, d: f64) -> Result<(f64, f32, i64, i32), Infallible> {
        assert_eq!(*env.0.borrow(), 100);
        env.0.replace(101);

        Ok((d * 4.0, c * 3.0, b * 2, a * 1))
    }

    #[derive(Clone)]
    struct Env(Rc<RefCell<i32>>);

    // Native static host function that returns a tuple.
    {
        let env = Env(Rc::new(RefCell::new(100)));

        let f = Function::new_native_with_env(&store, env.clone(), f);
        let f_native: NativeFunc<(i32, i64, f32, f64), (f64, f32, i64, i32)> = f.native().unwrap();

        assert_eq!(*env.0.borrow(), 100);

        let result = f_native.call(1, 3, 5.0, 7.0)?;

        assert_eq!(result, (28.0, 15.0, 6, 1));
        assert_eq!(*env.0.borrow(), 101);
    }

    // Native static host function that returns a result of a tuple.
    {
        let env = Env(Rc::new(RefCell::new(100)));

        let f = Function::new_native_with_env(&store, env.clone(), f_ok);
        let f_native: NativeFunc<(i32, i64, f32, f64), (f64, f32, i64, i32)> = f.native().unwrap();

        assert_eq!(*env.0.borrow(), 100);

        let result = f_native.call(1, 3, 5.0, 7.0)?;

        assert_eq!(result, (28.0, 15.0, 6, 1));
        assert_eq!(*env.0.borrow(), 101);
    }

    Ok(())
}

#[test]
fn dynamic_host_function_without_env() -> anyhow::Result<()> {
    let store = get_store(false);

    let f = Function::new(
        &store,
        &FunctionType::new(
            vec![ValType::I32, ValType::I64, ValType::F32, ValType::F64],
            vec![ValType::F64, ValType::F32, ValType::I64, ValType::I32],
        ),
        |values| {
            Ok(vec![
                Value::F64(values[3].unwrap_f64() * 4.0),
                Value::F32(values[2].unwrap_f32() * 3.0),
                Value::I64(values[1].unwrap_i64() * 2),
                Value::I32(values[0].unwrap_i32() * 1),
            ])
        },
    );
    let f_native: NativeFunc<(i32, i64, f32, f64), (f64, f32, i64, i32)> = f.native().unwrap();
    let result = f_native.call(1, 3, 5.0, 7.0)?;

    assert_eq!(result, (28.0, 15.0, 6, 1));

    Ok(())
}

#[test]
fn dynamic_host_function_with_env() -> anyhow::Result<()> {
    let store = get_store(false);

    #[derive(Clone)]
    struct Env(Rc<RefCell<i32>>);

    let env = Env(Rc::new(RefCell::new(100)));
    let f = Function::new_with_env(
        &store,
        &FunctionType::new(
            vec![ValType::I32, ValType::I64, ValType::F32, ValType::F64],
            vec![ValType::F64, ValType::F32, ValType::I64, ValType::I32],
        ),
        env.clone(),
        |env, values| {
            assert_eq!(*env.0.borrow(), 100);

            env.0.replace(101);

            Ok(vec![
                Value::F64(values[3].unwrap_f64() * 4.0),
                Value::F32(values[2].unwrap_f32() * 3.0),
                Value::I64(values[1].unwrap_i64() * 2),
                Value::I32(values[0].unwrap_i32() * 1),
            ])
        },
    );

    let f_native: NativeFunc<(i32, i64, f32, f64), (f64, f32, i64, i32)> = f.native().unwrap();

    assert_eq!(*env.0.borrow(), 100);

    let result = f_native.call(1, 3, 5.0, 7.0)?;

    assert_eq!(result, (28.0, 15.0, 6, 1));
    assert_eq!(*env.0.borrow(), 101);

    Ok(())
}
