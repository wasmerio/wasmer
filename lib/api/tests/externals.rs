use anyhow::Result;
use wasmer::*;

#[test]
fn global_new() -> Result<()> {
    let store = Store::default();
    let global = Global::new(&store, Value::I32(10));
    assert_eq!(
        *global.ty(),
        GlobalType {
            ty: Type::I32,
            mutability: Mutability::Const,
        }
    );

    let global_mut = Global::new_mut(&store, Value::I32(10));
    assert_eq!(
        *global_mut.ty(),
        GlobalType {
            ty: Type::I32,
            mutability: Mutability::Var,
        }
    );

    Ok(())
}

#[test]
fn global_get() -> Result<()> {
    let store = Store::default();
    let global_i32 = Global::new(&store, Value::I32(10));
    assert_eq!(global_i32.get(), Value::I32(10));
    let global_i64 = Global::new(&store, Value::I64(20));
    assert_eq!(global_i64.get(), Value::I64(20));
    let global_f32 = Global::new(&store, Value::F32(10.0));
    assert_eq!(global_f32.get(), Value::F32(10.0));
    let global_f64 = Global::new(&store, Value::F64(20.0));
    assert_eq!(global_f64.get(), Value::F64(20.0));

    Ok(())
}

#[test]
fn global_set() -> Result<()> {
    let store = Store::default();
    let global_i32 = Global::new(&store, Value::I32(10));
    // Set on a constant should error
    assert!(global_i32.set(Value::I32(20)).is_err());

    let global_i32_mut = Global::new_mut(&store, Value::I32(10));
    // Set on different type should error
    assert!(global_i32_mut.set(Value::I64(20)).is_err());

    // Set on same type should succeed
    global_i32_mut.set(Value::I32(20))?;
    assert_eq!(global_i32_mut.get(), Value::I32(20));

    Ok(())
}

