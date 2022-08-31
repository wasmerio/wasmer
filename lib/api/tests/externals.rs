use anyhow::Result;
#[cfg(feature = "js")]
use wasm_bindgen_test::*;

use wasmer::*;

#[cfg(feature = "js")]
#[cfg_attr(feature = "js", wasm_bindgen_test)]
fn global_new_js() {
    global_new().unwrap();
}

#[cfg_attr(feature = "sys", test)]
fn global_new() -> Result<()> {
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


#[cfg(feature = "js")]
#[cfg_attr(feature = "js", wasm_bindgen_test)]
fn global_get_js() {
    global_get().unwrap();
}

#[cfg_attr(feature = "sys", test)]
fn global_get() -> Result<()> {
    let mut store = Store::default();
    let global_i32 = Global::new(&mut store, Value::I32(10));
    assert_eq!(global_i32.get(&mut store), Value::I32(10));
    let global_i64 = Global::new(&mut store, Value::I64(20));
    assert_eq!(global_i64.get(&mut store), Value::I64(20));
    let global_f32 = Global::new(&mut store, Value::F32(10.0));
    assert_eq!(global_f32.get(&mut store), Value::F32(10.0));
    let global_f64 = Global::new(&mut store, Value::F64(20.0));
    assert_eq!(global_f64.get(&mut store), Value::F64(20.0));

    Ok(())
}

#[cfg(feature = "js")]
#[cfg_attr(feature = "js", wasm_bindgen_test)]
fn global_set_js() {
    global_set().unwrap();
}

#[cfg_attr(feature = "sys", test)]
fn global_set() -> Result<()> {
    let mut store = Store::default();
    let global_i32 = Global::new(&mut store, Value::I32(10));
    // Set on a constant should error
    assert!(global_i32.set(&mut store, Value::I32(20)).is_err());

    let global_i32_mut = Global::new_mut(&mut store, Value::I32(10));
    // Set on different type should error
    assert!(global_i32_mut.set(&mut store, Value::I64(20)).is_err());

    // Set on same type should succeed
    global_i32_mut.set(&mut store, Value::I32(20))?;
    assert_eq!(global_i32_mut.get(&mut store), Value::I32(20));

    Ok(())
}

#[cfg(feature = "js")]
#[cfg_attr(feature = "js", wasm_bindgen_test)]
fn table_new_js() {
    table_new().unwrap();
}

#[cfg_attr(feature = "sys", test)]
fn table_new() -> Result<()> {
    let mut store = Store::default();
    let table_type = TableType {
        ty: Type::FuncRef,
        minimum: 0,
        maximum: None,
    };
    let f = Function::new_typed(&mut store, || {});
    let table = Table::new(&mut store, table_type, Value::FuncRef(Some(f)))?;
    assert_eq!(table.ty(&mut store), table_type);

    // Anyrefs not yet supported
    // let table_type = TableType {
    //     ty: Type::ExternRef,
    //     minimum: 0,
    //     maximum: None,
    // };
    // let table = Table::new(&store, table_type, Value::ExternRef(ExternRef::Null))?;
    // assert_eq!(*table.ty(), table_type);

    Ok(())
}

#[cfg_attr(feature = "sys", test)]
#[cfg_attr(feature = "js", wasm_bindgen_test)]
#[ignore]
fn table_get_js() {
    table_get().unwrap();
}

#[cfg_attr(feature = "sys", test)]
fn table_get() -> Result<()> {
    let mut store = Store::default();
    let table_type = TableType {
        ty: Type::FuncRef,
        minimum: 0,
        maximum: Some(1),
    };
    let f = Function::new_typed(&mut store, |num: i32| num + 1);
    let table = Table::new(&mut store, table_type, Value::FuncRef(Some(f)))?;
    assert_eq!(table.ty(&mut store), table_type);
    let _elem = table.get(&mut store, 0).unwrap();
    // assert_eq!(elem.funcref().unwrap(), f);
    Ok(())
}

#[cfg_attr(feature = "sys", test)]
#[cfg_attr(feature = "js", wasm_bindgen_test)]
#[ignore]
fn table_set_js() {
    table_set().unwrap();
}

#[cfg_attr(feature = "sys", test)]
fn table_set() -> Result<()> {
    // Table set not yet tested
    Ok(())
}

#[cfg(feature = "js")]
#[cfg_attr(feature = "js", wasm_bindgen_test)]
fn table_grow_js() {
    table_grow().unwrap();
}

#[cfg_attr(feature = "sys", test)]
fn table_grow() -> Result<()> {
    let mut store = Store::default();
    let table_type = TableType {
        ty: Type::FuncRef,
        minimum: 0,
        maximum: Some(10),
    };
    let f = Function::new_typed(&mut store, |num: i32| num + 1);
    let table = Table::new(&mut store, table_type, Value::FuncRef(Some(f.clone())))?;
    // Growing to a bigger maximum should return None
    let old_len = table.grow(&mut store, 12, Value::FuncRef(Some(f.clone())));
    assert!(old_len.is_err());

    // Growing to a bigger maximum should return None
    let old_len = table.grow(&mut store, 5, Value::FuncRef(Some(f)))?;
    assert_eq!(old_len, 0);

    Ok(())
}

#[cfg_attr(feature = "sys", test)]
#[cfg_attr(feature = "js", wasm_bindgen_test)]
#[ignore]
fn table_copy_js() {
    table_copy().unwrap();
}

#[cfg_attr(feature = "sys", test)]
fn table_copy() -> Result<()> {
    // TODO: table copy test not yet implemented
    Ok(())
}

#[cfg(feature = "js")]
#[cfg_attr(feature = "js", wasm_bindgen_test)]
fn memory_new_js() {
    memory_new().unwrap();
}

#[cfg_attr(feature = "sys", test)]
fn memory_new() -> Result<()> {
    let mut store = Store::default();
    let memory_type = MemoryType {
        shared: false,
        minimum: Pages(0),
        maximum: Some(Pages(10)),
    };
    let memory = Memory::new(&mut store, memory_type)?;
    assert_eq!(memory.view(&mut store).size(), Pages(0));
    assert_eq!(memory.ty(&mut store), memory_type);
    Ok(())
}

#[cfg(feature = "js")]
#[cfg_attr(feature = "js", wasm_bindgen_test)]
fn memory_grow_js() {
    memory_grow().unwrap();
}

#[cfg_attr(feature = "sys", test)]
fn memory_grow() -> Result<()> {
    let mut store = Store::default();
    let desc = MemoryType::new(Pages(10), Some(Pages(16)), false);
    let memory = Memory::new(&mut store, desc)?;
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

    let bad_desc = MemoryType::new(Pages(15), Some(Pages(10)), false);
    let bad_result = Memory::new(&mut store, bad_desc);

    assert!(matches!(bad_result, Err(MemoryError::InvalidMemory { .. })));

    Ok(())
}

#[cfg(feature = "js")]
#[cfg_attr(feature = "js", wasm_bindgen_test)]
fn function_new_js() {
    function_new().unwrap();
}

#[cfg_attr(feature = "sys", test)]
fn function_new() -> Result<()> {
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
    let function =
        Function::new_typed(&mut store, || -> (i32, i64, f32, f64) { (1, 2, 3.0, 4.0) });
    assert_eq!(
        function.ty(&mut store).clone(),
        FunctionType::new(vec![], vec![Type::I32, Type::I64, Type::F32, Type::F64])
    );
    Ok(())
}

#[cfg(feature = "js")]
#[cfg_attr(feature = "js", wasm_bindgen_test)]
fn function_new_env_js() {
    function_new_env().unwrap();
}

#[cfg_attr(feature = "sys", test)]
fn function_new_env() -> Result<()> {
    let mut store = Store::default();
    #[derive(Clone)]
    struct MyEnv {}

    let my_env = MyEnv {};
    let env = FunctionEnv::new(&mut store, my_env);
    let function =
        Function::new_typed_with_env(&mut store, &env, |_env: FunctionEnvMut<MyEnv>| {});
    assert_eq!(
        function.ty(&mut store).clone(),
        FunctionType::new(vec![], vec![])
    );
    let function = Function::new_typed_with_env(
        &mut store,
        &env,
        |_env: FunctionEnvMut<MyEnv>, _a: i32| {},
    );
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
        Function::new_typed_with_env(&mut store, &env, |_env: FunctionEnvMut<MyEnv>| -> i32 {
            1
        });
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

#[cfg(feature = "js")]
#[cfg_attr(feature = "js", wasm_bindgen_test)]
fn function_new_dynamic_js() {
    function_new_dynamic().unwrap();
}

#[cfg_attr(feature = "sys", test)]
fn function_new_dynamic() -> Result<()> {
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
    let function_type =
        FunctionType::new(vec![Type::I32, Type::I64, Type::F32, Type::F64], vec![]);
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
    let function_type =
        FunctionType::new(vec![], vec![Type::I32, Type::I64, Type::F32, Type::F64]);
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

#[cfg(feature = "js")]
#[cfg_attr(feature = "js", wasm_bindgen_test)]
fn function_new_dynamic_env_js() {
    function_new_dynamic_env().unwrap();
}

#[cfg_attr(feature = "sys", test)]
fn function_new_dynamic_env() -> Result<()> {
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
    let function_type =
        FunctionType::new(vec![Type::I32, Type::I64, Type::F32, Type::F64], vec![]);
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
    let function_type =
        FunctionType::new(vec![], vec![Type::I32, Type::I64, Type::F32, Type::F64]);
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