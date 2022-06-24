use anyhow::Result;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use wasmer::Context as WasmerContext;
use wasmer::Type as ValueType;
use wasmer::*;

fn long_f(
    _ctx: ContextMut<()>,
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
) -> u64 {
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

fn long_f_dynamic(_ctx: ContextMut<()>, values: &[Value]) -> Result<Vec<Value>, RuntimeError> {
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

#[compiler_test(native_functions)]
fn native_function_works_for_wasm(config: crate::Config) -> anyhow::Result<()> {
    let store = config.store();
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
    let mut ctx = WasmerContext::new(&store, ());

    let import_object = imports! {
        "env" => {
            "multiply" => Function::new_native(&mut ctx, |_ctx: ContextMut<_>, a: i32, b: i32| a * b),
        },
    };
    let instance = Instance::new(&mut ctx, &module, &import_object)?;

    {
        let f: TypedFunction<(i32, i32), i32> =
            instance.exports.get_typed_function(&mut ctx, "add")?;
        let result = f.call(&mut ctx, 4, 6)?;
        assert_eq!(result, 10);
    }

    {
        let f: &Function = instance.exports.get("double_then_add")?;
        let result = f.call(&mut ctx, &[Value::I32(4), Value::I32(6)])?;
        assert_eq!(result[0], Value::I32(20));
    }

    {
        let dyn_f: &Function = instance.exports.get("double_then_add")?;
        let f: TypedFunction<(i32, i32), i32> = dyn_f.native(&mut ctx).unwrap();
        let result = f.call(&mut ctx, 4, 6)?;
        assert_eq!(result, 20);
    }

    Ok(())
}

#[compiler_test(native_functions)]
fn native_host_function_closure_panics(config: crate::Config) {
    let store = config.store();
    let mut ctx = WasmerContext::new(&store, ());
    let state = 3;
    Function::new_native(&mut ctx, move |_ctx: ContextMut<_>, _: i32| {
        println!("{}", state);
    });
}

#[compiler_test(native_functions)]
fn native_with_env_host_function_closure_panics(config: crate::Config) {
    let store = config.store();
    let env: i32 = 4;
    let mut ctx = WasmerContext::new(&store, env);
    let state = 3;
    Function::new_native(&mut ctx, move |_ctx: ContextMut<i32>, _: i32| {
        println!("{}", state);
    });
}

#[compiler_test(native_functions)]
fn non_native_functions_and_closures_with_no_env_work(config: crate::Config) -> anyhow::Result<()> {
    let store = config.store();
    let wat = r#"(module
        (func $multiply1 (import "env" "multiply1") (param i32 i32) (result i32))
        (func $multiply2 (import "env" "multiply2") (param i32 i32) (result i32))
        (func $multiply3 (import "env" "multiply3") (param i32 i32) (result i32))
        (func $multiply4 (import "env" "multiply4") (param i32 i32) (result i32))

        (func (export "test") (param i32 i32 i32 i32 i32) (result i32)
           (call $multiply4
             (call $multiply3
               (call $multiply2
                  (call $multiply1
                    (local.get 0)
                    (local.get 1))
                  (local.get 2))
               (local.get 3))
              (local.get 4)))
)"#;
    let module = Module::new(&store, wat).unwrap();
    let env: i32 = 10;
    let mut ctx = WasmerContext::new(&store, env);
    let ty = FunctionType::new(vec![Type::I32, Type::I32], vec![Type::I32]);
    let captured_by_closure = 20;
    let import_object = imports! {
        "env" => {
            "multiply1" => Function::new(&mut ctx, &ty, move |_ctx, args| {
                if let (Value::I32(v1), Value::I32(v2)) = (&args[0], &args[1]) {
                    Ok(vec![Value::I32(v1 * v2 * captured_by_closure)])
                } else {
                    panic!("Invalid arguments");
                }
            }),
            "multiply2" => Function::new(&mut ctx, &ty, move |ctx, args| {
                if let (Value::I32(v1), Value::I32(v2)) = (&args[0], &args[1]) {
                    Ok(vec![Value::I32(v1 * v2 * captured_by_closure * ctx.data())])
                } else {
                    panic!("Invalid arguments");
                }
            }),
            "multiply3" => Function::new_native(&mut ctx, |_ctx: ContextMut<_>, arg1: i32, arg2: i32| -> i32
                                                {arg1 * arg2 }),
            "multiply4" => Function::new_native(&mut ctx, |ctx: ContextMut<i32>, arg1: i32, arg2: i32| -> i32
                                                         {arg1 * arg2 * ctx.data() }),
        },
    };

    let instance = Instance::new(&mut ctx, &module, &import_object)?;

    let test: TypedFunction<(i32, i32, i32, i32, i32), i32> =
        instance.exports.get_typed_function(&mut ctx, "test")?;

    let result = test.call(&mut ctx, 2, 3, 4, 5, 6)?;
    let manually_computed_result = 6 * (5 * (4 * (3 * 2 * 20) * 10 * 20)) * 10;
    assert_eq!(result, manually_computed_result);
    Ok(())
}

#[compiler_test(native_functions)]
fn native_function_works_for_wasm_function_manyparams(config: crate::Config) -> anyhow::Result<()> {
    let store = config.store();
    let wat = r#"(module
        (func $longf (import "env" "longf") (param i32 i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i64))
        (func (export "longf_pure") (param i32 i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i64)
           (call $longf (local.get 0) (local.get 1) (local.get 2) (local.get 3) (local.get 4) (local.get 5) (local.get 6) (local.get 7) (local.get 8) (local.get 9)))
        (func (export "longf") (result i64)
           (call $longf (i32.const 1) (i32.const 2) (i32.const 3) (i32.const 4) (i32.const 5) (i32.const 6) (i64.const 7) (i64.const 8) (i32.const 9) (i32.const 0)))
)"#;
    let module = Module::new(&store, wat).unwrap();
    let mut ctx = WasmerContext::new(&store, ());
    let import_object = imports! {
        "env" => {
            "longf" => Function::new_native(&mut ctx, long_f),
        },
    };

    let instance = Instance::new(&mut ctx, &module, &import_object)?;

    {
        let dyn_f: &Function = instance.exports.get("longf")?;
        let f: TypedFunction<(), i64> = dyn_f.native(&mut ctx).unwrap();
        let result = f.call(&mut ctx)?;
        assert_eq!(result, 1234567890);
    }

    {
        let dyn_f: &Function = instance.exports.get("longf_pure")?;
        let f: TypedFunction<(u32, u32, u32, u32, u32, u16, u64, u64, u16, u32), i64> =
            dyn_f.native(&mut ctx).unwrap();
        let result = f.call(&mut ctx, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0)?;
        assert_eq!(result, 1234567890);
    }

    Ok(())
}

