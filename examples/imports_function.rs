//! A Wasm module can import entities, like functions, memories,
//! globals and tables.
//!
//! This example illustrates how to use imported functions. They come
//! in 2 flavors:
//!
//!   1. Dynamic functions, where parameters and results are of a
//!      slice of `Value`,
//!   2. Native function, where parameters and results are statically
//!      typed Rust values.
//!
//! You can run the example directly by executing in Wasmer root:
//!
//! ```shell
//! cargo run --example imported-function --release --features "cranelift"
//! ```
//!
//! Ready?

use wasmer::{
    imports, wat2wasm, Function, FunctionEnv, FunctionEnvMut, FunctionType, Instance, Module,
    Store, Type, TypedFunction, Value,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Let's declare the Wasm module with the text representation.
    let wasm_bytes = wat2wasm(
        br#"
(module
  (func $multiply_dynamic (import "env" "multiply_dynamic") (param i32) (result i32))
  (func $multiply_typed (import "env" "multiply_typed") (param i32) (result i32))

  (type $sum_t (func (param i32) (param i32) (result i32)))
  (func $sum_f (type $sum_t) (param $x i32) (param $y i32) (result i32)
    (call $multiply_dynamic (local.get $x))
    (call $multiply_typed (local.get $y))
    i32.add)
  (export "sum" (func $sum_f)))
"#,
    )?;

    // Create a Store.
    let mut store = Store::default();

    struct MyEnv;
    let env = FunctionEnv::new(&mut store, MyEnv {});

    println!("Compiling module...");
    // Let's compile the Wasm module.
    let module = Module::new(&store, wasm_bytes)?;

    // Create the functions
    let multiply_dynamic_signature = FunctionType::new(vec![Type::I32], vec![Type::I32]);
    let multiply_dynamic = Function::new(&mut store, &multiply_dynamic_signature, |args| {
        println!("Calling `multiply_dynamic`...");

        let result = args[0].unwrap_i32() * 2;

        println!("Result of `multiply_dynamic`: {:?}", result);

        Ok(vec![Value::I32(result)])
    });

    fn multiply(_env: FunctionEnvMut<MyEnv>, a: i32) -> i32 {
        println!("Calling `multiply_typed`...");
        let result = a * 3;

        println!("Result of `multiply_typed`: {:?}", result);

        result
    }
    let multiply_typed = Function::new_typed_with_env(&mut store, &env, multiply);

    // Create an import object.
    let import_object = imports! {
        "env" => {
            "multiply_dynamic" => multiply_dynamic,
            "multiply_typed" => multiply_typed,
        }
    };

    println!("Instantiating module...");
    // Let's instantiate the Wasm module.
    let instance = Instance::new(&mut store, &module, &import_object)?;

    // Here we go.
    //
    // The Wasm module exports a function called `sum`. Let's get it.
    let sum: TypedFunction<(i32, i32), i32> =
        instance.exports.get_function("sum")?.typed(&mut store)?;

    println!("Calling `sum` function...");
    // Let's call the `sum` exported function. It will call each
    // of the imported functions.
    let result = sum.call(&mut store, 1, 2)?;

    println!("Results of `sum`: {:?}", result);
    assert_eq!(result, 8);

    Ok(())
}

#[test]
fn test_exported_function() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
