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

use wasmer::{imports, wat2wasm, Instance, Module, Store, Value};
use wasmer_compiler_singlepass::Singlepass;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wasm_bytes = wat2wasm(
        r#"
(module
  (func (export "nearest") (param $x f64) (result f64) (f64.nearest (local.get $x)))
)
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
    let func = instance.exports.get_function("nearest")?;

    println!("Calling `fn` function...");
    //let result = sample.call(&mut store, &[Value::I32(123456)])?;
    //let result = sample.call(&mut store, &[])?;
    let result = func.call(&mut store, &[Value::F64(0.49999)])?;
    let ret = result[0].unwrap_f64();
    println!("Result 0x{} {}", ret, ret);

    // for i in 0..16 {
    //     let result = sample2.call(&mut store, &[Value::I32(i)])?; //, &[Value::I32(1), Value::I32(123)])?;
    //     println!("Result {i} = {:?}", result);
    // }

    // let result = sample2.call(&mut store, &[Value::I32(14)])?; //, &[Value::I32(1), Value::I32(123)])?;
    println!("Result {:?}", result);

    Ok(())
}
