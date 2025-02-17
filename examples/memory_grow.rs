//! Example demonstrating WebAssembly memory growth limits
//!
//! Tests growing memory to maximum allowed size according to the spec
//! You can run the example directly by executing:
//! ```shell
//! cargo run --example memory-grow --release --features "cranelift"
//! ```

use wasmer::{imports, wat2wasm, Instance, Module, Store};

fn main() -> anyhow::Result<()> {
    let wasm_bytes = wat2wasm(
        r#"
(module
   (memory (export "memory") 1 65536)
   (func (export "mem_size") (result i32)
       memory.size)
   (func (export "grow") (param i32) (result i32)
       local.get 0
       memory.grow))
"#
        .as_bytes(),
    )?;

    let mut store = Store::default();
    let module = Module::new(&store, wasm_bytes)?;
    let instance = Instance::new(&mut store, &module, &imports! {})?;
    let memory = instance.exports.get_memory("memory")?;

    println!("Testing memory growth limits...");
    println!("Initial size: {:?}", memory.view(&store).size());

    let _ = memory.grow(&mut store, 65534)?;
    println!("After growing by 65534: {:?}", memory.view(&store).size());

    let _ = memory.grow(&mut store, 1)?;
    println!(
        "After growing to max (65536): {:?}",
        memory.view(&store).size()
    );

    let result = memory.grow(&mut store, 1);
    println!("Attempt to exceed max: {:?}", result);

    Ok(())
}

#[test]
fn test_memory_grow() -> anyhow::Result<()> {
    main()
}
