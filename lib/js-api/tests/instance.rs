use anyhow::Result;
use wasm_bindgen_test::*;
use wasmer_js::*;

// #[test]
// fn exports_work_after_multiple_instances_have_been_freed() -> Result<()> {
//     let store = Store::default();
//     let module = Module::new(
//         &store,
//         "
//     (module
//       (type $sum_t (func (param i32 i32) (result i32)))
//       (func $sum_f (type $sum_t) (param $x i32) (param $y i32) (result i32)
//         local.get $x
//         local.get $y
//         i32.add)
//       (export \"sum\" (func $sum_f)))
// ",
//     )?;

//     let import_object = ImportObject::new();
//     let instance = Instance::new(&module, &import_object)?;
//     let instance2 = instance.clone();
//     let instance3 = instance.clone();

//     // The function is cloned to “break” the connection with `instance`.
//     let sum = instance.exports.get_function("sum")?.clone();

//     drop(instance);
//     drop(instance2);
//     drop(instance3);

//     // All instances have been dropped, but `sum` continues to work!
//     assert_eq!(
//         sum.call(&[Value::I32(1), Value::I32(2)])?.into_vec(),
//         vec![Value::I32(3)],
//     );

//     Ok(())
// }

#[wasm_bindgen_test]
fn test_exported_memory() {
    // Let's declare the Wasm module with the text representation.
    let wasm_bytes = wat2wasm(
        br#"
(module
  (memory (export "mem") 1)
)
"#,
    )
    .unwrap();

    // Create a Store.
    // Note that we don't need to specify the engine/compiler if we want to use
    // the default provided by Wasmer.
    // You can use `Store::default()` for that.
    let store = Store::default();

    println!("Compiling module...");
    // Let's compile the Wasm module.
    let module = Module::new(&store, wasm_bytes).unwrap();

    // Create an empty import object.
    let import_object = imports! {};

    println!("Instantiating module...");
    // Let's instantiate the Wasm module.
    let instance = Instance::new(&module, &import_object).unwrap();

    // let load = instance
    //     .exports
    //     .get_native_function::<(), (WasmPtr<u8, Array>, i32)>("load")?;

    // Here we go.
    //
    // The Wasm module exports a memory under "mem". Let's get it.
    let memory = instance.exports.get_memory("mem").unwrap();

    // Now that we have the exported memory, let's get some
    // information about it.
    //
    // The first thing we might be intersted in is the size of the memory.
    // Let's get it!
    println!("Memory size (pages) {:?}", memory.size());
    println!("Memory size (bytes) {:?}", memory.data_size());
}
