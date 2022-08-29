#[cfg(feature = "js")]
mod js {
    use wasm_bindgen_test::*;
    use wasmer::*;

    #[wasm_bindgen_test]
    fn global_new() {
        let mut store = Store::default();
        let global = Global::new(&mut store, Value::I32(10));
        assert_eq!(
            global.ty(&store),
            GlobalType {
                ty: Type::I32,
                mutability: Mutability::Const
            }
        );

        let global_mut = Global::new_mut(&mut store, Value::I32(10));
        assert_eq!(
            global_mut.ty(&store),
            GlobalType {
                ty: Type::I32,
                mutability: Mutability::Var
            }
        );
    }

    #[wasm_bindgen_test]
    fn global_get() {
        let mut store = Store::default();
        let global_i32 = Global::new(&mut store, Value::I32(10));
        assert_eq!(global_i32.get(&mut store), Value::I32(10));
        // 64-bit values are not yet fully supported in some versions of Node
        // Commenting this tests for now:

        // let global_i64 = Global::new(&store, Value::I64(20));
        // assert_eq!(global_i64.get(), Value::I64(20));
        let global_f32 = Global::new(&mut store, Value::F32(10.0));
        assert_eq!(global_f32.get(&store), Value::F32(10.0));
        // let global_f64 = Global::new(&store, Value::F64(20.0));
        // assert_eq!(global_f64.get(), Value::F64(20.0));
    }

    #[wasm_bindgen_test]
    fn global_set() {
        let mut store = Store::default();
        let global_i32 = Global::new(&mut store, Value::I32(10));
        // Set on a constant should error
        assert!(global_i32.set(&mut store, Value::I32(20)).is_err());

        let global_i32_mut = Global::new_mut(&mut store, Value::I32(10));
        // Set on different type should error
        assert!(global_i32_mut.set(&mut store, Value::I64(20)).is_err());

        // Set on same type should succeed
        global_i32_mut.set(&mut store, Value::I32(20)).unwrap();
        assert_eq!(global_i32_mut.get(&store), Value::I32(20));
    }

    #[wasm_bindgen_test]
    fn table_new() {
        let mut store = Store::default();
        let table_type = TableType {
            ty: Type::FuncRef,
            minimum: 0,
            maximum: None,
        };
        let f = Function::new_typed(&mut store, || {});
        let table = Table::new(&mut store, table_type, Value::FuncRef(Some(f))).unwrap();
        assert_eq!(table.ty(&store), table_type);

        // table.get()
        // Anyrefs not yet supported
        // let table_type = TableType {
        //     ty: Type::ExternRef,
        //     minimum: 0,
        //     maximum: None,
        // };
        // let table = Table::new(&store, table_type, Value::ExternRef(ExternRef::Null))?;
        // assert_eq!(*table.ty(&store), table_type);
    }

    // Tables are not yet fully supported in Wasm
    // Commenting this tests for now

    // #[test]
    // #[ignore]
    // fn table_get() -> Result<()> {
    //     let mut store = Store::default();
    // let mut env = FunctionEnv::new(&mut store,  ());
    //     let table_type = TableType {
    //         ty: Type::FuncRef,
    //         minimum: 0,
    //         maximum: Some(1),
    //     };
    //     let f = Function::new(&mut store, |num: i32| num + 1);
    //     let table = Table::new(&store, table_type, Value::FuncRef(Some(f.clone())))?;
    //     assert_eq!(*table.ty(&store), table_type);
    //     let _elem = table.get(0).unwrap();
    //     // assert_eq!(elem.funcref().unwrap(), f);
    //     Ok(())
    // }

    // #[test]
    // #[ignore]
    // fn table_set() -> Result<()> {
    //     // Table set not yet tested
    //     Ok(())
    // }

    // #[test]
    // fn table_grow() -> Result<()> {
    //     let mut store = Store::default();
    // let mut env = FunctionEnv::new(&mut store,  ());
    //     let table_type = TableType {
    //         ty: Type::FuncRef,
    //         minimum: 0,
    //         maximum: Some(10),
    //     };
    //     let f = Function::new(&mut store, &env, |num: i32| num + 1);
    //     let table = Table::new(&store, table_type, Value::FuncRef(Some(f.clone())))?;
    //     // Growing to a bigger maximum should return None
    //     let old_len = table.grow(12, Value::FuncRef(Some(f.clone())));
    //     assert!(old_len.is_err());

    //     // Growing to a bigger maximum should return None
    //     let old_len = table.grow(5, Value::FuncRef(Some(f.clone())))?;
    //     assert_eq!(old_len, 0);

    //     Ok(())
    // }

    // #[test]
    // #[ignore]
    // fn table_copy() -> Result<()> {
    //     // TODO: table copy test not yet implemented
    //     Ok(())
    // }

    #[wasm_bindgen_test]
    fn memory_new() {
        let mut store = Store::default();
        let memory_type = MemoryType {
            shared: false,
            minimum: Pages(0),
            maximum: Some(Pages(10)),
        };
        let memory = Memory::new(&mut store, memory_type).unwrap();
        assert_eq!(memory.view(&store).size(), Pages(0));
        assert_eq!(memory.ty(&store), memory_type);
    }

