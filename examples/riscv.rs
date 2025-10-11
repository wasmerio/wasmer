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

use wasmer::{Instance, Module, Store, Value, imports, wat2wasm};
use wasmer_compiler_singlepass::Singlepass;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wasm_bytes = wat2wasm(
        r#"
(module    
 (func (export "nested-br_table-value-index") (param i32) (result i32)
    (i32.add
      (i32.const 1)
      (block (result i32)
        (drop (i32.const 2))
        (br_table 0
          (i32.const 4)
          (block (result i32)
            (drop (br_if 1 (i32.const 8) (local.get 0))) (i32.const 1)
          )
        )
        (i32.const 16)
      )
    )
  )

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
    let func = instance
        .exports
        .get_function("nested-br_table-value-index")?;

    println!("Calling `fn` function...");
    //let result = sample.call(&mut store, &[Value::I32(123456)])?;
    //let result = sample.call(&mut store, &[])?;

    dbg!(func.call(&mut store, &[Value::I32(0)]));

    // for i in 0..16 {
    //     let result = sample2.call(&mut store, &[Value::I32(i)])?; //, &[Value::I32(1), Value::I32(123)])?;
    //     println!("Result {i} = {:?}", result);
    // }

    // let result = sample2.call(&mut store, &[Value::I32(14)])?; //, &[Value::I32(1), Value::I32(123)])?;

    Ok(())
}
