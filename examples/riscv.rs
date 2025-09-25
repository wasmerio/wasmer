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
  (memory 1 1 shared)

  (func (export "init") (param $value i64) (i64.store (i32.const 0) (local.get $value)))
  (func (export "i64.load") (result i64) (i64.load (i32.const 0)))

  (func (export "i32.atomic.rmw8.cmpxchg_u") (param $expected i32)  (param $value i32) (result i32) (i32.atomic.rmw8.cmpxchg_u (i32.const 7) (local.get $expected) (local.get $value)))
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
    let func = instance.exports.get_function("init")?;
    let func2 = instance.exports.get_function("i32.atomic.rmw8.cmpxchg_u")?;
    let func3 = instance.exports.get_function("i64.load")?;

    println!("Calling `fn` function...");
    //let result = sample.call(&mut store, &[Value::I32(123456)])?;
    //let result = sample.call(&mut store, &[])?;

    func.call(&mut store, &[Value::I64(0x1702030405060708)])?;

    let result = func3.call(&mut store, &[])?;
    let ret = result[0].unwrap_i64();
    println!("Result 0x{:x}", ret);

    let result = func2.call(&mut store, &[Value::I32(0x17), Value::I32(0xcc)])?;
    let ret = result[0].unwrap_i32();
    println!("Result 0x{:x}", ret);

    let result = func3.call(&mut store, &[])?;
    let ret = result[0].unwrap_i64();
    println!("Result 0x{:x}", ret);

    // for i in 0..16 {
    //     let result = sample2.call(&mut store, &[Value::I32(i)])?; //, &[Value::I32(1), Value::I32(123)])?;
    //     println!("Result {i} = {:?}", result);
    // }

    // let result = sample2.call(&mut store, &[Value::I32(14)])?; //, &[Value::I32(1), Value::I32(123)])?;

    Ok(())
}
