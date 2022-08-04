#[cfg(feature = "sys")]
mod sys {
    use anyhow::Result;
    use wasmer::FunctionEnv;
    use wasmer::*;

    #[test]
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

    #[test]
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

    #[test]
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

    #[test]
    fn table_new() -> Result<()> {
        let mut store = Store::default();
        let table_type = TableType {
            ty: Type::FuncRef,
            minimum: 0,
            maximum: None,
        };
        let f = Function::new_typed(&mut store, |_env: FunctionEnvMut<()>| {});
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

    #[test]
    #[ignore]
    fn table_get() -> Result<()> {
        let mut store = Store::default();
        let table_type = TableType {
            ty: Type::FuncRef,
            minimum: 0,
            maximum: Some(1),
        };
        let f = Function::new_typed(&mut store, |_env: FunctionEnvMut<()>, num: i32| num + 1);
        let table = Table::new(&mut store, table_type, Value::FuncRef(Some(f)))?;
        assert_eq!(table.ty(&mut store), table_type);
        let _elem = table.get(&mut store, 0).unwrap();
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
        let mut store = Store::default();
        let table_type = TableType {
            ty: Type::FuncRef,
            minimum: 0,
            maximum: Some(10),
        };
        let f = Function::new_typed(&mut store, |_env: FunctionEnvMut<()>, num: i32| num + 1);
        let table = Table::new(&mut store, table_type, Value::FuncRef(Some(f.clone())))?;
        // Growing to a bigger maximum should return None
        let old_len = table.grow(&mut store, 12, Value::FuncRef(Some(f.clone())));
        assert!(old_len.is_err());

        // Growing to a bigger maximum should return None
        let old_len = table.grow(&mut store, 5, Value::FuncRef(Some(f)))?;
        assert_eq!(old_len, 0);

        Ok(())
    }

    #[test]
    #[ignore]
    fn table_copy() -> Result<()> {
        // TODO: table copy test not yet implemented
        Ok(())
    }

    #[test]
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

    #[test]
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

    #[test]
    fn function_new() -> Result<()> {
        let mut store = Store::default();
        let function = Function::new_typed(&mut store, |_env: FunctionEnvMut<()>| {});
        assert_eq!(
            function.ty(&mut store).clone(),
            FunctionType::new(vec![], vec![])
        );
        let function = Function::new_typed(&mut store, |_env: FunctionEnvMut<()>, _a: i32| {});
        assert_eq!(
            function.ty(&mut store).clone(),
            FunctionType::new(vec![Type::I32], vec![])
        );
        let function = Function::new_typed(
            &mut store,
            |_env: FunctionEnvMut<()>, _a: i32, _b: i64, _c: f32, _d: f64| {},
        );
        assert_eq!(
            function.ty(&mut store).clone(),
            FunctionType::new(vec![Type::I32, Type::I64, Type::F32, Type::F64], vec![])
        );
        let function = Function::new_typed(&mut store, |_env: FunctionEnvMut<()>| -> i32 { 1 });
        assert_eq!(
            function.ty(&mut store).clone(),
            FunctionType::new(vec![], vec![Type::I32])
        );
        let function = Function::new_typed(
            &mut store,
            |_env: FunctionEnvMut<()>| -> (i32, i64, f32, f64) { (1, 2, 3.0, 4.0) },
        );
        assert_eq!(
            function.ty(&mut store).clone(),
            FunctionType::new(vec![], vec![Type::I32, Type::I64, Type::F32, Type::F64])
        );
        Ok(())
    }

    #[test]
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

    #[test]
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

    #[test]
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

    //     #[test]
    //     fn native_function_works() -> Result<()> {
    //         let mut store = Store::default();
    //         let function = Function::new_typed(&mut store, || {});
    //         let typed_function: TypedFunction<(), ()> = function.typed(&mut store).unwrap();
    //         let result = typed_function.call(&mut store);
    //         assert!(result.is_ok());

    //         let function =
    //             Function::new_typed(&mut store, |a: i32| -> i32 { a + 1 });
    //         let typed_function: TypedFunction<i32, i32> = function.typed(&mut store).unwrap();
    //         assert_eq!(typed_function.call(&mut store, 3).unwrap(), 4);

    //         fn rust_abi(a: i32, b: i64, c: f32, d: f64) -> u64 {
    //             (a as u64 * 1000) + (b as u64 * 100) + (c as u64 * 10) + (d as u64)
    //         }
    //         let function = Function::new_typed(&mut store, rust_abi);
    //         let typed_function: TypedFunction<(i32, i64, f32, f64), u64> =
    //             function.typed(&mut store).unwrap();
    //         assert_eq!(typed_function.call(&mut store, 8, 4, 1.5, 5.).unwrap(), 8415);

    //         let function = Function::new_typed(&mut store, || -> i32 { 1 });
    //         let typed_function: TypedFunction<(), i32> = function.typed(&mut store).unwrap();
    //         assert_eq!(typed_function.call(&mut store).unwrap(), 1);

    //         let function = Function::new_typed(&mut store, |_a: i32| {});
    //         let typed_function: TypedFunction<i32, ()> = function.typed(&mut store).unwrap();
    //         assert!(typed_function.call(&mut store, 4).is_ok());

    //         let function =
    //             Function::new_typed(&mut store, || -> (i32, i64, f32, f64) {
    //                 (1, 2, 3.0, 4.0)
    //             });
    //         let typed_function: TypedFunction<(), (i32, i64, f32, f64)> =
    //             function.typed(&mut store).unwrap();
    //         assert_eq!(typed_function.call(&mut store).unwrap(), (1, 2, 3.0, 4.0));

    //         Ok(())
    //     }

    //     #[test]
    //     fn function_outlives_instance() -> Result<()> {
    //         let mut store = Store::default();
    //         let wat = r#"(module
    //   (type $sum_t (func (param i32 i32) (result i32)))
    //   (func $sum_f (type $sum_t) (param $x i32) (param $y i32) (result i32)
    //     local.get $x
    //     local.get $y
    //     i32.add)
    //   (export "sum" (func $sum_f)))
    // "#;

    //         let f = {
    //             let module = Module::new(&store, wat)?;
    //             let instance = Instance::new(&mut store, &module, &imports! {})?;
    //             let f: TypedFunction<(i32, i32), i32> =
    //                 instance.exports.get_typed_function(&mut store, "sum")?;

    //             assert_eq!(f.call(&mut store, 4, 5)?, 9);
    //             f
    //         };

    //         assert_eq!(f.call(&mut store, 4, 5)?, 9);

    //         Ok(())
    //     }
    //     /*
    //         #[test]
    //         fn weak_instance_ref_externs_after_instance() -> Result<()> {
    //             let mut store = Store::default();
    //             let wat = r#"(module
    //       (memory (export "mem") 1)
    //       (type $sum_t (func (param i32 i32) (result i32)))
    //       (func $sum_f (type $sum_t) (param $x i32) (param $y i32) (result i32)
    //         local.get $x
    //         local.get $y
    //         i32.add)
    //       (export "sum" (func $sum_f)))
    //     "#;

    //             let f = {
    //                 let module = Module::new(&store, wat)?;
    //                 let instance = Instance::new(&mut store, &module, &imports! {})?;
    //                 let f: TypedFunction<(i32, i32), i32> =
    //                     instance.exports.get_with_generics_weak("sum")?;

    //                 assert_eq!(f.call(&mut store, 4, 5)?, 9);
    //                 f
    //             };

    //             assert_eq!(f.call(&mut store, 4, 5)?, 9);

    //             Ok(())
    //         }
    //         */
    //     #[test]
    //     fn manually_generate_wasmer_env() -> Result<()> {
    //         let mut store = Store::default();
    //         #[derive(Clone)]
    //         struct MyEnv {
    //             val: u32,
    //             memory: Option<Memory>,
    //         }

    //         fn host_function(env: FunctionEnvMut<MyEnv>, arg1: u32, arg2: u32) -> u32 {
    //             env.data().val + arg1 + arg2
    //         }

    //         let mut env = MyEnv {
    //             val: 5,
    //             memory: None,
    //         };
    //         let env = FunctionEnv::new(&mut store, env);

    //         let result = host_function(ctx.as_context_mut(), 7, 9);
    //         assert_eq!(result, 21);

    //         let memory = Memory::new(&mut store, MemoryType::new(0, None, false))?;
    //         ctx.as_mut(&mut store).memory = Some(memory);

    //         let result = host_function(ctx.as_context_mut(), 1, 2);
    //         assert_eq!(result, 8);

    //         Ok(())
    //     }
}
