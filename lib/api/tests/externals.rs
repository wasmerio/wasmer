use macro_wasmer_universal_test::universal_test;
#[cfg(feature = "js")]
use wasm_bindgen_test::*;

use wasmer::*;

#[cfg(feature = "sys")]
#[test]
fn test_wasi_fdrights() {
    use std::path::{Path, PathBuf};

    fn create_temp_dir() -> PathBuf {
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::env::temp_dir()
        }
        #[cfg(target_arch = "wasm32")]
        {
            let random = rand::random::<u64>();

            let dir = std::env::current_exe()
                .unwrap_or(Path::new("").to_path_buf())
                .join(&format!("temp-{random}"));

            std::fs::create_dir_all(&dir).unwrap();

            dir
        }
    }

    fn nuke_dir<P: AsRef<Path>>(path: P) -> Result<(), String> {
        use std::fs;
        let path = path.as_ref();
        for entry in fs::read_dir(path).map_err(|e| format!("{}", path.display()))? {
            let entry = entry.map_err(|e| format!("{}", path.display()))?;
            let path = entry.path();

            let file_type = entry
                .file_type()
                .map_err(|e| format!("{}: {e}", path.display()))?;

            if file_type.is_dir() {
                nuke_dir(&path)?;
                fs::remove_dir(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            } else {
                fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            }
        }

        Ok(())
    }

    static MAIN_RS: &str = include_str!("./wasmer-wasi-tests/src/main.rs");
    static CARGO_TOML: &str = include_str!("./wasmer-wasi-tests/Cargo.toml");
    let temp_dir = create_temp_dir();

    let _ = std::fs::create_dir_all(temp_dir.join("src"));
    let _ = std::fs::write(temp_dir.join("src").join("main.rs"), MAIN_RS);
    let _ = std::fs::write(temp_dir.join("Cargo.toml"), CARGO_TOML);

    let mut cmd = std::process::Command::new("rustup");
    cmd.arg("target");
    cmd.arg("add");
    cmd.arg("wasm32-wasi");
    let _ = cmd.output().unwrap();

    let mut cmd = std::process::Command::new("cargo");

    cmd.arg("build");
    cmd.arg("--release");
    cmd.arg("--manifest-path");
    cmd.arg(temp_dir.join("Cargo.toml"));
    cmd.arg("--target");
    cmd.arg("wasm32-wasi");

    let output = cmd.output().unwrap();

    println!("output: {:#?}", output);

    let release = temp_dir
        .join("target")
        .join("wasm32-wasi")
        .join("release")
        .join("wasmer-wasi-tests.wasm");

    println!("reading wasm file: {}", release.display());
    let file = std::fs::read(&release).unwrap();

    let _ = nuke_dir(temp_dir);

    let mut store = Store::default();
    let module = Module::from_binary(&mut store, &file).unwrap();
    let wasi_env = wasmer_wasi::WasiState::new("command-name")
        .finalize(&mut store)
        .unwrap();

    // Generate an `ImportObject`.
    let import_object = wasi_env.import_object(&mut store, &module).unwrap();

    // Let's instantiate the module with the imports.
    let instance = Instance::new(&mut store, &module, &import_object).unwrap();
    let memory = instance.exports.get_memory("memory").unwrap();
    wasi_env.data_mut(&mut store).set_memory(memory.clone());

    // Let's call the `_start` function, which is our `main` function in Rust.
    let start = instance.exports.get_function("_start").unwrap();
    let result = start.call(&mut store, &[]);

    assert!(result.is_err()); // program should panic (file not readable)

    // intentionally wrong to make the CI test fail
    let expected =
        r#"cannot access / (fd 5): Errno { code: 9, name: "EACCESS", message: "No access." }"#;
    if expected != format!("{result:?}") {
        println!("expected:");
        println!("{expected}");
        println!("result:");
        println!("{result:?}");
        panic!("expected != result");
    }
}

#[universal_test]
fn global_new() -> Result<(), String> {
    let mut store = Store::default();
    let global = Global::new(&mut store, Value::I32(10));
    assert_eq!(
        global.ty(&mut store),
        GlobalType {
            ty: Type::I32,
            mutability: Mutability::Const
        }
    );

    let global_mut = Global::new_mut(&mut store, Value::I32(10));
    assert_eq!(
        global_mut.ty(&mut store),
        GlobalType {
            ty: Type::I32,
            mutability: Mutability::Var
        }
    );

    Ok(())
}

