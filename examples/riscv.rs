//! A Wasm module can be compiled with multiple compilers.
//!
//! This example illustrates how to use RISC-V with the singlepass compiler.
//!
//! You can run the example directly by executing in Wasmer root:
//!
//! ```shell
//! cargo run --example riscv --release --features "singlepass"
//! ```
//!
//! Ready?

use std::iter;

use wasmer::{imports, wat2wasm, Instance, Module, Store, TypedFunction, Value};
use wasmer_compiler_singlepass::Singlepass;

fn gen_wat_add_function(arguments: usize) -> String {
    assert!(arguments > 0);
    let arg_types: Vec<_> = iter::repeat("i64").take(arguments).collect();
    let params: Vec<_> = (0..arguments)
        .map(|idx| format!("(param $p{} i64)", idx + 1))
        .collect();
    let fn_body: Vec<_> = (2..=arguments)
        .map(|idx| format!("local.get $p{idx}\ni64.add"))
        .collect();

    format!(
        r#"
    (module
    (type $sum_t (func (param {}) (result i64)))
    (func $sum_f (type $sum_t)
    {}
    (result i64)
    local.get $p1
    {}
    )
    (export "sum" (func $sum_f)))
    "#,
        arg_types.join(" "),
        params.join(" "),
        fn_body.join("\n")
    )
}

fn test_sum_generated() -> Result<(), Box<dyn std::error::Error>> {
    for params in 1..200 {
        let wat_body = gen_wat_add_function(params as usize);
        let wasm_bytes = wat2wasm(wat_body.as_bytes())?;

        let compiler = Singlepass::default();
        let mut store = Store::new(compiler);

        let module = Module::new(&store, wasm_bytes)?;

        // Create an empty import object.
        let import_object = imports! {};

        let instance = Instance::new(&mut store, &module, &import_object)?;
        let sum = instance.exports.get_function("sum")?;

        print!("Calling `sum` function for {params} arguments...");

        let args: Vec<_> = (1..=params).map(|value| Value::I64(value)).collect();
        let result = sum.call(&mut store, &args)?;
        println!("results: {:?}", result);
        assert_eq!(result.to_vec(), vec![Value::I64((1..=params).sum())]);
    }

    Ok(())
}

fn test_simple_sum_int() -> Result<(), Box<dyn std::error::Error>> {
    let wasm_bytes = wat2wasm(
        r#"
    (module
    (type $sum_t (func (param i64 i64 i64) (result i64)))
    (func $sum_f (type $sum_t)
        (param $p1 i64)
        (param $p2 i64)
        (param $p3 i64)
        (result i64)
    local.get $p1
    local.get $p2
    i64.add
    local.get $p3
    i64.add)
    (export "sum" (func $sum_f)))
    "#
        .as_bytes(),
    )?;

    let compiler = Singlepass::default();
    let mut store = Store::new(compiler);

    println!("Compiling module...");
    let module = Module::new(&store, wasm_bytes)?;

    // Create an empty import object.
    let import_object = imports! {};

    println!("Instantiating module...");
    let instance = Instance::new(&mut store, &module, &import_object)?;
    let sum = instance.exports.get_function("sum")?;

    // Option 1
    println!("Calling `sum` function...");
    let args = [Value::I64(1), Value::I64(10), Value::I64(100)];
    let result = sum.call(&mut store, &args)?;
    println!("Results: {:?}", result);
    assert_eq!(result.to_vec(), vec![Value::I64(111)]);

    // Option 2
    let sum_typed: TypedFunction<(i64, i64, i64), i64> = sum.typed(&mut store)?;
    println!("Calling `sum` function (natively)...");
    let result = sum_typed.call(&mut store, 1, 10, 100)?;
    println!("Results: {:?}", result);
    assert_eq!(result, 111);

    Ok(())
}

fn test_simple_sum_fp() -> Result<(), Box<dyn std::error::Error>> {
    let wasm_bytes = wat2wasm(
        r#"
    (module
    (type $sum_t (func (param f64 f64 f64) (result f64)))
    (func $sum_f (type $sum_t)
        (param $p1 f64)
        (param $p2 f64)
        (param $p3 f64)
        (result f64)
    local.get $p1
    local.get $p2
    f64.add
    local.get $p3
    f64.add)
    (export "sum" (func $sum_f)))
    "#
        .as_bytes(),
    )?;

    let compiler = Singlepass::default();
    let mut store = Store::new(compiler);

    println!("Compiling module...");
    let module = Module::new(&store, wasm_bytes)?;

    // Create an empty import object.
    let import_object = imports! {};

    println!("Instantiating module...");
    let instance = Instance::new(&mut store, &module, &import_object)?;
    let sum = instance.exports.get_function("sum")?;

    // Option 1
    println!("Calling `sum` function...");
    let args = [Value::F64(1.), Value::F64(10.), Value::F64(100.)];
    let result = sum.call(&mut store, &args)?;
    println!("Results: {:?}", result);
    assert_eq!(result.to_vec(), vec![Value::F64(111.)]);

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    test_simple_sum_fp()?;
    test_simple_sum_int()?;
    test_sum_generated()?;

    Ok(())
}