#[compiler_test(native_functions)]
fn native_function_works_for_wasm_function_manyparams_dynamic(
    config: crate::Config,
) -> anyhow::Result<()> {
    let store = config.store();
    let mut ctx = WasmerContext::new(&store, ());
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
            "longf" => Function::new(&mut ctx, FunctionType::new(vec![ValueType::I32, ValueType::I32, ValueType::I32, ValueType::I32, ValueType::I32, ValueType::I32, ValueType::I64 , ValueType::I64 ,ValueType::I32, ValueType::I32], vec![ValueType::I64]), long_f_dynamic),
        },
    };

    let instance = Instance::new(&mut ctx, &module, &import_object)?;

    {
        let dyn_f: &Function = instance.exports.get("longf")?;
        let f: TypedFunction<(), i64> = dyn_f.native(&mut ctx).unwrap();
        let result = f.call(&mut ctx)?;
        assert_eq!(result, 1234567890);
    }

    {
        let dyn_f: &Function = instance.exports.get("longf_pure")?;
        let f: TypedFunction<(u32, u32, u32, u32, u32, u16, u64, u64, u16, u32), i64> =
            dyn_f.native(&mut ctx).unwrap();
        let result = f.call(&mut ctx, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0)?;
        assert_eq!(result, 1234567890);
    }

    Ok(())
}

