//! A Wasm module can import and export entities, like functions, memories, globals and tables.
//! This example illustrates the basics of using these entities.
//!
//! In this example we'll be using a sample Wasm module which exports some entities and requires us
//! to also import some of them.
//!
//! The goal here is to give you an idea of how to work with imports and exports. We won't go into
//! the details of each entities, they'll be covered in more details in the other examples.
//!
//! You can run the example directly by executing in Wasmer root:
//!
//! ```shell
//! cargo run --example imports-exports --release --features "cranelift"
//! ```
//!
//! Ready?

use wasmer::{
    imports, wat2wasm, Function, FunctionType, Global, Instance, Memory, Module, Store, Table,
    Type, Value,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Let's declare the Wasm module.
    //
    // We are using the text representation of the module here but you can also load `.wasm`
    // files using the `include_bytes!` macro.
    let wasm_bytes = wat2wasm(
        br#"
(module
  (func $host_function (import "" "host_function") (result i32))
  (global $host_global (import "env" "host_global") i32)

  (func $function (export "guest_function") (result i32) (global.get $global))
  (global $global (export "guest_global") i32 (i32.const 42))
  (table $table (export "guest_table") 1 1 funcref)
  (memory $memory (export "guest_memory") 1))
"#,
    )?;

    // Create a Store.
    let mut store = Store::default();

    println!("Compiling module...");
    // Let's compile the Wasm module.
    let module = Module::new(&store, wasm_bytes)?;

    // Here we go.
    //
    // Before we can instantiate our module, we need to define
    // the entities we will import.
    //
    // We won't go into details here as creating entities will be
    // covered in more detail in other examples.
    println!("Creating the imported function...");
    let host_function_signature = FunctionType::new(vec![], vec![Type::I32]);
    let host_function = Function::new(&mut store, &host_function_signature, |_args| {
        Ok(vec![Value::I32(42)])
    });

    println!("Creating the imported global...");
    let host_global = Global::new(&mut store, Value::I32(42));

    // Create an import object.
    //
    // Imports are stored in namespaces. We'll need to register each of the
    // namespaces with a name and add the imported entities there.
    //
    // Note that the namespace can also have an empty name.
    //
    // Our module requires us to import:
    //   * A function `host_function` in a namespace with an empty name;
    //   * A global `host_global` in the `env` namespace.
    //
    // Let's do this!
    let import_object = imports! {
        "" => {
            "host_function" => host_function,
        },
        "env" => {
            "host_global" => host_global,
        },
    };

    println!("Instantiating module...");
    // Let's instantiate the Wasm module.
    let instance = Instance::new(&mut store, &module, &import_object)?;

    // Here we go.
    //
    // The Wasm module exports some entities:
    //   * A function: `guest_function`
    //   * A global: `guest_global`
    //   * A memory: `guest_memory`
    //   * A table: `guest_table`
    //
    // Let's get them.
    println!("Getting the exported function...");
    let function = instance.exports.get::<Function>("guest_function")?;
    println!("Got exported function of type: {:?}", function.ty(&store));

    println!("Getting the exported global...");
    let global = instance.exports.get::<Global>("guest_global")?;
    println!("Got exported global of type: {:?}", global.ty(&store));

    println!("Getting the exported memory...");
    let memory = instance.exports.get::<Memory>("guest_memory")?;
    println!("Got exported memory of type: {:?}", memory.ty(&store));

    println!("Getting the exported table...");
    let table = instance.exports.get::<Table>("guest_table")?;
    println!("Got exported table of type: {:?}", table.ty(&store));

    Ok(())
}

#[test]
fn test_imports_exports() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