    #[wasm_bindgen_test]
    fn memory_grow() {
        let mut store = Store::default();

        let desc = MemoryType::new(Pages(10), Some(Pages(16)), false);
        let memory = Memory::new(&mut store, desc).unwrap();
        assert_eq!(memory.view(&store).size(), Pages(10));

        let result = memory.grow(&mut store, Pages(2)).unwrap();
        assert_eq!(result, Pages(10));
        assert_eq!(memory.view(&store).size(), Pages(12));

        let result = memory.grow(&mut store, Pages(10));
        assert!(result.is_err());
        assert_eq!(
            result,
            Err(MemoryError::CouldNotGrow {
                current: 12.into(),
                attempted_delta: 10.into()
            })
        );
    }

    #[wasm_bindgen_test]
    fn function_new() {
        let mut store = Store::default();
        let function = Function::new_typed(&mut store, || {});
        assert_eq!(
            function.ty(&store).clone(),
            FunctionType::new(vec![], vec![])
        );
        let function = Function::new_typed(&mut store, |_a: i32| {});
        assert_eq!(
            function.ty(&store).clone(),
            FunctionType::new(vec![Type::I32], vec![])
        );
        let function = Function::new_typed(&mut store, |_a: i32, _b: i64, _c: f32, _d: f64| {});
        assert_eq!(
            function.ty(&store).clone(),
            FunctionType::new(vec![Type::I32, Type::I64, Type::F32, Type::F64], vec![])
        );
        let function = Function::new_typed(&mut store, || -> i32 { 1 });
        assert_eq!(
            function.ty(&store).clone(),
            FunctionType::new(vec![], vec![Type::I32])
        );
        let function =
            Function::new_typed(&mut store, || -> (i32, i64, f32, f64) { (1, 2, 3.0, 4.0) });
        assert_eq!(
            function.ty(&store).clone(),
            FunctionType::new(vec![], vec![Type::I32, Type::I64, Type::F32, Type::F64])
        );
    }

    #[wasm_bindgen_test]
    fn function_new_env() {
        let mut store = Store::default();
        #[derive(Clone)]
        struct MyEnv {}

        let my_env = MyEnv {};
        let env = FunctionEnv::new(&mut store, my_env);

        let function =
            Function::new_typed_with_env(&mut store, &env, |_: FunctionEnvMut<'_, MyEnv>| {});
        assert_eq!(
            function.ty(&store).clone(),
            FunctionType::new(vec![], vec![])
        );
        let function = Function::new_typed_with_env(
            &mut store,
            &env,
            |_: FunctionEnvMut<'_, MyEnv>, _a: i32| {},
        );
        assert_eq!(
            function.ty(&store).clone(),
            FunctionType::new(vec![Type::I32], vec![])
        );
        let function = Function::new_typed_with_env(
            &mut store,
            &env,
            |_: FunctionEnvMut<'_, MyEnv>, _a: i32, _b: i64, _c: f32, _d: f64| {},
        );
        assert_eq!(
            function.ty(&store).clone(),
            FunctionType::new(vec![Type::I32, Type::I64, Type::F32, Type::F64], vec![])
        );
        let function =
            Function::new_typed_with_env(&mut store, &env, |_: FunctionEnvMut<'_, MyEnv>| -> i32 {
                1
            });
        assert_eq!(
            function.ty(&store).clone(),
            FunctionType::new(vec![], vec![Type::I32])
        );
        let function = Function::new_typed_with_env(
            &mut store,
            &env,
            |_: FunctionEnvMut<'_, MyEnv>| -> (i32, i64, f32, f64) { (1, 2, 3.0, 4.0) },
        );
        assert_eq!(
            function.ty(&store).clone(),
            FunctionType::new(vec![], vec![Type::I32, Type::I64, Type::F32, Type::F64])
        );
    }