#[compiler_test(native_functions)]
fn static_host_function_without_env(config: crate::Config) -> anyhow::Result<()> {
    let store = config.store();
    let mut ctx = WasmerContext::new(&store, ());

    fn f(_ctx: ContextMut<()>, a: i32, b: i64, c: f32, d: f64) -> (f64, f32, i64, i32) {
        (d * 4.0, c * 3.0, b * 2, a * 1)
    }

    fn f_ok(
        _ctx: ContextMut<()>,
        a: i32,
        b: i64,
        c: f32,
        d: f64,
    ) -> Result<(f64, f32, i64, i32), Infallible> {
        Ok((d * 4.0, c * 3.0, b * 2, a * 1))
    }

    fn long_f(
        _ctx: ContextMut<()>,
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
        let f = Function::new_native(&mut ctx, f);
        let f_native: TypedFunction<(i32, i64, f32, f64), (f64, f32, i64, i32)> =
            f.native(&mut ctx).unwrap();
        let result = f_native.call(&mut ctx, 1, 3, 5.0, 7.0)?;
        assert_eq!(result, (28.0, 15.0, 6, 1));
    }

    // Native static host function that returns a tuple.
    {
        let long_f = Function::new_native(&mut ctx, long_f);
        let long_f_native: TypedFunction<
            (u32, u32, u32, u32, u32, u16, u64, u64, u16, u32),
            (u32, u64, u32),
        > = long_f.native(&mut ctx).unwrap();
        let result = long_f_native.call(&mut ctx, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0)?;
        assert_eq!(result, (654321, 87, 09));
    }

    // Native static host function that returns a result of a tuple.
    {
        let f = Function::new_native(&mut ctx, f_ok);
        let f_native: TypedFunction<(i32, i64, f32, f64), (f64, f32, i64, i32)> =
            f.native(&mut ctx).unwrap();
        let result = f_native.call(&mut ctx, 1, 3, 5.0, 7.0)?;
        assert_eq!(result, (28.0, 15.0, 6, 1));
    }

    Ok(())
}