#[universal_test]
fn global_get() -> Result<(), String> {
    let mut store = Store::default();

    let global_i32 = Global::new(&mut store, Value::I32(10));
    assert_eq!(global_i32.get(&mut store), Value::I32(10));

    // 64-bit values are not yet fully supported in some versions of Node
    #[cfg(feature = "sys")]
    {
        let global_i64 = Global::new(&mut store, Value::I64(20));
        assert_eq!(global_i64.get(&mut store), Value::I64(20));
    }

    let global_f32 = Global::new(&mut store, Value::F32(10.0));
    assert_eq!(global_f32.get(&mut store), Value::F32(10.0));

    // 64-bit values are not yet fully supported in some versions of Node
    #[cfg(feature = "sys")]
    {
        let global_f64 = Global::new(&mut store, Value::F64(20.0));
        assert_eq!(global_f64.get(&mut store), Value::F64(20.0));
    }

    Ok(())
}

#[universal_test]
fn global_set() -> Result<(), String> {
    let mut store = Store::default();
    let global_i32 = Global::new(&mut store, Value::I32(10));
    // Set on a constant should error
    assert!(global_i32.set(&mut store, Value::I32(20)).is_err());

    let global_i32_mut = Global::new_mut(&mut store, Value::I32(10));
    // Set on different type should error
    assert!(global_i32_mut.set(&mut store, Value::I64(20)).is_err());

    // Set on same type should succeed
    global_i32_mut
        .set(&mut store, Value::I32(20))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(global_i32_mut.get(&mut store), Value::I32(20));

    Ok(())
}

#[universal_test]
fn table_new() -> Result<(), String> {
    let mut store = Store::default();
    let table_type = TableType {
        ty: Type::FuncRef,
        minimum: 0,
        maximum: None,
    };
    let f = Function::new_typed(&mut store, || {});
    let table = Table::new(&mut store, table_type, Value::FuncRef(Some(f)))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(table.ty(&mut store), table_type);

    // Anyrefs not yet supported
    // let table_type = TableType {
    //     ty: Type::ExternRef,
    //     minimum: 0,
    //     maximum: None,
    // };
    // let table = Table::new(&store, table_type, Value::ExternRef(ExternRef::Null)).map_err(|e| format!("{e:?}"))?;
    // assert_eq!(*table.ty(), table_type);

    Ok(())
}

#[universal_test]
fn table_get() -> Result<(), String> {
    // Tables are not yet fully supported in Wasm
    // This test was marked as #[ignore] on -sys, which is why it is commented out.

    //    let mut store = Store::default();
    //    let table_type = TableType {
    //        ty: Type::FuncRef,
    //        minimum: 0,
    //        maximum: Some(1),
    //    };
    //    let f = Function::new_typed(&mut store, |num: i32| num + 1);
    //    let table = Table::new(&mut store, table_type, Value::FuncRef(Some(f)))
    //        .map_err(|e| format!("{e:?}"))?;
    //    assert_eq!(table.ty(&mut store), table_type);
    //    let _elem = table.get(&mut store, 0).unwrap();
    //    assert_eq!(elem.funcref().unwrap(), f);

    Ok(())
}

#[universal_test]
fn table_set() -> Result<(), String> {
    // Table set not yet tested
    Ok(())
}

#[universal_test]
fn table_grow() -> Result<(), String> {
    // Tables are not yet fully supported in Wasm
    #[cfg(feature = "sys")]
    {
        let mut store = Store::default();
        let table_type = TableType {
            ty: Type::FuncRef,
            minimum: 0,
            maximum: Some(10),
        };
        let f = Function::new_typed(&mut store, |num: i32| num + 1);
        let table = Table::new(&mut store, table_type, Value::FuncRef(Some(f.clone())))
            .map_err(|e| format!("{e:?}"))?;
        // Growing to a bigger maximum should return None
        let old_len = table.grow(&mut store, 12, Value::FuncRef(Some(f.clone())));
        assert!(old_len.is_err());

        // Growing to a bigger maximum should return None
        let old_len = table
            .grow(&mut store, 5, Value::FuncRef(Some(f)))
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(old_len, 0);
    }

    Ok(())
}

#[universal_test]
fn table_copy() -> Result<(), String> {
    // TODO: table copy test not yet implemented
    Ok(())
}