#[test]
fn table_new() -> Result<()> {
    let store = Store::default();
    let table_type = TableType {
        ty: Type::FuncRef,
        minimum: 0,
        maximum: None,
    };
    let f = Function::new_native(&store, || {});
    let table = Table::new(&store, table_type, Value::FuncRef(f))?;
    assert_eq!(*table.ty(), table_type);

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

#[test]
#[ignore]
fn table_get() -> Result<()> {
    let store = Store::default();
    let table_type = TableType {
        ty: Type::FuncRef,
        minimum: 0,
        maximum: Some(1),
    };
    let f = Function::new_native(&store, |num: i32| num + 1);
    let table = Table::new(&store, table_type, Value::FuncRef(f.clone()))?;
    assert_eq!(*table.ty(), table_type);
    let _elem = table.get(0).unwrap();
    // assert_eq!(elem.funcref().unwrap(), f);
    Ok(())
}

#[test]
#[ignore]
fn table_set() -> Result<()> {
    // Table set not yet tested
    Ok(())
}

#[test]
fn table_grow() -> Result<()> {
    let store = Store::default();
    let table_type = TableType {
        ty: Type::FuncRef,
        minimum: 0,
        maximum: Some(10),
    };
    let f = Function::new_native(&store, |num: i32| num + 1);
    let table = Table::new(&store, table_type, Value::FuncRef(f.clone()))?;
    // Growing to a bigger maximum should return None
    let old_len = table.grow(12, Value::FuncRef(f.clone()));
    assert!(old_len.is_err());

    // Growing to a bigger maximum should return None
    let old_len = table.grow(5, Value::FuncRef(f.clone()))?;
    assert_eq!(old_len, 0);

    Ok(())
}

#[test]
#[ignore]
fn table_copy() -> Result<()> {
    // table copy test not yet implemented
    Ok(())
}

#[test]
fn memory_new() -> Result<()> {
    let store = Store::default();
    let memory_type = MemoryType {
        shared: false,
        minimum: Pages(0),
        maximum: Some(Pages(10)),
    };
    let memory = Memory::new(&store, memory_type)?;
    assert_eq!(memory.size(), Pages(0));
    assert_eq!(*memory.ty(), memory_type);
    Ok(())
}

#[test]
fn memory_grow() -> Result<()> {
    let store = Store::default();

    let desc = MemoryType::new(Pages(10), Some(Pages(16)), false);
    let memory = Memory::new(&store, desc)?;
    assert_eq!(memory.size(), Pages(10));

    let result = memory.grow(Pages(2)).unwrap();
    assert_eq!(result, Pages(10));
    assert_eq!(memory.size(), Pages(12));

    let result = memory.grow(Pages(10));
    assert_eq!(
        result,
        Err(MemoryError::CouldNotGrow {
            current: 12.into(),
            attempted_delta: 10.into(),
        })
    );

    let bad_desc = MemoryType::new(Pages(15), Some(Pages(10)), false);
    let bad_result = Memory::new(&store, bad_desc);

    assert!(matches!(bad_result, Err(MemoryError::InvalidMemory { .. })));

    Ok(())
}

#[test]
fn function_new() -> Result<()> {
    let store = Store::default();
    let function = Function::new_native(&store, || {});
    assert_eq!(function.ty().clone(), FunctionType::new(vec![], vec![]));
    let function = Function::new_native(&store, |_a: i32| {});
    assert_eq!(
        function.ty().clone(),
        FunctionType::new(vec![Type::I32], vec![])
    );
    let function = Function::new_native(&store, |_a: i32, _b: i64, _c: f32, _d: f64| {});
    assert_eq!(
        function.ty().clone(),
        FunctionType::new(vec![Type::I32, Type::I64, Type::F32, Type::F64], vec![])
    );
    let function = Function::new_native(&store, || -> i32 { 1 });
    assert_eq!(
        function.ty().clone(),
        FunctionType::new(vec![], vec![Type::I32])
    );
    let function = Function::new_native(&store, || -> (i32, i64, f32, f64) { (1, 2, 3.0, 4.0) });
    assert_eq!(
        function.ty().clone(),
        FunctionType::new(vec![], vec![Type::I32, Type::I64, Type::F32, Type::F64])
    );
    Ok(())
}

#[test]
fn function_new_env() -> Result<()> {
    let store = Store::default();
    #[derive(Clone)]
    struct MyEnv {};
    let my_env = MyEnv {};
    let function = Function::new_native_with_env(&store, my_env.clone(), |_env: &mut MyEnv| {});
    assert_eq!(function.ty().clone(), FunctionType::new(vec![], vec![]));
    let function =
        Function::new_native_with_env(&store, my_env.clone(), |_env: &mut MyEnv, _a: i32| {});
    assert_eq!(
        function.ty().clone(),
        FunctionType::new(vec![Type::I32], vec![])
    );
    let function = Function::new_native_with_env(
        &store,
        my_env.clone(),
        |_env: &mut MyEnv, _a: i32, _b: i64, _c: f32, _d: f64| {},
    );
    assert_eq!(
        function.ty().clone(),
        FunctionType::new(vec![Type::I32, Type::I64, Type::F32, Type::F64], vec![])
    );
    let function =
        Function::new_native_with_env(&store, my_env.clone(), |_env: &mut MyEnv| -> i32 { 1 });
    assert_eq!(
        function.ty().clone(),
        FunctionType::new(vec![], vec![Type::I32])
    );
    let function = Function::new_native_with_env(
        &store,
        my_env.clone(),
        |_env: &mut MyEnv| -> (i32, i64, f32, f64) { (1, 2, 3.0, 4.0) },
    );
    assert_eq!(
        function.ty().clone(),
        FunctionType::new(vec![], vec![Type::I32, Type::I64, Type::F32, Type::F64])
    );
    Ok(())
}

#[test]
fn function_new_dynamic() -> Result<()> {
    let store = Store::default();
    let function_type = FunctionType::new(vec![], vec![]);
    let function = Function::new(&store, &function_type, |_values: &[Value]| unimplemented!());
    assert_eq!(function.ty().clone(), function_type);
    let function_type = FunctionType::new(vec![Type::I32], vec![]);
    let function = Function::new(&store, &function_type, |_values: &[Value]| unimplemented!());
    assert_eq!(function.ty().clone(), function_type);
    let function_type = FunctionType::new(vec![Type::I32, Type::I64, Type::F32, Type::F64], vec![]);
    let function = Function::new(&store, &function_type, |_values: &[Value]| unimplemented!());
    assert_eq!(function.ty().clone(), function_type);
    let function_type = FunctionType::new(vec![], vec![Type::I32]);
    let function = Function::new(&store, &function_type, |_values: &[Value]| unimplemented!());
    assert_eq!(function.ty().clone(), function_type);
    let function_type = FunctionType::new(vec![], vec![Type::I32, Type::I64, Type::F32, Type::F64]);
    let function = Function::new(&store, &function_type, |_values: &[Value]| unimplemented!());
    assert_eq!(function.ty().clone(), function_type);
    Ok(())
}

#[test]
fn function_new_dynamic_env() -> Result<()> {
    let store = Store::default();
    #[derive(Clone)]
    struct MyEnv {};
    let my_env = MyEnv {};

    let function_type = FunctionType::new(vec![], vec![]);
    let function = Function::new_with_env(
        &store,
        &function_type,
        my_env.clone(),
        |_env: &mut MyEnv, _values: &[Value]| unimplemented!(),
    );
    assert_eq!(function.ty().clone(), function_type);
    let function_type = FunctionType::new(vec![Type::I32], vec![]);
    let function = Function::new_with_env(
        &store,
        &function_type,
        my_env.clone(),
        |_env: &mut MyEnv, _values: &[Value]| unimplemented!(),
    );
    assert_eq!(function.ty().clone(), function_type);
    let function_type = FunctionType::new(vec![Type::I32, Type::I64, Type::F32, Type::F64], vec![]);
    let function = Function::new_with_env(
        &store,
        &function_type,
        my_env.clone(),
        |_env: &mut MyEnv, _values: &[Value]| unimplemented!(),
    );
    assert_eq!(function.ty().clone(), function_type);
    let function_type = FunctionType::new(vec![], vec![Type::I32]);
    let function = Function::new_with_env(
        &store,
        &function_type,
        my_env.clone(),
        |_env: &mut MyEnv, _values: &[Value]| unimplemented!(),
    );
    assert_eq!(function.ty().clone(), function_type);
    let function_type = FunctionType::new(vec![], vec![Type::I32, Type::I64, Type::F32, Type::F64]);
    let function = Function::new_with_env(
        &store,
        &function_type,
        my_env.clone(),
        |_env: &mut MyEnv, _values: &[Value]| unimplemented!(),
    );
    assert_eq!(function.ty().clone(), function_type);
    Ok(())
}

#[test]
fn native_function_works() -> Result<()> {
    let store = Store::default();
    let function = Function::new_native(&store, || {});
    let native_function: NativeFunc<(), ()> = function.native().unwrap();
    let result = native_function.call();
    assert!(result.is_ok());

    // TODO:
    /*let function = Function::new_native(&store, |a: i32| -> i32 { a + 1 });
    let native_function: NativeFunc<i32, i32> = function.native().unwrap();
    assert!(native_function.call(3).unwrap(), 4);
    */

    fn rust_abi(a: i32, b: i64, c: f32, d: f64) -> u64 {
        (a as u64 * 1000) + (b as u64 * 100) + (c as u64 * 10) + (d as u64)
    }
    let function = Function::new_native(&store, rust_abi);
    let native_function: NativeFunc<(i32, i64, f32, f64), u64> = function.native().unwrap();
    assert_eq!(native_function.call(8, 4, 1.5, 5.).unwrap(), 8415);

    let function = Function::new_native(&store, || -> i32 { 1 });
    let native_function: NativeFunc<(), i32> = function.native().unwrap();
    assert_eq!(native_function.call().unwrap(), 1);

    // TODO:
    /*let function = Function::new_native(&store, |_a: i32| {});
    let native_function: NativeFunc<i32, ()> = function.native().unwrap();
    assert!(native_function.call(4).is_ok());*/

    // TODO:
    /*let function = Function::new_native(&store, || -> (i32, i64, f32, f64) { (1, 2, 3.0, 4.0) });
    let native_function: NativeFunc<(), (i32, i64, f32, f64)> = function.native().unwrap();
    assert_eq!(native_function.call().unwrap(), (1, 2, 3.0, 4.0));
    */
    Ok(())
}
