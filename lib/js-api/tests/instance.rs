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
    let store = Store::default();
    let module = Module::new(
        &store,
        br#"
    (module
      (memory (export "mem") 1)
    )
    "#,
    )
    .unwrap();

    let import_object = imports! {};
    let instance = Instance::new(&module, &import_object).unwrap();

    let memory = instance.exports.get_memory("mem").unwrap();
    assert_eq!(memory.size(), Pages(1));
    assert_eq!(memory.data_size(), 65536);

    // let load = instance
    //     .exports
    //     .get_native_function::<(), (WasmPtr<u8, Array>, i32)>("load")?;
}

#[wasm_bindgen_test]
fn test_exported_function() {
    let store = Store::default();
    let module = Module::new(
        &store,
        br#"
    (module
        (func (export "get_magic") (result i32)
          (i32.const 42)
        )
    )
    "#,
    )
    .unwrap();

    let import_object = imports! {};
    let instance = Instance::new(&module, &import_object).unwrap();

    // let memory = instance.exports.get_memory("mem").unwrap();
    // assert_eq!(memory.size(), Pages(1));
    // assert_eq!(memory.data_size(), 65536);

    let get_magic = instance.exports.get_function("get_magic").unwrap();

    let expected = vec![Val::F64(42.0)].into_boxed_slice();
    assert_eq!(get_magic.call(&[]), Ok(expected));
}

// #[wasm_bindgen_test]
// fn test_exported_function() {
//     let store = Store::default();
//     let module = Module::new(&store, br#"
//     (module
//         (memory (export "mem") 1)
//         (global $length (mut i32) (i32.const 13))

//         (func (export "load") (result i32 i32)
//           (i32.const 42)
//           global.get $length)

//         (data (i32.const 42) "Hello, World!"))
//     "#).unwrap();

//     let import_object = imports! {};
//     let instance = Instance::new(&module, &import_object).unwrap();

//     // let memory = instance.exports.get_memory("mem").unwrap();
//     // assert_eq!(memory.size(), Pages(1));
//     // assert_eq!(memory.data_size(), 65536);

//     let load = instance
//         .exports
//         .get_function::<(), (WasmPtr<u8, Array>, i32)>("load")?;
// }
