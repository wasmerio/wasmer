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

use wasmer::{Instance, Module, Store};
use wasmer_compiler_cranelift::Cranelift;
use wasmer_engine_universal::Universal;
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
    let store = Store::new(&Universal::new(Cranelift::default()).engine());

    println!("Compiling module...");
    // Let's compile the Wasm module.
    let module = Module::new(&store, wasm_bytes)?;

    println!("Creating `WasiEnv`...");
    // First, we create the `WasiEnv` with the stdio pipes
    let input = Pipe::new();
    let output = Pipe::new();
    let mut wasi_env = WasiState::new("hello")
        .stdin(Box::new(input))
        .stdout(Box::new(output))
        .finalize()?;

    println!("Instantiating module with WASI imports...");
    // Then, we get the import object related to our WASI
    // and attach it to the Wasm instance.
    let import_object = wasi_env.import_object(&module)?;
    let instance = Instance::new(&module, &import_object)?;

    let msg = "racecar go zoom";
    println!("Writing \"{}\" to the WASI stdin...", msg);
    // To write to the stdin, we need a mutable reference to the pipe
    //
    // We access WasiState in a nested scope to ensure we're not holding
    // the mutex after we need it.
    {
        let mut state = wasi_env.state();
        let wasi_stdin = state.fs.stdin_mut()?.as_mut().unwrap();
        // Then we can write to it!
        writeln!(wasi_stdin, "{}", msg)?;
    }

    println!("Call WASI `_start` function...");
    // And we just call the `_start` function!
    let start = instance.exports.get_function("_start")?;
    start.call(&[])?;

    println!("Reading from the WASI stdout...");
    // To read from the stdout, we again need a mutable reference to the pipe
    let mut state = wasi_env.state();
    let wasi_stdout = state.fs.stdout_mut()?.as_mut().unwrap();
    // Then we can read from it!
    let mut buf = String::new();
    wasi_stdout.read_to_string(&mut buf)?;
    println!("Read \"{}\" from the WASI stdout!", buf.trim());

    Ok(())
}

#[test]
#[cfg(feature = "wasi")]
fn test_wasi() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
