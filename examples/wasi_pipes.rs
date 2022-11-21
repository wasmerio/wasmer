//! Piping to and from a WASI compiled WebAssembly module with Wasmer.
//!
//! This example builds on the WASI example, showing how you can pipe to and
//! from a WebAssembly module.
//!
//! You can run the example directly by executing in Wasmer root:
//!
//! ```shell
//! cargo run --example wasi-pipes --release --features "cranelift,wasi"
//! ```
//!
//! Ready?

use std::io::{Read, Write};
use wasmer::{Instance, Module, Store};
use wasmer_compiler_cranelift::Cranelift;
use wasmer_wasi::{Pipe, WasiState};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/wasi-wast/wasi/unstable/pipe_reverse.wasm"
    );
    // Let's declare the Wasm module with the text representation.
    let wasm_bytes = std::fs::read(wasm_path)?;

    // Create a Store.
    // Note that we don't need to specify the engine/compiler if we want to use
    // the default provided by Wasmer.
    // You can use `Store::default()` for that.
    let mut store = Store::new(Cranelift::default());

    println!("Compiling module...");
    // Let's compile the Wasm module.
    let module = Module::new(&store, wasm_bytes)?;

    println!("Creating `WasiEnv`...");
    // First, we create the `WasiEnv` with the stdio pipes
    let mut input = Pipe::new();
    let mut output = Pipe::new();
    let wasi_env = WasiState::new("hello")
        .stdin(Box::new(input.clone()))
        .stdout(Box::new(output.clone()))
        .finalize(&mut store)?;

    println!("Instantiating module with WASI imports...");
    // Then, we get the import object related to our WASI
    // and attach it to the Wasm instance.
    let import_object = wasi_env.import_object(&mut store, &module)?;
    let instance = Instance::new(&mut store, &module, &import_object)?;

    println!("Attach WASI memory...");
    // Attach the memory export
    let memory = instance.exports.get_memory("memory")?;
    wasi_env.data_mut(&mut store).set_memory(memory.clone());

    let msg = "racecar go zoom";
    println!("Writing \"{}\" to the WASI stdin...", msg);
    // To write to the stdin
    writeln!(input, "{}", msg)?;

    println!("Call WASI `_start` function...");
    // And we just call the `_start` function!
    let start = instance.exports.get_function("_start")?;
    start.call(&mut store, &[])?;

    println!("Reading from the WASI stdout...");

    // To read from the stdout
    let mut buf = String::new();
    output.read_to_string(&mut buf)?;
    println!("Read \"{}\" from the WASI stdout!", buf.trim());

    Ok(())
}

#[test]
#[cfg(feature = "wasi")]
fn test_wasi() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
