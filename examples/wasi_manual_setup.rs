//! Running a WASI compiled WebAssembly module with Wasmer.
//!
//! This example illustrates how to run WASI modules with
//! Wasmer. To run WASI we have to have to do mainly 3 steps:
//!
//!   1. Create a `WasiEnv` instance
//!   2. Attach the imports from the `WasiEnv` to a new instance
//!   3. Run the `WASI` module.
//!
//! You can run the example directly by executing in Wasmer root:
//!
//! ```shell
//! cargo run --example wasi-manual-setup --release --features "cranelift,tokio,wasi"
//! ```
//!
//! Ready?

use wasmer::{Instance, Module, Store, wat2wasm};
use wasmer_wasix::WasiEnv;

const HELLO_WASI_WAT: &[u8] = br#"
(module
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "\10\00\00\00\0e\00\00\00")
  (data (i32.const 16) "Hello, world!\0a")
  (func (export "_start")
    i32.const 1
    i32.const 0
    i32.const 1
    i32.const 8
    call $fd_write
    drop))
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wasm_bytes = wat2wasm(HELLO_WASI_WAT)?;

    // Create a Store.
    let mut store = Store::default();

    println!("Compiling module...");
    // Let's compile the Wasm module.
    let module = Module::new(&store, wasm_bytes)?;

    println!("Starting `tokio` runtime...");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = runtime.enter();

    println!("Creating `WasiEnv`...");
    // First, we create the `WasiEnv`
    let mut wasi_env = WasiEnv::builder("hello")
        // .args(&["world"])
        // .env("KEY", "Value")
        .engine(store.engine().clone())
        .finalize(&mut store)?;

    println!("Instantiating module with WASI imports...");
    // Then, we get the import object related to our WASI
    // and attach it to the Wasm instance.
    let import_object = wasi_env.import_object(&mut store, &module)?;
    let instance = Instance::new(&mut store, &module, &import_object)?;

    println!("Attach WASI memory...");
    // // Attach the memory export
    // let memory = instance.exports.get_memory("memory")?;
    // wasi_env.data_mut(&mut store).set_memory(memory.clone());

    wasi_env.initialize(&mut store, instance.clone())?;

    println!("Call WASI `_start` function...");
    // And we just call the `_start` function!
    let start = instance.exports.get_function("_start")?;
    start.call(&mut store, &[])?;

    wasi_env.on_exit(&mut store, None);

    Ok(())
}

#[test]
#[cfg(feature = "wasi")]
fn test_wasi() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
