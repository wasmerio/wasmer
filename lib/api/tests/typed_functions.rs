use macro_wasmer_universal_test::universal_test;
use wasmer::*;

#[cfg(feature = "js")]
use wasm_bindgen_test::wasm_bindgen_test;

#[universal_test]
#[cfg_attr(
    feature = "js",
    ignore = "Closures with context are not supported in JS yet"
)]
fn typed_host_function_closure_panics() -> Result<(), String> {
    let mut store = Store::default();
    let state = 3;

    Function::new_typed(&mut store, move |_: i32| {
        println!("{state}");
    });

    Ok(())
}

#[universal_test]
#[cfg_attr(
    feature = "js",
    ignore = "Closures with context are not supported in JS yet"
)]
fn typed_with_env_host_function_closure_panics() -> Result<(), String> {
    let mut store = Store::default();
    let env: i32 = 4;
    let env = FunctionEnv::new(&mut store, env);
    let state = 3;
    Function::new_typed_with_env(
        &mut store,
        &env,
        move |_env: FunctionEnvMut<i32>, _: i32| {
            println!("{state}");
        },
    );

    Ok(())
}

#[universal_test]
#[cfg_attr(
    feature = "js",
    ignore = "Closures with context are not supported in JS yet"
)]
fn non_typed_functions_and_closures_with_no_env_work() -> anyhow::Result<()> {
    let mut store = Store::default();
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
    let env = FunctionEnv::new(&mut store, env);
    let ty = FunctionType::new(vec![Type::I32, Type::I32], vec![Type::I32]);
    let captured_by_closure = 20;
    let import_object = imports! {
        "env" => {
            "multiply1" => Function::new_with_env(&mut store, &env, &ty, move |_env, args| {
                if let (Value::I32(v1), Value::I32(v2)) = (&args[0], &args[1]) {
                    Ok(vec![Value::I32(v1 * v2 * captured_by_closure)])
                } else {
                    panic!("Invalid arguments");
                }
            }),
            "multiply2" => Function::new_with_env(&mut store, &env, &ty, move |env, args| {
                if let (Value::I32(v1), Value::I32(v2)) = (&args[0], &args[1]) {
                    Ok(vec![Value::I32(v1 * v2 * captured_by_closure * env.data())])
                } else {
                    panic!("Invalid arguments");
                }
            }),
            "multiply3" => Function::new_typed_with_env(&mut store, &env, |_env: FunctionEnvMut<_>, arg1: i32, arg2: i32| -> i32
                                                {arg1 * arg2 }),
            "multiply4" => Function::new_typed_with_env(&mut store, &env, |env: FunctionEnvMut<i32>, arg1: i32, arg2: i32| -> i32
                                                         {arg1 * arg2 * env.data() }),
        },
    };

    let instance = Instance::new(&mut store, &module, &import_object)?;

    let test: TypedFunction<(i32, i32, i32, i32, i32), i32> =
        instance.exports.get_typed_function(&mut store, "test")?;

    let result = test.call(&mut store, 2, 3, 4, 5, 6)?;
    let manually_computed_result = 6 * (5 * (4 * (3 * 2 * 20) * 10 * 20)) * 10;
    assert_eq!(result, manually_computed_result);
    Ok(())
}

static STATIC_CONTEXT_VAL: i32 = 1234;

#[universal_test]
#[cfg_attr(
    feature = "js",
    ignore = "Closures with context are not supported in JS yet"
)]
fn holochain_typed_function() -> anyhow::Result<()> {
    // Declare WASM Module
    let wasm_bytes = wat2wasm(
        br#"
(module
  (func $multiply_typed 
      (import "env" "multiply_typed") 
      (param i32) 
      (result i32)
  )
  (type $sum_t 
    (func (param i32) (param i32) (result i32))
  )
  (func $sum_f (type $sum_t) (param $x i32) (param $y i32) (result i32)
    (call $multiply_typed 
      (local.get $y)
    )
  )
(export "sum" (func $sum_f)))
"#,
    )?;
    let mut store = Store::default();
    struct MyEnv {}
    let env = FunctionEnv::new(&mut store, MyEnv {});
    let module = Module::new(&store, wasm_bytes)?;

    // Define some context data that the host function closure will use
    static STATIC_CONTEXT_VAL2: i32 = 1234;
    let context_val = 1234;
    fn my_val() -> i32 {
        1234
    }

    // Define the host function closure
    let multiply_by_3 = move |_env: FunctionEnvMut<MyEnv>, a: i32| -> i32 {
        assert_eq!(STATIC_CONTEXT_VAL, 1234);
        assert_eq!(STATIC_CONTEXT_VAL2, 1234);
        assert_eq!(context_val, 1234);
        assert_eq!(my_val(), 1234);

        a * 3
    };

    // Define the host function and WASM instance
    let multiply_typed = Function::new_typed_with_env(&mut store, &env, multiply_by_3);
    let import_object = imports! {
        "env" => {
            "multiply_typed" => multiply_typed,
        }
    };
    let instance = Instance::new(&mut store, &module, &import_object)?;

    // Execute the WASM function 'sum'
    let sum: TypedFunction<(i32, i32), i32> =
        instance.exports.get_function("sum")?.typed(&mut store)?;
    let result = sum.call(&mut store, 1, 2)?;
    assert_eq!(result, 6);

    Ok(())
}
