//! This is a simple example introducing the core concepts of the Wasmer API.
//!
//! You can run the example directly by executing the following in the Wasmer root:
//!
//! ```shell
//! cargo run --example hello-world --release --features "cranelift"
//! ```

use wasmer::{imports, wat2wasm, Function, Instance, Module, Store, TypedFunction};
use wasmer_compiler_cranelift::Cranelift;

fn main() -> anyhow::Result<()> {
    // First we create a simple Wasm program to use with Wasmer.
    // We use the WebAssembly text format and use `wasmer::wat2wasm` to compile
    // it into a WebAssembly binary.
    //
    // Most WebAssembly programs come from compiling source code in a high level
    // language and will already be in the binary format.
    let wasm_bytes = wat2wasm(
        br#"
(module
  ;; First we define a type with no parameters and no results.
  (type $no_args_no_rets_t (func (param) (result)))

  ;; Then we declare that we want to import a function named "env" "say_hello" with
  ;; that type signature.
  (import "env" "say_hello" (func $say_hello (type $no_args_no_rets_t)))

  ;; Finally we create an entrypoint that calls our imported function.
  (func $run (type $no_args_no_rets_t)
    (call $say_hello))
  ;; And mark it as an exported function named "run".
  (export "run" (func $run)))
"#,
    )?;

    // Next we create the `Store`, the top level type in the Wasmer API.
    //
    // Note that we don't need to specify the engine/compiler if we want to use
    // the default provided by Wasmer.
    // You can use `Store::default()` for that.
    //
    // However for the purposes of showing what's happening, we create a compiler
    // (`Cranelift`) and pass it to an engine (`Universal`). We then pass the engine to
    // the store and are now ready to compile and run WebAssembly!
    let mut store = Store::new(Cranelift::default());

    // We then use our store and Wasm bytes to compile a `Module`.
    // A `Module` is a compiled WebAssembly module that isn't ready to execute yet.
    let module = Module::new(&store, wasm_bytes)?;

    // We define a function to act as our "env" "say_hello" function imported in the
    // Wasm program above.
    fn say_hello_world() {
        println!("Hello, world!")
    }

    // We then create an import object so that the `Module`'s imports can be satisfied.
    let import_object = imports! {
        // We use the default namespace "env".
        "env" => {
            // And call our function "say_hello".
            "say_hello" => Function::new_typed(&mut store, say_hello_world),
        }
    };

    // We then use the `Module` and the import object to create an `Instance`.
    //
    // An `Instance` is a compiled WebAssembly module that has been set up
    // and is ready to execute.
    let instance = Instance::new(&mut store, &module, &import_object)?;

    // We get the `TypedFunction` with no parameters and no results from the instance.
    //
    // Recall that the Wasm module exported a function named "run", this is getting
    // that exported function from the `Instance`.
    let run_func: TypedFunction<(), ()> = instance.exports.get_typed_function(&mut store, "run")?;

    // Finally, we call our exported Wasm function which will call our "say_hello"
    // function and return.
    run_func.call(&mut store)?;

    Ok(())
}

#[test]
fn test_hello_world() -> anyhow::Result<()> {
    main()
}
