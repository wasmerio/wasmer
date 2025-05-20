//! Defining an engine in Wasmer is one of the fundamental steps.
//!
//! As a reminder, an engine applies roughly 2 steps:
//!
//!   1. It compiles the Wasm module bytes to executable code, through
//!      the intervention of a compiler,
//!   2. It stores the executable code somewhere.
//!
//! This example focuses on the first step: the compiler. It
//! illustrates how the abstraction over the compiler is so powerful
//! that it is possible to cross-compile a Wasm module.
//!
//! You can run the example directly by executing in Wasmer root:
//!
//! ```shell
//! cargo run --example cross-compilation --release --features "cranelift"
//! ```
//!
//! Ready?

use std::str::FromStr;
use wasmer::{
    sys::{CpuFeature, EngineBuilder},
    wat2wasm, Module, RuntimeError, Store,
};
use wasmer_compiler_cranelift::Cranelift;
use wasmer_types::target::{Target, Triple};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Let's declare the Wasm module with the text representation.
    let wasm_bytes = wat2wasm(
        br#"
(module
  (type $sum_t (func (param i32 i32) (result i32)))
  (func $sum_f (type $sum_t) (param $x i32) (param $y i32) (result i32)
    local.get $x
    local.get $y
    i32.add)
  (export "sum" (func $sum_f)))
"#,
    )?;

    // Define a compiler configuration.
    //
    // In this situation, the compiler is
    // `wasmer_compiler_cranelift`. The compiler is responsible to
    // compile the Wasm module into executable code.
    let compiler_config = Cranelift::default();

    // Here we go.
    //
    // Let's define the target “triple”. Historically, such things had
    // three fields, though additional fields have been added over
    // time.
    let triple = Triple::from_str("x86_64-linux-musl")
        .map_err(|error| RuntimeError::new(error.to_string()))?;

    // Here we go again.
    //
    // Let's define a CPU feature.
    let mut cpu_feature = CpuFeature::set();
    cpu_feature.insert(CpuFeature::from_str("sse2")?);

    // Here we go finally.
    //
    // Let's build the target.
    let target = Target::new(triple, cpu_feature);
    println!("Chosen target: {:?}", target);

    // Define the engine that will drive everything.
    //
    // That's where we specify the target for the compiler.
    //
    // Use the Universal engine.
    let engine = EngineBuilder::new(compiler_config).set_target(Some(target));

    // Create a store, that holds the engine.
    let store = Store::new(engine);

    println!("Compiling module...");
    // Let's compile the Wasm module.
    let _module = Module::new(&store, wasm_bytes)?;

    println!("Module compiled successfully.");

    // Congrats, the Wasm module is cross-compiled!
    //
    // What to do with that? It is possible to use an engine (probably
    // a headless engine) to execute the cross-compiled Wasm module an
    // the targeted platform.

    Ok(())
}

#[test]
#[cfg(not(any(
    windows,
    // We don't support yet crosscompilation in macOS with Apple Silicon
    all(target_os = "macos", target_arch = "aarch64"),
    target_env = "musl",
)))]
fn test_cross_compilation() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
