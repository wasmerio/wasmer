//! With Wasmer you'll be able to interact with guest module memory.
//!
//! This example illustrates the basics of interacting with Wasm module memory.:
//!
//!   1. How to load a Wasm modules as bytes
//!   2. How to compile the module
//!   3. How to create an instance of the module
//!
//! You can run the example directly by executing in Wasmer root:
//!
//! ```shell
//! cargo run --example memory --release --features "cranelift"
//! ```
//!
//! Ready?

use std::mem;
use wasmer::{imports, wat2wasm, Bytes, Instance, Module, Pages, Store, TypedFunction};

// this example is a work in progress:
// TODO: clean it up and comment it https://github.com/wasmerio/wasmer/issues/1749

fn main() -> anyhow::Result<()> {
    // Let's declare the Wasm module.
    //
    // We are using the text representation of the module here but you can also load `.wasm`
    // files using the `include_bytes!` macro.
    let wasm_bytes = wat2wasm(
        r#"
(module
  (type $mem_size_t (func (result i32)))
  (type $get_at_t (func (param i32) (result i32)))
  (type $set_at_t (func (param i32) (param i32)))

  (memory $mem 1)

  (func $get_at (type $get_at_t) (param $idx i32) (result i32)
    (i32.load (local.get $idx)))

  (func $set_at (type $set_at_t) (param $idx i32) (param $val i32)
    (i32.store (local.get $idx) (local.get $val)))

  (func $mem_size (type $mem_size_t) (result i32)
    (memory.size))

  (export "get_at" (func $get_at))
  (export "set_at" (func $set_at))
  (export "mem_size" (func $mem_size))
  (export "memory" (memory $mem)))
"#
        .as_bytes(),
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

    // The module exports some utility functions, let's get them.
    //
    // These function will be used later in this example.
    let mem_size: TypedFunction<(), i32> = instance
        .exports
        .get_typed_function(&mut store, "mem_size")?;
    let get_at: TypedFunction<i32, i32> =
        instance.exports.get_typed_function(&mut store, "get_at")?;
    let set_at: TypedFunction<(i32, i32), ()> =
        instance.exports.get_typed_function(&mut store, "set_at")?;
    let memory = instance.exports.get_memory("memory")?;

    // We now have an instance ready to be used.
    //
    // We will start by querying the most intersting information
    // about the memory: its size. There are mainly two ways of getting
    // this:
    // * the size as a number of `Page`s
    // * the size as a number of bytes
    //
    // The size in bytes can be found either by querying its pages or by
    // querying the memory directly.
    println!("Querying memory size...");
    let memory_view = memory.view(&store);
    assert_eq!(memory_view.size(), Pages::from(1));
    assert_eq!(memory_view.size().bytes(), Bytes::from(65536 as usize));
    assert_eq!(memory_view.data_size(), 65536);

    // Sometimes, the guest module may also export a function to let you
    // query the memory. Here we have a `mem_size` function, let's try it:
    let result = mem_size.call(&mut store)?;

    let memory_view = memory.view(&store);
    println!("Memory size: {:?}", result);
    assert_eq!(Pages::from(result as u32), memory_view.size());

    // Now that we know the size of our memory, it's time to see how wa
    // can change this.
    //
    // A memory can be grown to allow storing more things into it. Let's
    // see how we can do that:
    println!("Growing memory...");

    // 'wasm-c-api' does not support direct calls to memory.grow()
    #[cfg(not(feature = "wasm-c-api"))]
    {
        // Here we are requesting two more pages for our memory.
        memory.grow(&mut store, 2)?;

        let memory_view = memory.view(&store);
        assert_eq!(memory_view.size(), Pages::from(3));
        assert_eq!(memory_view.data_size(), 65536 * 3);

        // Now that we know how to query and adjust the size of the memory,
        // let's see how wa can write to it or read from it.
        //
        // We'll only focus on how to do this using exported functions, the goal
        // is to show how to work with memory addresses. Here we'll use absolute
        // addresses to write and read a value.
        let mem_addr = 0x2220;
        let val = 0xFEFEFFE;
        set_at.call(&mut store, mem_addr, val)?;

        let result = get_at.call(&mut store, mem_addr)?;
        println!("Value at {:#x?}: {:?}", mem_addr, result);
        assert_eq!(result, val);

        // Now instead of using hard coded memory addresses, let's try to write
        // something at the end of the second memory page and read it.
        let page_size = 0x1_0000;
        let mem_addr = (page_size * 2) - mem::size_of_val(&val) as i32;
        let val = 0xFEA09;
        set_at.call(&mut store, mem_addr, val)?;

        let result = get_at.call(&mut store, mem_addr)?;
        println!("Value at {:#x?}: {:?}", mem_addr, result);
        assert_eq!(result, val);
    }

    Ok(())
}

#[test]
fn test_memory() -> anyhow::Result<()> {
    main()
}
