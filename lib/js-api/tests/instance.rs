use anyhow::Result;
use wasm_bindgen_test::*;
use wasmer_js::*;

#[wasm_bindgen_test]
fn test_exported_memory() {
    let store = Store::default();
    let mut module = Module::new(
        &store,
        br#"
    (module
      (memory (export "mem") 1)
    )
    "#,
    )
    .unwrap();
    module.set_type_hints(ModuleTypeHints {
        imports: vec![],
        exports: vec![ExternType::Memory(MemoryType::new(Pages(1), None, false))],
    });

    let import_object = imports! {};
    let instance = Instance::new(&module, &import_object).unwrap();

    let memory = instance.exports.get_memory("mem").unwrap();
    assert_eq!(memory.ty(), MemoryType::new(Pages(1), None, false));
    assert_eq!(memory.size(), Pages(1));
    assert_eq!(memory.data_size(), 65536);

    memory.grow(Pages(1)).unwrap();
    assert_eq!(memory.ty(), MemoryType::new(Pages(2), None, false));
    assert_eq!(memory.size(), Pages(2));
    assert_eq!(memory.data_size(), 65536 * 2);
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

    let get_magic = instance.exports.get_function("get_magic").unwrap();
    assert_eq!(
        get_magic.ty().clone(),
        FunctionType::new(vec![], vec![Type::I32])
    );

    let expected = vec![Val::I32(42)].into_boxed_slice();
    assert_eq!(get_magic.call(&[]), Ok(expected));
}

#[wasm_bindgen_test]
fn test_imported_function_dynamic() {
    let store = Store::default();
    let mut module = Module::new(
        &store,
        br#"
    (module
        (func $imported (import "env" "imported") (param i32) (result i32))
        (func $imported_multivalue (import "env" "imported_multivalue") (param i32 i32) (result i32 i32))
        (func (export "exported") (param i32) (result i32)
            (call $imported (local.get 0))
        )
        (func (export "exported_multivalue") (param i32 i32) (result i32 i32)
            (call $imported_multivalue (local.get 0) (local.get 1))
        )
    )
    "#,
    )
    .unwrap();
    module.set_type_hints(ModuleTypeHints {
        imports: vec![
            ExternType::Function(FunctionType::new(vec![Type::I32], vec![Type::I32])),
            ExternType::Function(FunctionType::new(
                vec![Type::I32, Type::I32],
                vec![Type::I32, Type::I32],
            )),
        ],
        exports: vec![
            ExternType::Function(FunctionType::new(vec![Type::I32], vec![Type::I32])),
            ExternType::Function(FunctionType::new(
                vec![Type::I32, Type::I32],
                vec![Type::I32, Type::I32],
            )),
        ],
    });

    let imported_signature = FunctionType::new(vec![Type::I32], vec![Type::I32]);
    let imported = Function::new(&store, &imported_signature, |args| {
        println!("Calling `imported`...");
        let result = args[0].unwrap_i32() * 2;
        println!("Result of `imported`: {:?}", result);
        Ok(vec![Value::I32(result)])
    });

    let imported_multivalue_signature =
        FunctionType::new(vec![Type::I32, Type::I32], vec![Type::I32, Type::I32]);
    let imported_multivalue = Function::new(&store, &imported_multivalue_signature, |args| {
        println!("Calling `imported`...");
        // let result = args[0].unwrap_i32() * ;
        // println!("Result of `imported`: {:?}", result);
        Ok(vec![args[1].clone(), args[0].clone()])
    });

    let import_object = imports! {
        "env" => {
            "imported" => imported,
            "imported_multivalue" => imported_multivalue,
        }
    };
    let instance = Instance::new(&module, &import_object).unwrap();

    let exported = instance.exports.get_function("exported").unwrap();

    let expected = vec![Val::I32(6)].into_boxed_slice();
    assert_eq!(exported.call(&[Val::I32(3)]), Ok(expected));

    let exported_multivalue = instance
        .exports
        .get_function("exported_multivalue")
        .unwrap();

    let expected = vec![Val::I32(2), Val::I32(3)].into_boxed_slice();
    assert_eq!(
        exported_multivalue.call(&[Val::I32(3), Val::I32(2)]),
        Ok(expected)
    );
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

#[wasm_bindgen_test]
fn test_imported_exported_global() {
    let store = Store::default();
    let mut module = Module::new(
        &store,
        br#"
    (module
        (global $mut_i32_import (import "" "global") (mut i32))
        (func (export "getGlobal") (result i32) (global.get $mut_i32_import))
        (func (export "incGlobal") (global.set $mut_i32_import (
            i32.add (i32.const 1) (global.get $mut_i32_import)
        )))
    )
    "#,
    )
    .unwrap();
    module.set_type_hints(ModuleTypeHints {
        imports: vec![ExternType::Global(GlobalType::new(
            ValType::I32,
            Mutability::Var,
        ))],
        exports: vec![
            ExternType::Function(FunctionType::new(vec![], vec![Type::I32])),
            ExternType::Function(FunctionType::new(vec![], vec![])),
        ],
    });
    let mut global = Global::new_mut(&store, Value::I32(0));
    let import_object = imports! {
        "" => {
            "global" => global.clone()
        }
    };
    let instance = Instance::new(&module, &import_object).unwrap();

    let get_global = instance.exports.get_function("getGlobal").unwrap();
    assert_eq!(
        get_global.call(&[]),
        Ok(vec![Val::I32(0)].into_boxed_slice())
    );

    global.set(Value::I32(42));
    assert_eq!(
        get_global.call(&[]),
        Ok(vec![Val::I32(42)].into_boxed_slice())
    );

    let inc_global = instance.exports.get_function("incGlobal").unwrap();
    inc_global.call(&[]);
    assert_eq!(
        get_global.call(&[]),
        Ok(vec![Val::I32(43)].into_boxed_slice())
    );
    assert_eq!(global.get(), Val::I32(43));
}
