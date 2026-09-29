//! Piping to and from a WASI compiled WebAssembly module with Wasmer.
//!
//! This example builds on the WASI example, showing how you can pipe to and
//! from a WebAssembly module.
//!
//! You can run the example directly by executing in Wasmer root:
//!
//! ```shell
//! cargo run --example wasi-pipes --release --features "cranelift,tokio,backend,wasi"
//! ```
//!
//! Ready?

use std::io::{Read, Write};

use wasmer::{Module, wat2wasm};
use wasmer_wasix::{
    Pipe,
    runners::wasi::{RuntimeOrEngine, WasiRunner},
};

const REVERSE_WASI_WAT: &[u8] = br#"
(module
  (import "wasi_snapshot_preview1" "fd_read"
    (func $fd_read (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "\20\00\00\00\60\00\00\00")
  (data (i32.const 8) "\80\00\00\00\00\00\00\00")
  (func (export "_start")
    (local $n i32)
    (local $i i32)
    i32.const 0
    i32.const 0
    i32.const 1
    i32.const 16
    call $fd_read
    drop
    i32.const 16
    i32.load
    local.set $n
    local.get $n
    i32.const 0
    i32.gt_u
    i32.const 31
    local.get $n
    i32.add
    i32.load8_u
    i32.const 10
    i32.eq
    i32.and
    if
      local.get $n
      i32.const 1
      i32.sub
      local.set $n
    end
    block $done
      loop $copy
        local.get $i
        local.get $n
        i32.ge_u
        br_if $done
        i32.const 128
        local.get $i
        i32.add
        i32.const 32
        local.get $n
        i32.add
        i32.const 1
        i32.sub
        local.get $i
        i32.sub
        i32.load8_u
        i32.store8
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $copy
      end
    end
    i32.const 12
    local.get $n
    i32.store
    i32.const 1
    i32.const 8
    i32.const 1
    i32.const 20
    call $fd_write
    drop))
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wasm_bytes = wat2wasm(REVERSE_WASI_WAT)?.into_owned();

    // We need at least an engine to be able to compile the module.
    let engine = wasmer::Engine::default();

    println!("Compiling module...");
    // Let's compile the Wasm module.
    let module = Module::new(&engine, &wasm_bytes[..])?;

    let msg = "racecar go zoom";
    println!("Writing \"{msg}\" to the WASI stdin...");
    let (mut stdin_sender, stdin_reader) = Pipe::channel();
    let (stdout_sender, mut stdout_reader) = Pipe::channel();

    // To write to the stdin
    writeln!(stdin_sender, "{msg}")?;

    {
        // Create a WASI runner. We use a scope to make sure the runner is dropped
        // as soon as we are done with it; otherwise, it will keep the stdout pipe
        // open.
        let mut runner = WasiRunner::new();

        // Configure the WasiRunner with the stdio pipes.
        runner
            .with_stdin(Box::new(stdin_reader))
            .with_stdout(Box::new(stdout_sender));

        // Now, run the module.
        println!("Running module...");
        runner.run_wasm(
            RuntimeOrEngine::Engine(engine),
            "hello",
            module,
            wasmer_types::ModuleHash::new(wasm_bytes),
        )?;
    }

    // To read from the stdout
    let mut buf = String::new();
    stdout_reader.read_to_string(&mut buf)?;
    println!("Read \"{}\" from the WASI stdout!", buf.trim());

    // Verify the module wrote the correct thing, for the test below
    assert_eq!(buf.trim(), "mooz og racecar");

    Ok(())
}

#[test]
#[cfg(feature = "wasi")]
fn test_wasi() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
