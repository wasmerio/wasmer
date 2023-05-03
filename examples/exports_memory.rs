//! A Wasm module can export entities, like functions, memories,
//! globals and tables.
//!
//! This example illustrates how to use exported memories
//!
//! You can run the example directly by executing in Wasmer root:
//!
//! ```shell
//! cargo run --example exported-memory --release --features "cranelift"
//! ```
//!
//! Ready?

use wasmer::{imports, wat2wasm, Instance, Module, Store, TypedFunction, WasmPtr};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Let's declare the Wasm module with the text representation.
    let wasm_bytes = wat2wasm(
        br#"
(module
  (memory (export "mem") 1)

  (global $offset i32 (i32.const 42))
  (global $length (mut i32) (i32.const 13))

  (func (export "load") (result i32 i32)
    global.get $offset
    global.get $length)

  (data (i32.const 42) "Hello, World!"))
"#,
    )?;

    // Create a Store.
    let mut store = Store::default();

    println!("Compiling module...");
    // Let's compile the Wasm module.
    let module = Module::new(&store, wasm_bytes)?;

    // Create an empty import object.
    let import_object = imports! {};

    println!("Instantiating module...");
    // Let's instantiate the Wasm module.
    let instance = Instance::new(&mut store, &module, &import_object)?;

    let load: TypedFunction<(), (WasmPtr<u8>, i32)> =
        instance.exports.get_typed_function(&mut store, "load")?;

    // Here we go.
    //
    // The Wasm module exports a memory under "mem". Let's get it.
    let memory = instance.exports.get_memory("mem")?;

    // Now that we have the exported memory, let's get some
    // information about it.
    //
    // The first thing we might be intersted in is the size of the memory.
    // Let's get it!
    let memory_view = memory.view(&store);
    println!("Memory size (pages) {:?}", memory_view.size());
    println!("Memory size (bytes) {:?}", memory_view.data_size());

    // Oh! Wait, before reading the contents, we need to know
    // where to find what we are looking for.
    //
    // Fortunately, the Wasm module exports a `load` function
    // which will tell us the offset and length of the string.
    let (ptr, length) = load.call(&mut store)?;
    println!("String offset: {:?}", ptr.offset());
    println!("String length: {:?}", length);

    // We now know where to find our string, let's read it.
    //
    // We will get bytes out of the memory so we need to
    // decode them into a string.
    let memory_view = memory.view(&store);
    let str = ptr.read_utf8_string(&memory_view, length as u32).unwrap();
    println!("Memory contents: {:?}", str);

    // What about changing the contents of the memory with a more
    // appropriate string?
    //
    // To do that, we'll make a slice from our pointer and change the content
    // of each element.
    let new_str = b"Hello, Wasmer!";
    let values = ptr.slice(&memory_view, new_str.len() as u32).unwrap();
    for i in 0..new_str.len() {
        values.index(i as u64).write(new_str[i]).unwrap();
    }

    // And now, let's see the result.
    //
    // Since the new strings is bigger than the older one, we
    // query the length again. The offset remains the same as
    // before.
    println!("New string length: {:?}", new_str.len());

    let str = ptr
        .read_utf8_string(&memory_view, new_str.len() as u32)
        .unwrap();
    println!("New memory contents: {:?}", str);

    // Much better, don't you think?

    Ok(())
}

#[test]
fn test_exported_memory() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
