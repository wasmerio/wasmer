//! Defining an engine in Wasmer is one of the fundamental steps.
//!
//! This example illustrates how to use the `wasmer_engine_dylib`,
//! aka the Dylib engine. An engine applies roughly 2 steps:
//!
//!   1. It compiles the Wasm module bytes to executable code, through
//!      the intervention of a compiler,
//!   2. It stores the executable code somewhere.
//!
//! In the particular context of the Dylib engine, the executable code
//! is stored in a shared object (`.dylib`, `.so` or `.dll` file).
//!
//! You can run the example directly by executing in Wasmer root:
//!
//! ```shell
//! cargo run --example engine-dylib --release --features "cranelift"
//! ```
//!
//! Ready?

use wasmer::{imports, wat2wasm, Instance, Module, Store, Value};
use wasmer_compiler_cranelift::Cranelift;
/*
use wasmer_engine_dylib::Dylib;
*/

fn main() -> Result<(), Box<dyn std::error::Error>> {
    /*
        // Let's declare the Wasm module with the text representation.
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

        // Define a compiler configuration.
        //
        // In this situation, the compiler is
        // `wasmer_compiler_cranelift`. The compiler is responsible to
        // compile the Wasm module into executable code.
        let compiler_config = Cranelift::default();

        println!("Creating Dylib engine...");
        // Define the engine that will drive everything.
        //
        // In this case, the engine is `wasmer_engine_dylib` which means
        // that a shared object is going to be generated.
        let engine = Dylib::new(compiler_config);

        // Create a store, that holds the engine.
        let mut store = Store::new(engine);

        println!("Compiling module...");
        // Here we go.
        //
        // Let's compile the Wasm module. It is at this step that the Wasm
        // text is transformed into Wasm bytes (if necessary), and then
        // compiled to executable code by the compiler, which is then
        // stored into a shared object by the engine.
        let module = Module::new(&store, wasm_bytes)?;

        // Congrats, the Wasm module is compiled! Now let's execute it for
        // the sake of having a complete example.

        // Create an import object. Since our Wasm module didn't declare
        // any imports, it's an empty object.
        let import_object = imports! {};

        println!("Instantiating module...");
        // And here we go again. Let's instantiate the Wasm module.
        let instance = Instance::new(&module, &import_object)?;

        println!("Calling `sum` function...");
        // The Wasm module exports a function called `sum`.
        let sum = instance.exports.get_function("sum")?;
        let results = sum.call(&[Value::I32(1), Value::I32(2)])?;

        println!("Results: {:?}", results);
        assert_eq!(results.to_vec(), vec![Value::I32(3)]);
        */

    Ok(())
}

#[test]
#[cfg(not(any(target_arch = "aarch64", target_env = "musl")))]
fn test_engine_dylib() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
