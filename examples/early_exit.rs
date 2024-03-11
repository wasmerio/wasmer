//! There are cases where you may want to interrupt this synchronous execution of the Wasm module
//! while the it is calling a host function. This can be useful for saving resources, and not
//! returning back to the guest Wasm for execution, when you already know the Wasm execution will
//! fail, or no longer be needed.
//!
//! In this example, we will run a Wasm module that calls the imported host function
//! interrupt_execution. This host function will immediately stop executing the WebAssembly module.
//!
//! You can run the example directly by executing in Wasmer root:
//!
//! ```shell
//! cargo run --example early-exit --release --features "cranelift"
//! ```
//!
//! Ready?

use anyhow::bail;
use std::fmt;
use wasmer::{imports, wat2wasm, Function, Instance, Module, Store, TypedFunction};

// First we need to create an error type that we'll use to signal the end of execution.
#[derive(Debug, Clone, Copy)]
struct ExitCode(u32);

// This type must implement `std::error::Error` so we must also implement `std::fmt::Display` for it.
impl fmt::Display for ExitCode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// And then we implement `std::error::Error`.
impl std::error::Error for ExitCode {}

fn main() -> anyhow::Result<()> {
    // Let's declare the Wasm module with the text representation.
    let wasm_bytes = wat2wasm(
        br#"
(module
  (type $run_t (func (param i32 i32) (result i32)))
  (type $early_exit_t (func (param) (result)))
  (import "env" "early_exit" (func $early_exit (type $early_exit_t)))
  (func $run (type $run_t) (param $x i32) (param $y i32) (result i32)
    (call $early_exit)
    (i32.add
        local.get $x
        local.get $y))
  (export "run" (func $run)))
"#,
    )?;

    // Create a Store.
    let mut store = Store::default();

    println!("Compiling module...");
    // Let's compile the Wasm module.
    let module = Module::new(&store, wasm_bytes)?;

    // We declare the host function that we'll use to terminate execution.
    fn early_exit() -> Result<(), ExitCode> {
        // This is where it happens.
        Err(ExitCode(1))
    }

    // Create an import object.
    let import_object = imports! {
        "env" => {
            "early_exit" => Function::new_typed(&mut store, early_exit),
        }
    };

    println!("Instantiating module...");
    // Let's instantiate the Wasm module.
    let instance = Instance::new(&mut store, &module, &import_object)?;

    // Here we go.
    //
    // Get the `run` function which we'll use as our entrypoint.
    println!("Calling `run` function...");
    let run_func: TypedFunction<(i32, i32), i32> =
        instance.exports.get_typed_function(&mut store, "run")?;

    // When we call a function it can either succeed or fail. We expect it to fail.
    match run_func.call(&mut store, 1, 7) {
        Ok(result) => {
            bail!(
                "Expected early termination with `ExitCode`, found: {}",
                result
            );
        }
        // In case of a failure, which we expect, we attempt to downcast the error into the error
        // type that we were expecting.
        Err(e) => match e.downcast::<ExitCode>() {
            // We found the exit code used to terminate execution.
            Ok(exit_code) => {
                println!("Exited early with exit code: {}", exit_code);

                Ok(())
            }
            Err(e) => {
                bail!("Unknown error `{}` found. expected `ErrorCode`", e);
            }
        },
    }
}

#[test]
fn test_early_exit() -> anyhow::Result<()> {
    main()
}
