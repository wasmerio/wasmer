//! WebAssembly is a living standard. Wasmer integrates some
//! WebAssembly features that aren't yet stable but can still be
//! turned on. This example explains how.
//!
//! You can run the example directly by executing in Wasmer root:
//!
//! ```shell
//! cargo run --example features --release --features "cranelift"
//! ```
//!
//! Ready?

use wasmer::{imports, wat2wasm, EngineBuilder, Features, Instance, Module, Store, Value};
use wasmer_compiler_cranelift::Cranelift;

fn main() -> anyhow::Result<()> {
    // Let's declare the Wasm module with the text representation.
    let wasm_bytes = wat2wasm(
        br#"
(module
  (type $swap_t (func (param i32 i64) (result i64 i32)))
  (func $swap (type $swap_t) (param $x i32) (param $y i64) (result i64 i32)
    (local.get $y)
    (local.get $x))
  (export "swap" (func $swap)))
"#,
    )?;

    // Set up the compiler.
    let compiler = Cranelift::default();

    // Let's declare the features.
    let mut features = Features::new();
    // Enable the multi-value feature.
    features.multi_value(true);

    // Set up the engine. That's where we define the features!
    let engine = EngineBuilder::new(compiler).set_features(Some(features));

    // Now, let's define the store, and compile the module.
    let mut store = Store::new(engine);
    let module = Module::new(&store, wasm_bytes)?;

    // Finally, let's instantiate the module, and execute something
    // :-).
    let import_object = imports! {};
    let instance = Instance::new(&mut store, &module, &import_object)?;
    let swap = instance.exports.get_function("swap")?;

    let results = swap.call(&mut store, &[Value::I32(1), Value::I64(2)])?;

    assert_eq!(results.to_vec(), vec![Value::I64(2), Value::I32(1)]);

    Ok(())
}

#[test]
fn test_features() -> anyhow::Result<()> {
    main()
}
