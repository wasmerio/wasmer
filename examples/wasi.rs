//! Running a WASI compiled WebAssembly module with Wasmer.
//!
//! This example illustrates how to run WASI modules with
//! Wasmer.
//!
//! If you need more manual control over the instantiation, including custom
//! imports, then check out the ./wasi_manual_setup.rs example.
//!
//! You can run the example directly by executing in Wasmer root:
//!
//! ```shell
//! cargo run --example wasi --release --features "cranelift,wasi"
//! ```
//!
//! Ready?

use std::io::Read;

use wasmer::{Module, Store};
use wasmer_wasix::{Pipe, WasiEnv};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/wasi-wast/wasi/unstable/hello.wasm"
    );
    // Let's declare the Wasm module with the text representation.
    let wasm_bytes = std::fs::read(wasm_path)?;

    // Create a Store.
    let mut store = Store::default();

    println!("Compiling module...");
    // Let's compile the Wasm module.
    let module = Module::new(&store, wasm_bytes)?;

    let (stdout_tx, mut stdout_rx) = Pipe::channel();

    // Run the module.
    WasiEnv::builder("hello")
        // .args(&["world"])
        // .env("KEY", "Value")
        .stdout(Box::new(stdout_tx))
        .run_with_store(module, &mut store)?;

    eprintln!("Run complete - reading output");

    let mut buf = String::new();
    stdout_rx.read_to_string(&mut buf).unwrap();

    eprintln!("Output: {buf}");

    Ok(())
}

#[test]
#[cfg(feature = "wasi")]
fn test_wasi() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
