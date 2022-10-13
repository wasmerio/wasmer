//! Defining an engine in Wasmer is one of the fundamental steps.
//!
//! This example illustrates a neat feature of engines: their ability
//! to run in a headless mode. At the time of writing, all engines
//! have a headless mode, but it's not a requirement of the `Engine`
//! trait (defined in the `wasmer_engine` crate).
//!
//! What problem does it solve, and what does it mean?
//!
//! Once a Wasm module is compiled into executable code and stored
//! somewhere (e.g. in memory with the Universal engine), the module
//! can be instantiated and executed. But imagine for a second the
//! following scenario:
//!
//!   * Modules are compiled ahead of time, to be instantiated later
//!     on.
//!   * Modules are cross-compiled on a machine ahead of time
//!     to be run on another machine later one.
//!
//! In both scenarios, the environment where the compiled Wasm module
//! will be executed can be very constrained. For such particular
//! contexts, Wasmer can be compiled _without_ the compilers, so that
//! the `wasmer` binary is as small as possible. Indeed, there is no
//! need for a compiler since the Wasm module is already compiled. All
//! we need is an engine that _only_ drives the instantiation and
//! execution of the Wasm module.
//!
//! And that, that's a headless engine.
//!
//! To achieve such a scenario, a Wasm module must be compiled, then
//! serialized —for example into a file—, then later, potentially on
//! another machine, deserialized. The next steps are classical: The
//! Wasm module is instantiated and executed.
//!
//! This example uses a `compiler` because it illustrates the entire
//! workflow, but keep in mind the compiler isn't required after the
//! compilation step.
//!
//! You can run the example directly by executing in Wasmer root:
//!
//! ```shell
//! cargo run --example engine-headless --release --features "cranelift"
//! ```
//!
//! Ready?

use tempfile::NamedTempFile;
use wasmer::{imports, wat2wasm, EngineBuilder, Instance, Module, Store, Value};
use wasmer_compiler_cranelift::Cranelift;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // First step, let's compile the Wasm module and serialize it.
    // Note: we need a compiler here.
    let serialized_module_file = {
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
        let compiler = Cranelift::default();

        // Create a store, that holds the engine.
        let store = Store::new(compiler);

        println!("Compiling module...");
        // Let's compile the Wasm module.
        let module = Module::new(&store, wasm_bytes)?;

        println!("Serializing module...");
        // Here we go. Let's serialize the compiled Wasm module in a
        // file.
        let serialized_module_file = NamedTempFile::new()?;
        module.serialize_to_file(&serialized_module_file)?;

        serialized_module_file
    };

    // Second step, deserialize the compiled Wasm module, and execute
    // it, for example with Wasmer without a compiler.
    {
        println!("Creating headless Universal engine...");
        // We create a headless Universal engine.
        let engine = EngineBuilder::headless();
        let mut store = Store::new(engine);

        println!("Deserializing module...");
        // Here we go.
        //
        // Deserialize the compiled Wasm module. This code is unsafe
        // because Wasmer can't assert the bytes are valid (see the
        // `wasmer::Module::deserialize`'s documentation to learn
        // more).
        let module = unsafe { Module::deserialize_from_file(&store, serialized_module_file) }?;

        // Congrats, the Wasm module has been deserialized! Now let's
        // execute it for the sake of having a complete example.

        // Create an import object. Since our Wasm module didn't declare
        // any imports, it's an empty object.
        let import_object = imports! {};

        println!("Instantiating module...");
        // Let's instantiate the Wasm module.
        let instance = Instance::new(&mut store, &module, &import_object)?;

        println!("Calling `sum` function...");
        // The Wasm module exports a function called `sum`.
        let sum = instance.exports.get_function("sum")?;
        let results = sum.call(&mut store, &[Value::I32(1), Value::I32(2)])?;

        println!("Results: {:?}", results);
        assert_eq!(results.to_vec(), vec![Value::I32(3)]);
    }

    Ok(())
}

#[test]
#[cfg(not(any(windows, target_arch = "aarch64", target_env = "musl")))]
fn test_engine_headless() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
