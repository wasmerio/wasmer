//! A Wasm module can export entities, like functions, memories,
//! globals and tables.
//!
//! This example illustrates how to use exported functions. They come
//! in 2 flavors:
//!
//!   1. Dynamic functions, where parameters and results are of a
//!      slice of `Value`,
//!   2. Native function, where parameters and results are statically
//!      typed Rust values.
//!
//! Ready?

use std::sync::Arc;
use wasmer::{imports, wat2wasm, Instance, Module, Store, Value};
use wasmer_compiler_cranelift::Cranelift;
use wasmer_engine_jit::JIT;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Let's declare the Wasm module with the text representation.
    let wasm_bytes = wat2wasm(
        r#"
(module
  (type $sum_t (func (param i32 i32) (result i32)))
  (func $sum_f (type $sum_t) (param $x i32) (param $y i32) (result i32)
    local.get $x
    local.get $y
    i32.add)
  (export "sum" (func $sum_f)))
"#
        .as_bytes(),
    )?;

    // Define a compiler configuration.
    let mut compiler_config = Cranelift::default();

    // Define the engine that will drive everything.
    let engine = JIT::new(&mut compiler_config).engine();

    // Create a store, that holds the engine.
    let store = Store::new(&*engine);

    // Let's compile the Wasm module.
    let module = Module::new(&store, wasm_bytes)?;

    // Create an empty import object.
    let import_object = imports! {};

    // Let's instantiate the Wasm module.
    let instance = Instance::new(&module, &import_object)?;

    // Here we go.
    //
    // The Wasm module exports a function called `sum`. Let's get
    // it. Note that
    //
    //     ```
    //     get_function(name)
    //     ```
    //
    // is just an alias to
    //
    //     ```
    //     get::<Function>(name)`.
    //     ```
    let sum = instance.exports.get_function("sum")?;

    // Let's call the `sum` exported function. The parameters are a
    // slice of `Value`s. The results are a boxed slice of `Value`s.
    let results = sum.call(&[Value::I32(1), Value::I32(2)])?;

    assert_eq!(results.to_vec(), vec![Value::I32(3)]);

    // That was fun. But what if we can get rid of the `Value`s? Well,
    // that's possible with the `NativeFunction` API. The function
    // will use native Rust values.
    //
    // Note that `native` takes 2 generic parameters: `Args` and
    // `Rets`, respectively for the parameters and the results. If
    // those values don't match the exported function signature, an
    // error will be raised.
    let sum = sum.native::<(i32, i32), i32>()?;

    // Let's call the `sum` exported function. The parameters are
    // statically typed Rust values of type `i32` and `i32`. The
    // result, in this case particular case, in a unit of type `i32`.
    let result = sum.call(1, 2)?;

    assert_eq!(result, 3);

    // Much nicer, isn't it?
    //
    // Those two API exist because they addres different needs. The
    // former has a more dynamic approach, while the second has a more
    // static approach.

    Ok(())
}

#[test]
fn test_exported_function() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
