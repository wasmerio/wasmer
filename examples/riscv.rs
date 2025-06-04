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
    (type $sum_t (func (param i32 i32) (result i32)))
    (func $sum_f (type $sum_t) (param $x i32) (param $y i32) (result i32)
    local.get $x
    local.get $y
    i32.add)
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
    let args = [Value::I32(1), Value::I32(2)];
    let result = sum.call(&mut store, &args)?;
    println!("Results: {:?}", result);
    assert_eq!(result.to_vec(), vec![Value::I32(3)]);

    // Option 2
    let sum_typed: TypedFunction<(i32, i32), i32> = sum.typed(&mut store)?;
    println!("Calling `sum` function (natively)...");
    let result = sum_typed.call(&mut store, 1, 2)?;
    println!("Results: {:?}", result);
    assert_eq!(result, 3);

    Ok(())
}