#[universal_test]
fn memory_new() -> Result<(), String> {
    let mut store = Store::default();
    let memory_type = MemoryType {
        shared: false,
        minimum: Pages(0),
        maximum: Some(Pages(10)),
    };
    let memory = Memory::new(&mut store, memory_type).map_err(|e| format!("{e:?}"))?;
    assert_eq!(memory.view(&mut store).size(), Pages(0));
    assert_eq!(memory.ty(&mut store), memory_type);
    Ok(())
}

#[universal_test]
fn memory_grow() -> Result<(), String> {
    let mut store = Store::default();
    let desc = MemoryType::new(Pages(10), Some(Pages(16)), false);
    let memory = Memory::new(&mut store, desc).map_err(|e| format!("{e:?}"))?;
    assert_eq!(memory.view(&mut store).size(), Pages(10));

    let result = memory.grow(&mut store, Pages(2)).unwrap();
    assert_eq!(result, Pages(10));
    assert_eq!(memory.view(&mut store).size(), Pages(12));

    let result = memory.grow(&mut store, Pages(10));
    assert_eq!(
        result,
        Err(MemoryError::CouldNotGrow {
            current: 12.into(),
            attempted_delta: 10.into()
        })
    );

    // JS will never give BadMemory unless V8 is broken somehow
    #[cfg(feature = "sys")]
    {
        let bad_desc = MemoryType::new(Pages(15), Some(Pages(10)), false);
        let bad_result = Memory::new(&mut store, bad_desc);
        assert!(matches!(bad_result, Err(MemoryError::InvalidMemory { .. })));
    }

    Ok(())
}

#[universal_test]
fn function_new() -> Result<(), String> {
    let mut store = Store::default();
    let function = Function::new_typed(&mut store, || {});
    assert_eq!(
        function.ty(&mut store).clone(),
        FunctionType::new(vec![], vec![])
    );
    let function = Function::new_typed(&mut store, |_a: i32| {});
    assert_eq!(
        function.ty(&mut store).clone(),
        FunctionType::new(vec![Type::I32], vec![])
    );
    let function = Function::new_typed(&mut store, |_a: i32, _b: i64, _c: f32, _d: f64| {});
    assert_eq!(
        function.ty(&mut store).clone(),
        FunctionType::new(vec![Type::I32, Type::I64, Type::F32, Type::F64], vec![])
    );
    let function = Function::new_typed(&mut store, || -> i32 { 1 });
    assert_eq!(
        function.ty(&mut store).clone(),
        FunctionType::new(vec![], vec![Type::I32])
    );
    let function = Function::new_typed(&mut store, || -> (i32, i64, f32, f64) { (1, 2, 3.0, 4.0) });
    assert_eq!(
        function.ty(&mut store).clone(),
        FunctionType::new(vec![], vec![Type::I32, Type::I64, Type::F32, Type::F64])
    );
    Ok(())
}

#[universal_test]
fn function_new_env() -> Result<(), String> {
    let mut store = Store::default();
    #[derive(Clone)]
    struct MyEnv {}

    let my_env = MyEnv {};
    let env = FunctionEnv::new(&mut store, my_env);
    let function = Function::new_typed_with_env(&mut store, &env, |_env: FunctionEnvMut<MyEnv>| {});
    assert_eq!(
        function.ty(&mut store).clone(),
        FunctionType::new(vec![], vec![])
    );
    let function =
        Function::new_typed_with_env(&mut store, &env, |_env: FunctionEnvMut<MyEnv>, _a: i32| {});
    assert_eq!(
        function.ty(&mut store).clone(),
        FunctionType::new(vec![Type::I32], vec![])
    );
    let function = Function::new_typed_with_env(
        &mut store,
        &env,
        |_env: FunctionEnvMut<MyEnv>, _a: i32, _b: i64, _c: f32, _d: f64| {},
    );
    assert_eq!(
        function.ty(&mut store).clone(),
        FunctionType::new(vec![Type::I32, Type::I64, Type::F32, Type::F64], vec![])
    );
    let function =
        Function::new_typed_with_env(&mut store, &env, |_env: FunctionEnvMut<MyEnv>| -> i32 { 1 });
    assert_eq!(
        function.ty(&mut store).clone(),
        FunctionType::new(vec![], vec![Type::I32])
    );
    let function = Function::new_typed_with_env(
        &mut store,
        &env,
        |_env: FunctionEnvMut<MyEnv>| -> (i32, i64, f32, f64) { (1, 2, 3.0, 4.0) },
    );
    assert_eq!(
        function.ty(&mut store).clone(),
        FunctionType::new(vec![], vec![Type::I32, Type::I64, Type::F32, Type::F64])
    );
    Ok(())
}

