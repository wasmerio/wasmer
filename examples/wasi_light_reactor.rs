//! Example of using lightweight WASI for reactor-style usage
//!
//! This example demonstrates how to use the lightweight WASI implementation
//! for plugins or dynamic application components without the overhead of
//! filesystem, networking, or threading features.
//!
//! You can run the example directly by executing in Wasmer root:
//!
//! ```shell
//! cargo run --example wasi-light-reactor --release --features "cranelift"
//! ```

use std::collections::HashMap;
use wasmer::{Instance, Module, Store, wat2wasm};
use wasmer_wasi_light::{WasiLightEnv, generate_import_object};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a simple WASM module that uses WASI functions
    let wasm_bytes = wat2wasm(
        br#"
(module
  (import "wasi_snapshot_preview1" "args_get" (func $args_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "args_sizes_get" (func $args_sizes_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "environ_get" (func $environ_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "environ_sizes_get" (func $environ_sizes_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "clock_time_get" (func $clock_time_get (param i32 i64 i32) (result i32)))
  (import "wasi_snapshot_preview1" "random_get" (func $random_get (param i32 i32) (result i32)))
  
  (memory (export "memory") 1)
  
  (func $get_time (export "get_time") (result i64)
    (local $time_ptr i32)
    (local.set $time_ptr (i32.const 0))
    (call $clock_time_get (i32.const 0) (i64.const 0) (local.get $time_ptr))
    (i64.load (local.get $time_ptr))
  )
  
  (func $get_random (export "get_random") (param $len i32) (result i32)
    (local $buf_ptr i32)
    (local.set $buf_ptr (i32.const 100))
    (call $random_get (local.get $buf_ptr) (local.get $len))
    (local.get $buf_ptr)
  )
  
  (func $get_args_count (export "get_args_count") (result i32)
    (local $argc_ptr i32)
    (local $argv_buf_size_ptr i32)
    (local.set $argc_ptr (i32.const 0))
    (local.set $argv_buf_size_ptr (i32.const 4))
    (call $args_sizes_get (local.get $argc_ptr) (local.get $argv_buf_size_ptr))
    (i32.load (local.get $argc_ptr))
  )
  
  (func $get_env_count (export "get_env_count") (result i32)
    (local $environ_count_ptr i32)
    (local $environ_buf_size_ptr i32)
    (local.set $environ_count_ptr (i32.const 0))
    (local.set $environ_buf_size_ptr (i32.const 4))
    (call $environ_sizes_get (local.get $environ_count_ptr) (local.get $environ_buf_size_ptr))
    (i32.load (local.get $environ_count_ptr))
  )
)
"#,
    )?;

    // Create a Store
    let mut store = Store::default();

    println!("Compiling module...");
    // Compile the Wasm module
    let module = Module::new(&store, wasm_bytes)?;

    println!("Creating lightweight WASI environment...");
    // Create a lightweight WASI environment
    let wasi_env = WasiLightEnv::new()
        .args(vec!["arg1".to_string(), "arg2".to_string(), "arg3".to_string()])
        .envs(HashMap::from([
            ("KEY1".to_string(), "VALUE1".to_string()),
            ("KEY2".to_string(), "VALUE2".to_string()),
            ("RUST_BACKTRACE".to_string(), "1".to_string()),
        ]))
        .clock_offset(std::time::Duration::from_secs(3600)) // 1 hour offset
        .random_seed(42); // Deterministic random seed

    println!("Generating import object...");
    // Generate the import object
    let import_object = generate_import_object(&mut store, &wasi_env)?;

    println!("Instantiating module...");
    // Instantiate the module
    let instance = Instance::new(&mut store, &module, &import_object)?;

    println!("Calling exported functions...");
    
    // Test getting time
    let get_time: wasmer::TypedFunction<(), i64> = instance
        .exports
        .get_function("get_time")?
        .typed(&mut store)?;
    let time = get_time.call(&mut store)?;
    println!("Current time (with offset): {}", time);

    // Test getting random bytes
    let get_random: wasmer::TypedFunction<i32, i32> = instance
        .exports
        .get_function("get_random")?
        .typed(&mut store)?;
    let random_ptr = get_random.call(&mut store, 10)?;
    println!("Random bytes pointer: {}", random_ptr);

    // Test getting argument count
    let get_args_count: wasmer::TypedFunction<(), i32> = instance
        .exports
        .get_function("get_args_count")?
        .typed(&mut store)?;
    let args_count = get_args_count.call(&mut store)?;
    println!("Argument count: {}", args_count);

    // Test getting environment variable count
    let get_env_count: wasmer::TypedFunction<(), i32> = instance
        .exports
        .get_function("get_env_count")?
        .typed(&mut store)?;
    let env_count = get_env_count.call(&mut store)?;
    println!("Environment variable count: {}", env_count);

    println!("All tests passed! The lightweight WASI implementation is working correctly.");

    Ok(())
} 