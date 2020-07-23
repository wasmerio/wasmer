use crate::utils::get_store;
use anyhow::Result;
use std::cell::RefCell;
use std::convert::Infallible;
use std::rc::Rc;

use wasmer::*;

#[test]
fn native_function_works_for_wasm() -> Result<()> {
    let store = get_store();
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

#[test]
fn static_host_function_without_env() -> anyhow::Result<()> {
    let store = get_store();

    fn f(a: i32, b: i64, c: f32, d: f64) -> (f64, f32, i64, i32) {
        (d * 4.0, c * 3.0, b * 2, a * 1)
    }

    fn f_ok(a: i32, b: i64, c: f32, d: f64) -> Result<(f64, f32, i64, i32), Infallible> {
        Ok((d * 4.0, c * 3.0, b * 2, a * 1))
    }

    // Native static host function that returns a tuple.
    {
        let f = Function::new_native(&store, f);
        let f_native: NativeFunc<(i32, i64, f32, f64), (f64, f32, i64, i32)> = f.native().unwrap();
        let result = f_native.call(1, 3, 5.0, 7.0)?;
        assert_eq!(result, (28.0, 15.0, 6, 1));
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
    let store = get_store();

    fn f(env: &mut Env, a: i32, b: i64, c: f32, d: f64) -> (f64, f32, i64, i32) {
        assert_eq!(*env.0.borrow(), 100);
        env.0.replace(101);

        (d * 4.0, c * 3.0, b * 2, a * 1)
    }

    fn f_ok(
        env: &mut Env,
        a: i32,
        b: i64,
        c: f32,
        d: f64,
    ) -> Result<(f64, f32, i64, i32), Infallible> {
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
    let store = get_store();

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
    let store = get_store();

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