    #[wasm_bindgen_test]
    fn function_new_dynamic() {
        let mut store = Store::default();

        // Using &FunctionType signature
        let function_type = FunctionType::new(vec![], vec![]);
        let function = Function::new(
            &mut store,
            &function_type,
            |_values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&store).clone(), function_type);
        let function_type = FunctionType::new(vec![Type::I32], vec![]);
        let function = Function::new(
            &mut store,
            &function_type,
            |_values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&store).clone(), function_type);
        let function_type =
            FunctionType::new(vec![Type::I32, Type::I64, Type::F32, Type::F64], vec![]);
        let function = Function::new(
            &mut store,
            &function_type,
            |_values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&store).clone(), function_type);
        let function_type = FunctionType::new(vec![], vec![Type::I32]);
        let function = Function::new(
            &mut store,
            &function_type,
            |_values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&store).clone(), function_type);
        let function_type =
            FunctionType::new(vec![], vec![Type::I32, Type::I64, Type::F32, Type::F64]);
        let function = Function::new(
            &mut store,
            &function_type,
            |_values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&store).clone(), function_type);

        // Using array signature
        let function_type = ([Type::V128], [Type::I32, Type::F32, Type::F64]);
        let function = Function::new(
            &mut store,
            function_type,
            |_values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&store).params(), [Type::V128]);
        assert_eq!(
            function.ty(&store).results(),
            [Type::I32, Type::F32, Type::F64]
        );
    }

    #[wasm_bindgen_test]
    fn function_new_dynamic_env() {
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
            |_env: FunctionEnvMut<'_, MyEnv>, _values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&store).clone(), function_type);
        let function_type = FunctionType::new(vec![Type::I32], vec![]);
        let function = Function::new_with_env(
            &mut store,
            &env,
            &function_type,
            |_env: FunctionEnvMut<'_, MyEnv>, _values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&store).clone(), function_type);
        let function_type =
            FunctionType::new(vec![Type::I32, Type::I64, Type::F32, Type::F64], vec![]);
        let function = Function::new_with_env(
            &mut store,
            &env,
            &function_type,
            |_env: FunctionEnvMut<'_, MyEnv>, _values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&store).clone(), function_type);
        let function_type = FunctionType::new(vec![], vec![Type::I32]);
        let function = Function::new_with_env(
            &mut store,
            &env,
            &function_type,
            |_env: FunctionEnvMut<'_, MyEnv>, _values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&store).clone(), function_type);
        let function_type =
            FunctionType::new(vec![], vec![Type::I32, Type::I64, Type::F32, Type::F64]);
        let function = Function::new_with_env(
            &mut store,
            &env,
            &function_type,
            |_env: FunctionEnvMut<'_, MyEnv>, _values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&store).clone(), function_type);

        // Using array signature
        let function_type = ([Type::V128], [Type::I32, Type::F32, Type::F64]);
        let function = Function::new_with_env(
            &mut store,
            &env,
            function_type,
            |_env: FunctionEnvMut<'_, MyEnv>, _values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&store).params(), [Type::V128]);
        assert_eq!(
            function.ty(&store).results(),
            [Type::I32, Type::F32, Type::F64]
        );
    }

    #[wasm_bindgen_test]
    fn native_function_works() {
        let mut store = Store::default();
        let function = Function::new_typed(&mut store, || {});
        let typed_function: TypedFunction<(), ()> = function.typed(&mut store).unwrap();
        let result = typed_function.call(&mut store);
        assert!(result.is_ok());

        let function = Function::new_typed(&mut store, |a: i32| -> i32 { a + 1 });
        let typed_function: TypedFunction<i32, i32> = function.typed(&mut store).unwrap();
        assert_eq!(typed_function.call(&mut store, 3).unwrap(), 4);

        // fn rust_abi(a: i32, b: i64, c: f32, d: f64) -> u64 {
        //     (a as u64 * 1000) + (b as u64 * 100) + (c as u64 * 10) + (d as u64)
        // }
        // let function = Function::new(&mut store, &env, rust_abi);
        // let typed_function: TypedFunction<(i32, i64, f32, f64), u64> = function.native(&mut store).unwrap();
        // assert_eq!(typed_function.call(8, 4, 1.5, 5.).unwrap(), 8415);

        let function = Function::new_typed(&mut store, || -> i32 { 1 });
        let typed_function: TypedFunction<(), i32> = function.typed(&mut store).unwrap();
        assert_eq!(typed_function.call(&mut store).unwrap(), 1);

        let function = Function::new_typed(&mut store, |_a: i32| {});
        let typed_function: TypedFunction<i32, ()> = function.typed(&mut store).unwrap();
        assert!(typed_function.call(&mut store, 4).is_ok());

        // let function = Function::new(&mut store, &env, || -> (i32, i64, f32, f64) { (1, 2, 3.0, 4.0) });
        // let typed_function: TypedFunction<(), (i32, i64, f32, f64)> = function.native(&mut store).unwrap();
        // assert_eq!(typed_function.call().unwrap(), (1, 2, 3.0, 4.0));
    }

    #[wasm_bindgen_test]
    fn function_outlives_instance() {
        let mut store = Store::default();
        let wat = r#"(module
      (type $sum_t (func (param i32 i32) (result i32)))
      (func $sum_f (type $sum_t) (param $x i32) (param $y i32) (result i32)
        local.get $x
        local.get $y
        i32.add)
      (export "sum" (func $sum_f)))
    "#;

        let f = {
            let module = Module::new(&store, wat).unwrap();
            let instance = Instance::new(&mut store, &module, &imports! {}).unwrap();
            let f = instance.exports.get_function("sum").unwrap();

            assert_eq!(
                f.call(&mut store, &[Val::I32(4), Val::I32(5)]).unwrap(),
                vec![Val::I32(9)].into_boxed_slice()
            );
            f.clone()
        };

        assert_eq!(
            f.call(&mut store, &[Val::I32(4), Val::I32(5)]).unwrap(),
            vec![Val::I32(9)].into_boxed_slice()
        );
    }
}
