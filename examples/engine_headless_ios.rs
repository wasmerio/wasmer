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
//! somewhere (e.g. in memory with the Universal engine, or in a
//! shared object file with the Dylib engine), the module can be
//! instantiated and executed. But imagine for a second the following
//! scenario:
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

use std::fs::File;
use std::path::Path;
use std::str::FromStr;
use tempfile::NamedTempFile;
use wasmer::imports;
use wasmer::wat2wasm;
use wasmer::Instance;
use wasmer::Module;
use wasmer::RuntimeError;
use wasmer::Store;
use wasmer::Value;
use wasmer_compiler::{CpuFeature, Target, Triple};
use wasmer_compiler_cranelift::Cranelift;
use wasmer_engine_dylib::Dylib;

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

        // Create a compiler for iOS
        let compiler_config = Cranelift::default();
        let triple = Triple::from_str("aarch64-apple-ios")
            .map_err(|error| RuntimeError::new(error.to_string()))?;

        // Let's build the target.
        let mut cpu_feature = CpuFeature::set();
        let target = Target::new(triple, cpu_feature);
        println!("Chosen target: {:?}", target);

        println!("Creating Dylib engine...");
        let engine = Dylib::new(compiler_config).target(target).engine();

        // Create a store, that holds the engine.
        let store = Store::new(&engine);

        println!("Compiling module...");
        // Let's compile the Wasm module.
        let module = Module::new(&store, wasm_bytes)?;
        // Here we go. Let's serialize the compiled Wasm module in a
        // file.
        println!("Serializing module...");
        let mut dylib_file = Path::new("./sum.dylib");
        module.serialize_to_file(dylib_file)?;
        dylib_file
    };

    Ok(())
}

#[test]
#[cfg(not(any(windows, target_env = "musl")))]
fn test_engine_headless_ios() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
