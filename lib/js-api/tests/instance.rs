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
    let mut module = Module::new(
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
    module.set_type_hints(ModuleTypeHints {
        imports: vec![],
        exports: vec![ExternType::Function(FunctionType::new(
            vec![],
            vec![Type::I32],
        ))],
    });

    let import_object = imports! {};
    let instance = Instance::new(&module, &import_object).unwrap();

    // let memory = instance.exports.get_memory("mem").unwrap();
    // assert_eq!(memory.size(), Pages(1));
    // assert_eq!(memory.data_size(), 65536);

    let get_magic = instance.exports.get_function("get_magic").unwrap();
    assert_eq!(
        get_magic.ty().clone(),
        FunctionType::new(vec![], vec![Type::I32])
    );

    let expected = vec![Val::I32(42)].into_boxed_slice();
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

#[wasm_bindgen_test]
fn test_imported_function_dynamic() {
    let store = Store::default();
    let mut module = Module::new(
        &store,
        br#"
    (module
        (func $imported (import "env" "imported") (param i32) (result i32))
        (func (export "exported") (param i32) (result i32)
            (call $imported (local.get 0))
        )
    )
    "#,
    )
    .unwrap();
    module.set_type_hints(ModuleTypeHints {
        imports: vec![ExternType::Function(FunctionType::new(
            vec![Type::I32],
            vec![Type::I32],
        ))],
        exports: vec![ExternType::Function(FunctionType::new(
            vec![Type::I32],
            vec![Type::I32],
        ))],
    });

    let imported_signature = FunctionType::new(vec![Type::I32], vec![Type::I32]);
    let imported = Function::new(&store, &imported_signature, |args| {
        println!("Calling `imported`...");
        let result = args[0].unwrap_i32() * 2;
        println!("Result of `imported`: {:?}", result);
        Ok(vec![Value::I32(result)])
    });

    let import_object = imports! {
        "env" => {
            "imported" => imported,
        }
    };
    let instance = Instance::new(&module, &import_object).unwrap();

    // let memory = instance.exports.get_memory("mem").unwrap();
    // assert_eq!(memory.size(), Pages(1));
    // assert_eq!(memory.data_size(), 65536);

    let exported = instance.exports.get_function("exported").unwrap();

    let expected = vec![Val::I32(5)].into_boxed_slice();
    assert_eq!(exported.call(&[Val::I32(4)]), Ok(expected));
}

#[wasm_bindgen_test]
fn test_imported_function_native() {
    let store = Store::default();
    let mut module = Module::new(
        &store,
        br#"
    (module
        (func $imported (import "env" "imported") (param i32) (result i32))
        (func (export "exported") (param i32) (result i32)
            (call $imported (local.get 0))
        )
    )
    "#,
    )
    .unwrap();
    module.set_type_hints(ModuleTypeHints {
        imports: vec![ExternType::Function(FunctionType::new(
            vec![Type::I32],
            vec![Type::I32],
        ))],
        exports: vec![ExternType::Function(FunctionType::new(
            vec![Type::I32],
            vec![Type::I32],
        ))],
    });

    fn imported_fn(arg: u32) -> u32 {
        return arg + 1;
    }

    let imported = Function::new_native(&store, imported_fn);

    let import_object = imports! {
        "env" => {
            "imported" => imported,
        }
    };
    let instance = Instance::new(&module, &import_object).unwrap();

    // let memory = instance.exports.get_memory("mem").unwrap();
    // assert_eq!(memory.size(), Pages(1));
    // assert_eq!(memory.data_size(), 65536);

    let exported = instance.exports.get_function("exported").unwrap();

    let expected = vec![Val::I32(5)].into_boxed_slice();
    assert_eq!(exported.call(&[Val::I32(4)]), Ok(expected));
}

#[wasm_bindgen_test]
fn test_imported_function_native_with_env() {
    let store = Store::default();
    let mut module = Module::new(
        &store,
        br#"
    (module
        (func $imported (import "env" "imported") (param i32) (result i32))
        (func (export "exported") (param i32) (result i32)
            (call $imported (local.get 0))
        )
    )
    "#,
    )
    .unwrap();
    module.set_type_hints(ModuleTypeHints {
        imports: vec![ExternType::Function(FunctionType::new(
            vec![Type::I32],
            vec![Type::I32],
        ))],
        exports: vec![ExternType::Function(FunctionType::new(
            vec![Type::I32],
            vec![Type::I32],
        ))],
    });

    #[derive(WasmerEnv, Clone)]
    struct Env {
        multiplier: u32,
    }

    fn imported_fn(env: &Env, arg: u32) -> u32 {
        return env.multiplier * arg;
    }

    let imported = Function::new_native_with_env(&store, Env { multiplier: 3 }, imported_fn);

    let import_object = imports! {
        "env" => {
            "imported" => imported,
        }
    };
    let instance = Instance::new(&module, &import_object).unwrap();

    // let memory = instance.exports.get_memory("mem").unwrap();
    // assert_eq!(memory.size(), Pages(1));
    // assert_eq!(memory.data_size(), 65536);

    let exported = instance.exports.get_function("exported").unwrap();

    let expected = vec![Val::I32(12)].into_boxed_slice();
    assert_eq!(exported.call(&[Val::I32(4)]), Ok(expected));
}

#[wasm_bindgen_test]
fn test_imported_function_native_with_wasmer_env() {
    let store = Store::default();
    let mut module = Module::new(
        &store,
        br#"
    (module
        (func $imported (import "env" "imported") (param i32) (result i32))
        (func (export "exported") (param i32) (result i32)
            (call $imported (local.get 0))
        )
        (memory (export "memory") 1)
    )
    "#,
    )
    .unwrap();
    module.set_type_hints(ModuleTypeHints {
        imports: vec![ExternType::Function(FunctionType::new(
            vec![Type::I32],
            vec![Type::I32],
        ))],
        exports: vec![
            ExternType::Function(FunctionType::new(vec![Type::I32], vec![Type::I32])),
            ExternType::Memory(MemoryType::new(Pages(1), None, false)),
        ],
    });

    #[derive(WasmerEnv, Clone)]
    struct Env {
        multiplier: u32,
        #[wasmer(export)]
        memory: LazyInit<Memory>,
    }

    fn imported_fn(env: &Env, arg: u32) -> u32 {
        let memory = env.memory_ref().unwrap();
        let memory_val = memory.uint8view().get_index(0);
        return (memory_val as u32) * env.multiplier * arg;
    }

    let imported = Function::new_native_with_env(
        &store,
        Env {
            multiplier: 3,
            memory: LazyInit::new(),
        },
        imported_fn,
    );

    let import_object = imports! {
        "env" => {
            "imported" => imported,
        }
    };
    let instance = Instance::new(&module, &import_object).unwrap();

    let memory = instance.exports.get_memory("memory").unwrap();
    assert_eq!(memory.data_size(), 65536);
    let memory_val = memory.uint8view().get_index(0);
    assert_eq!(memory_val, 0);

    memory.uint8view().set_index(0, 2);
    let memory_val = memory.uint8view().get_index(0);
    assert_eq!(memory_val, 2);

    let exported = instance.exports.get_function("exported").unwrap();

    /// It with the provided memory
    let expected = vec![Val::I32(24)].into_boxed_slice();
    assert_eq!(exported.call(&[Val::I32(4)]), Ok(expected));

    /// It works if we update the memory
    memory.uint8view().set_index(0, 3);
    let expected = vec![Val::I32(36)].into_boxed_slice();
    assert_eq!(exported.call(&[Val::I32(4)]), Ok(expected));
}