#[universal_test]
fn function_new_dynamic() -> Result<(), String> {
    let mut store = Store::default();

    // Using &FunctionType signature
    let function_type = FunctionType::new(vec![], vec![]);
    let function = Function::new(
        &mut store,
        &function_type,
        |_values: &[Value]| unimplemented!(),
    );
    assert_eq!(function.ty(&mut store).clone(), function_type);
    let function_type = FunctionType::new(vec![Type::I32], vec![]);
    let function = Function::new(
        &mut store,
        &function_type,
        |_values: &[Value]| unimplemented!(),
    );
    assert_eq!(function.ty(&mut store).clone(), function_type);
    let function_type = FunctionType::new(vec![Type::I32, Type::I64, Type::F32, Type::F64], vec![]);
    let function = Function::new(
        &mut store,
        &function_type,
        |_values: &[Value]| unimplemented!(),
    );
    assert_eq!(function.ty(&mut store).clone(), function_type);
    let function_type = FunctionType::new(vec![], vec![Type::I32]);
    let function = Function::new(
        &mut store,
        &function_type,
        |_values: &[Value]| unimplemented!(),
    );
    assert_eq!(function.ty(&mut store).clone(), function_type);
    let function_type = FunctionType::new(vec![], vec![Type::I32, Type::I64, Type::F32, Type::F64]);
    let function = Function::new(
        &mut store,
        &function_type,
        |_values: &[Value]| unimplemented!(),
    );
    assert_eq!(function.ty(&mut store).clone(), function_type);

    // Using array signature
    let function_type = ([Type::V128], [Type::I32, Type::F32, Type::F64]);
    let function = Function::new(
        &mut store,
        function_type,
        |_values: &[Value]| unimplemented!(),
    );
    assert_eq!(function.ty(&mut store).params(), [Type::V128]);
    assert_eq!(
        function.ty(&mut store).results(),
        [Type::I32, Type::F32, Type::F64]
    );

    Ok(())
}

#[universal_test]
fn function_new_dynamic_env() -> Result<(), String> {
    let mut store = Store::default();
    #[derive(Clone)]
    struct MyEnv {}
    let my_env = MyEnv {};
    let env = FunctionEnv::new(&mut store, my_env);

    // Using &FunctionType signature
    let function_type = FunctionType::new(vec![], vec![]);
    let function = Function::new_with_env(
        &mut store,
        &env,
        &function_type,
        |_env: FunctionEnvMut<MyEnv>, _values: &[Value]| unimplemented!(),
    );
    assert_eq!(function.ty(&mut store).clone(), function_type);
    let function_type = FunctionType::new(vec![Type::I32], vec![]);
    let function = Function::new_with_env(
        &mut store,
        &env,
        &function_type,
        |_env: FunctionEnvMut<MyEnv>, _values: &[Value]| unimplemented!(),
    );
    assert_eq!(function.ty(&mut store).clone(), function_type);
    let function_type = FunctionType::new(vec![Type::I32, Type::I64, Type::F32, Type::F64], vec![]);
    let function = Function::new_with_env(
        &mut store,
        &env,
        &function_type,
        |_env: FunctionEnvMut<MyEnv>, _values: &[Value]| unimplemented!(),
    );
    assert_eq!(function.ty(&mut store).clone(), function_type);
    let function_type = FunctionType::new(vec![], vec![Type::I32]);
    let function = Function::new_with_env(
        &mut store,
        &env,
        &function_type,
        |_env: FunctionEnvMut<MyEnv>, _values: &[Value]| unimplemented!(),
    );
    assert_eq!(function.ty(&mut store).clone(), function_type);
    let function_type = FunctionType::new(vec![], vec![Type::I32, Type::I64, Type::F32, Type::F64]);
    let function = Function::new_with_env(
        &mut store,
        &env,
        &function_type,
        |_env: FunctionEnvMut<MyEnv>, _values: &[Value]| unimplemented!(),
    );
    assert_eq!(function.ty(&mut store).clone(), function_type);

    // Using array signature
    let function_type = ([Type::V128], [Type::I32, Type::F32, Type::F64]);
    let function = Function::new_with_env(
        &mut store,
        &env,
        function_type,
        |_env: FunctionEnvMut<MyEnv>, _values: &[Value]| unimplemented!(),
    );
    assert_eq!(function.ty(&mut store).params(), [Type::V128]);
    assert_eq!(
        function.ty(&mut store).results(),
        [Type::I32, Type::F32, Type::F64]
    );

    Ok(())
}