#[compiler_test(native_functions)]
fn static_host_function_with_env(config: crate::Config) -> anyhow::Result<()> {
    let store = config.store();

    fn f(mut ctx: ContextMut<Env>, a: i32, b: i64, c: f32, d: f64) -> (f64, f32, i64, i32) {
        let mut guard = ctx.data_mut().0.lock().unwrap();
        assert_eq!(*guard, 100);
        *guard = 101;

        (d * 4.0, c * 3.0, b * 2, a * 1)
    }

    fn f_ok(
        mut ctx: ContextMut<Env>,
        a: i32,
        b: i64,
        c: f32,
        d: f64,
    ) -> Result<(f64, f32, i64, i32), Infallible> {
        let mut guard = ctx.data_mut().0.lock().unwrap();
        assert_eq!(*guard, 100);
        *guard = 101;

        Ok((d * 4.0, c * 3.0, b * 2, a * 1))
    }

    #[derive(Clone)]
    struct Env(Arc<Mutex<i32>>);

    impl std::ops::Deref for Env {
        type Target = Arc<Mutex<i32>>;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    // Native static host function that returns a tuple.
    {
        let env = Env(Arc::new(Mutex::new(100)));
        let mut ctx = WasmerContext::new(&store, env);

        let f = Function::new_native(&mut ctx, f);
        let f_native: TypedFunction<(i32, i64, f32, f64), (f64, f32, i64, i32)> =
            f.native(&mut ctx).unwrap();

        assert_eq!(*ctx.data_mut().0.lock().unwrap(), 100);

        let result = f_native.call(&mut ctx, 1, 3, 5.0, 7.0)?;

        assert_eq!(result, (28.0, 15.0, 6, 1));
        assert_eq!(*ctx.data_mut().0.lock().unwrap(), 101);
    }

    // Native static host function that returns a result of a tuple.
    {
        let env = Env(Arc::new(Mutex::new(100)));
        let mut ctx = WasmerContext::new(&store, env);

        let f = Function::new_native(&mut ctx, f_ok);
        let f_native: TypedFunction<(i32, i64, f32, f64), (f64, f32, i64, i32)> =
            f.native(&mut ctx).unwrap();

        assert_eq!(*ctx.data_mut().0.lock().unwrap(), 100);

        let result = f_native.call(&mut ctx, 1, 3, 5.0, 7.0)?;

        assert_eq!(result, (28.0, 15.0, 6, 1));
        assert_eq!(*ctx.data_mut().0.lock().unwrap(), 101);
    }

    Ok(())
}

#[compiler_test(native_functions)]
fn dynamic_host_function_without_env(config: crate::Config) -> anyhow::Result<()> {
    let store = config.store();
    let mut ctx = WasmerContext::new(&store, ());
    let f = Function::new(
        &mut ctx,
        FunctionType::new(
            vec![
                ValueType::I32,
                ValueType::I64,
                ValueType::F32,
                ValueType::F64,
            ],
            vec![
                ValueType::F64,
                ValueType::F32,
                ValueType::I64,
                ValueType::I32,
            ],
        ),
        |_ctx: ContextMut<_>, values| {
            Ok(vec![
                Value::F64(values[3].unwrap_f64() * 4.0),
                Value::F32(values[2].unwrap_f32() * 3.0),
                Value::I64(values[1].unwrap_i64() * 2),
                Value::I32(values[0].unwrap_i32() * 1),
            ])
        },
    );
    let f_native: TypedFunction<(i32, i64, f32, f64), (f64, f32, i64, i32)> =
        f.native(&mut ctx).unwrap();
    let result = f_native.call(&mut ctx, 1, 3, 5.0, 7.0)?;

    assert_eq!(result, (28.0, 15.0, 6, 1));

    Ok(())
}

#[compiler_test(native_functions)]
fn dynamic_host_function_with_env(config: crate::Config) -> anyhow::Result<()> {
    let store = config.store();

    #[derive(Clone)]
    struct Env(Arc<Mutex<i32>>);

    impl std::ops::Deref for Env {
        type Target = Arc<Mutex<i32>>;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    let env = Env(Arc::new(Mutex::new(100)));
    let mut ctx = WasmerContext::new(&store, env);
    let f = Function::new(
        &mut ctx,
        FunctionType::new(
            vec![
                ValueType::I32,
                ValueType::I64,
                ValueType::F32,
                ValueType::F64,
            ],
            vec![
                ValueType::F64,
                ValueType::F32,
                ValueType::I64,
                ValueType::I32,
            ],
        ),
        |mut ctx, values| {
            let mut guard = ctx.data_mut().0.lock().unwrap();
            assert_eq!(*guard, 100);

            *guard = 101;

            Ok(vec![
                Value::F64(values[3].unwrap_f64() * 4.0),
                Value::F32(values[2].unwrap_f32() * 3.0),
                Value::I64(values[1].unwrap_i64() * 2),
                Value::I32(values[0].unwrap_i32() * 1),
            ])
        },
    );

    let f_native: TypedFunction<(i32, i64, f32, f64), (f64, f32, i64, i32)> =
        f.native(&mut ctx).unwrap();

    assert_eq!(*ctx.data().0.lock().unwrap(), 100);

    let result = f_native.call(&mut ctx, 1, 3, 5.0, 7.0)?;

    assert_eq!(result, (28.0, 15.0, 6, 1));
    assert_eq!(*ctx.data().0.lock().unwrap(), 101);

    Ok(())
}
