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
use wasmer::{imports, wat2wasm, Instance, Module, Store, TypedFunction, Value};
use wasmer_compiler_singlepass::Singlepass;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wasm_bytes = wat2wasm(
        r#"
    (module
    (type $sum_t (func (param i64 i64 i64) (result i64)))
    (func $sum_f (type $sum_t) (param $x i64) (param $y i64) (param $z i64) (result i64)
    local.get $x
    local.get $y
    i64.add
    local.get $z
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
    let result = sum_typed.call(&mut store, 1, 2, 3)?;
    println!("Results: {:?}", result);
    assert_eq!(result, 6);

    Ok(())
}
