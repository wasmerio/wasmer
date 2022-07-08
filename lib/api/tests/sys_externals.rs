#[cfg(feature = "sys")]
mod sys {
    use anyhow::Result;
    use wasmer::Context as WasmerContext;
    use wasmer::*;

    #[test]
    fn global_new() -> Result<()> {
        let mut store = Store::default();
        let mut ctx = WasmerContext::new(&store, ());
        let global = Global::new(&mut ctx, Value::I32(10));
        assert_eq!(
            global.ty(&mut ctx),
            GlobalType {
                ty: Type::I32,
                mutability: Mutability::Const
            }
        );

        let global_mut = Global::new_mut(&mut ctx, Value::I32(10));
        assert_eq!(
            global_mut.ty(&mut ctx),
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
        let mut ctx = WasmerContext::new(&store, ());
        let global_i32 = Global::new(&mut ctx, Value::I32(10));
        assert_eq!(global_i32.get(&mut ctx), Value::I32(10));
        let global_i64 = Global::new(&mut ctx, Value::I64(20));
        assert_eq!(global_i64.get(&mut ctx), Value::I64(20));
        let global_f32 = Global::new(&mut ctx, Value::F32(10.0));
        assert_eq!(global_f32.get(&mut ctx), Value::F32(10.0));
        let global_f64 = Global::new(&mut ctx, Value::F64(20.0));
        assert_eq!(global_f64.get(&mut ctx), Value::F64(20.0));

        Ok(())
    }

    #[test]
    fn global_set() -> Result<()> {
        let mut store = Store::default();
        let mut ctx = WasmerContext::new(&store, ());
        let global_i32 = Global::new(&mut ctx, Value::I32(10));
        // Set on a constant should error
        assert!(global_i32.set(&mut ctx, Value::I32(20)).is_err());

        let global_i32_mut = Global::new_mut(&mut ctx, Value::I32(10));
        // Set on different type should error
        assert!(global_i32_mut.set(&mut ctx, Value::I64(20)).is_err());

        // Set on same type should succeed
        global_i32_mut.set(&mut ctx, Value::I32(20))?;
        assert_eq!(global_i32_mut.get(&mut ctx), Value::I32(20));

        Ok(())
    }

    #[test]
    fn table_new() -> Result<()> {
        let mut store = Store::default();
        let mut ctx = WasmerContext::new(&store, ());
        let table_type = TableType {
            ty: Type::FuncRef,
            minimum: 0,
            maximum: None,
        };
        let f = Function::new_native(&mut ctx, |_ctx: FunctionEnvMut<()>| {});
        let table = Table::new(&mut ctx, table_type, Value::FuncRef(Some(f)))?;
        assert_eq!(table.ty(&mut ctx), table_type);

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
        let mut ctx = WasmerContext::new(&store, ());
        let table_type = TableType {
            ty: Type::FuncRef,
            minimum: 0,
            maximum: Some(1),
        };
        let f = Function::new_native(&mut ctx, |_ctx: FunctionEnvMut<()>, num: i32| num + 1);
        let table = Table::new(&mut ctx, table_type, Value::FuncRef(Some(f)))?;
        assert_eq!(table.ty(&mut ctx), table_type);
        let _elem = table.get(&mut ctx, 0).unwrap();
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
        let mut ctx = WasmerContext::new(&store, ());
        let table_type = TableType {
            ty: Type::FuncRef,
            minimum: 0,
            maximum: Some(10),
        };
        let f = Function::new_native(&mut ctx, |_ctx: FunctionEnvMut<()>, num: i32| num + 1);
        let table = Table::new(&mut ctx, table_type, Value::FuncRef(Some(f.clone())))?;
        // Growing to a bigger maximum should return None
        let old_len = table.grow(&mut ctx, 12, Value::FuncRef(Some(f.clone())));
        assert!(old_len.is_err());

        // Growing to a bigger maximum should return None
        let old_len = table.grow(&mut ctx, 5, Value::FuncRef(Some(f)))?;
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
        let mut ctx = WasmerContext::new(&store, ());
        let memory_type = MemoryType {
            shared: false,
            minimum: Pages(0),
            maximum: Some(Pages(10)),
        };
        let memory = Memory::new(&mut ctx, memory_type)?;
        assert_eq!(memory.size(&mut ctx), Pages(0));
        assert_eq!(memory.ty(&mut ctx), memory_type);
        Ok(())
    }

    #[test]
    fn memory_grow() -> Result<()> {
        let mut store = Store::default();
        let mut ctx = WasmerContext::new(&store, ());
        let desc = MemoryType::new(Pages(10), Some(Pages(16)), false);
        let memory = Memory::new(&mut ctx, desc)?;
        assert_eq!(memory.size(&mut ctx), Pages(10));

        let result = memory.grow(&mut ctx, Pages(2)).unwrap();
        assert_eq!(result, Pages(10));
        assert_eq!(memory.size(&mut ctx), Pages(12));

        let result = memory.grow(&mut ctx, Pages(10));
        assert_eq!(
            result,
            Err(MemoryError::CouldNotGrow {
                current: 12.into(),
                attempted_delta: 10.into()
            })
        );

        let bad_desc = MemoryType::new(Pages(15), Some(Pages(10)), false);
        let bad_result = Memory::new(&mut ctx, bad_desc);

        assert!(matches!(bad_result, Err(MemoryError::InvalidMemory { .. })));

        Ok(())
    }

    #[test]
    fn function_new() -> Result<()> {
        let mut store = Store::default();
        let mut ctx = WasmerContext::new(&store, ());
        let function = Function::new_native(&mut ctx, |_ctx: FunctionEnvMut<_>| {});
        assert_eq!(
            function.ty(&mut ctx).clone(),
            FunctionType::new(vec![], vec![])
        );
        let function = Function::new_native(&mut ctx, |_ctx: FunctionEnvMut<_>, _a: i32| {});
        assert_eq!(
            function.ty(&mut ctx).clone(),
            FunctionType::new(vec![Type::I32], vec![])
        );
        let function = Function::new_native(
            &mut ctx,
            |_ctx: FunctionEnvMut<_>, _a: i32, _b: i64, _c: f32, _d: f64| {},
        );
        assert_eq!(
            function.ty(&mut ctx).clone(),
            FunctionType::new(vec![Type::I32, Type::I64, Type::F32, Type::F64], vec![])
        );
        let function = Function::new_native(&mut ctx, |_ctx: FunctionEnvMut<_>| -> i32 { 1 });
        assert_eq!(
            function.ty(&mut ctx).clone(),
            FunctionType::new(vec![], vec![Type::I32])
        );
        let function =
            Function::new_native(&mut ctx, |_ctx: FunctionEnvMut<_>| -> (i32, i64, f32, f64) {
                (1, 2, 3.0, 4.0)
            });
        assert_eq!(
            function.ty(&mut ctx).clone(),
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
        let mut ctx = WasmerContext::new(&store, my_env);
        let function = Function::new_native(&mut ctx, |_ctx: FunctionEnvMut<MyEnv>| {});
        assert_eq!(
            function.ty(&mut ctx).clone(),
            FunctionType::new(vec![], vec![])
        );
        let function = Function::new_native(&mut ctx, |_ctx: FunctionEnvMut<MyEnv>, _a: i32| {});
        assert_eq!(
            function.ty(&mut ctx).clone(),
            FunctionType::new(vec![Type::I32], vec![])
        );
        let function = Function::new_native(
            &mut ctx,
            |_ctx: FunctionEnvMut<MyEnv>, _a: i32, _b: i64, _c: f32, _d: f64| {},
        );
        assert_eq!(
            function.ty(&mut ctx).clone(),
            FunctionType::new(vec![Type::I32, Type::I64, Type::F32, Type::F64], vec![])
        );
        let function = Function::new_native(&mut ctx, |_ctx: FunctionEnvMut<MyEnv>| -> i32 { 1 });
        assert_eq!(
            function.ty(&mut ctx).clone(),
            FunctionType::new(vec![], vec![Type::I32])
        );
        let function = Function::new_native(
            &mut ctx,
            |_ctx: FunctionEnvMut<MyEnv>| -> (i32, i64, f32, f64) { (1, 2, 3.0, 4.0) },
        );
        assert_eq!(
            function.ty(&mut ctx).clone(),
            FunctionType::new(vec![], vec![Type::I32, Type::I64, Type::F32, Type::F64])
        );
        Ok(())
    }

    #[test]
    fn function_new_dynamic() -> Result<()> {
        let mut store = Store::default();
        let mut ctx = WasmerContext::new(&store, ());

        // Using &FunctionType signature
        let function_type = FunctionType::new(vec![], vec![]);
        let function = Function::new(
            &mut ctx,
            &function_type,
            |_ctx: FunctionEnvMut<()>, _values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&mut ctx).clone(), function_type);
        let function_type = FunctionType::new(vec![Type::I32], vec![]);
        let function = Function::new(
            &mut ctx,
            &function_type,
            |_ctx: FunctionEnvMut<()>, _values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&mut ctx).clone(), function_type);
        let function_type =
            FunctionType::new(vec![Type::I32, Type::I64, Type::F32, Type::F64], vec![]);
        let function = Function::new(
            &mut ctx,
            &function_type,
            |_ctx: FunctionEnvMut<()>, _values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&mut ctx).clone(), function_type);
        let function_type = FunctionType::new(vec![], vec![Type::I32]);
        let function = Function::new(
            &mut ctx,
            &function_type,
            |_ctx: FunctionEnvMut<()>, _values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&mut ctx).clone(), function_type);
        let function_type =
            FunctionType::new(vec![], vec![Type::I32, Type::I64, Type::F32, Type::F64]);
        let function = Function::new(
            &mut ctx,
            &function_type,
            |_ctx: FunctionEnvMut<()>, _values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&mut ctx).clone(), function_type);

        // Using array signature
        let function_type = ([Type::V128], [Type::I32, Type::F32, Type::F64]);
        let function = Function::new(
            &mut ctx,
            function_type,
            |_ctx: FunctionEnvMut<()>, _values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&mut ctx).params(), [Type::V128]);
        assert_eq!(
            function.ty(&mut ctx).results(),
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
        let mut ctx = WasmerContext::new(&store, my_env);

        // Using &FunctionType signature
        let function_type = FunctionType::new(vec![], vec![]);
        let function = Function::new(
            &mut ctx,
            &function_type,
            |_ctx: FunctionEnvMut<MyEnv>, _values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&mut ctx).clone(), function_type);
        let function_type = FunctionType::new(vec![Type::I32], vec![]);
        let function = Function::new(
            &mut ctx,
            &function_type,
            |_ctx: FunctionEnvMut<MyEnv>, _values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&mut ctx).clone(), function_type);
        let function_type =
            FunctionType::new(vec![Type::I32, Type::I64, Type::F32, Type::F64], vec![]);
        let function = Function::new(
            &mut ctx,
            &function_type,
            |_ctx: FunctionEnvMut<MyEnv>, _values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&mut ctx).clone(), function_type);
        let function_type = FunctionType::new(vec![], vec![Type::I32]);
        let function = Function::new(
            &mut ctx,
            &function_type,
            |_ctx: FunctionEnvMut<MyEnv>, _values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&mut ctx).clone(), function_type);
        let function_type =
            FunctionType::new(vec![], vec![Type::I32, Type::I64, Type::F32, Type::F64]);
        let function = Function::new(
            &mut ctx,
            &function_type,
            |_ctx: FunctionEnvMut<MyEnv>, _values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&mut ctx).clone(), function_type);

        // Using array signature
        let function_type = ([Type::V128], [Type::I32, Type::F32, Type::F64]);
        let function = Function::new(
            &mut ctx,
            function_type,
            |_ctx: FunctionEnvMut<MyEnv>, _values: &[Value]| unimplemented!(),
        );
        assert_eq!(function.ty(&mut ctx).params(), [Type::V128]);
        assert_eq!(
            function.ty(&mut ctx).results(),
            [Type::I32, Type::F32, Type::F64]
        );

        Ok(())
    }

    #[test]
    fn native_function_works() -> Result<()> {
        let mut store = Store::default();
        let mut ctx = WasmerContext::new(&store, ());
        let function = Function::new_native(&mut ctx, |_ctx: FunctionEnvMut<()>| {});
        let native_function: TypedFunction<(), ()> = function.native(&mut ctx).unwrap();
        let result = native_function.call(&mut ctx);
        assert!(result.is_ok());

        let function =
            Function::new_native(&mut ctx, |_ctx: FunctionEnvMut<()>, a: i32| -> i32 { a + 1 });
        let native_function: TypedFunction<i32, i32> = function.native(&mut ctx).unwrap();
        assert_eq!(native_function.call(&mut ctx, 3).unwrap(), 4);

        fn rust_abi(_ctx: FunctionEnvMut<()>, a: i32, b: i64, c: f32, d: f64) -> u64 {
            (a as u64 * 1000) + (b as u64 * 100) + (c as u64 * 10) + (d as u64)
        }
        let function = Function::new_native(&mut ctx, rust_abi);
        let native_function: TypedFunction<(i32, i64, f32, f64), u64> =
            function.native(&mut ctx).unwrap();
        assert_eq!(native_function.call(&mut ctx, 8, 4, 1.5, 5.).unwrap(), 8415);

        let function = Function::new_native(&mut ctx, |_ctx: FunctionEnvMut<()>| -> i32 { 1 });
        let native_function: TypedFunction<(), i32> = function.native(&mut ctx).unwrap();
        assert_eq!(native_function.call(&mut ctx).unwrap(), 1);

        let function = Function::new_native(&mut ctx, |_ctx: FunctionEnvMut<()>, _a: i32| {});
        let native_function: TypedFunction<i32, ()> = function.native(&mut ctx).unwrap();
        assert!(native_function.call(&mut ctx, 4).is_ok());

        let function =
            Function::new_native(&mut ctx, |_ctx: FunctionEnvMut<()>| -> (i32, i64, f32, f64) {
                (1, 2, 3.0, 4.0)
            });
        let native_function: TypedFunction<(), (i32, i64, f32, f64)> =
            function.native(&mut ctx).unwrap();
        assert_eq!(native_function.call(&mut ctx).unwrap(), (1, 2, 3.0, 4.0));

        Ok(())
    }

    #[test]
    fn function_outlives_instance() -> Result<()> {
        let mut store = Store::default();
        let mut ctx = WasmerContext::new(&store, ());
        let wat = r#"(module
  (type $sum_t (func (param i32 i32) (result i32)))
  (func $sum_f (type $sum_t) (param $x i32) (param $y i32) (result i32)
    local.get $x
    local.get $y
    i32.add)
  (export "sum" (func $sum_f)))
"#;

        let f = {
            let module = Module::new(&store, wat)?;
            let instance = Instance::new(&mut ctx, &module, &imports! {})?;
            let f: TypedFunction<(i32, i32), i32> =
                instance.exports.get_typed_function(&mut ctx, "sum")?;

            assert_eq!(f.call(&mut ctx, 4, 5)?, 9);
            f
        };

        assert_eq!(f.call(&mut ctx, 4, 5)?, 9);

        Ok(())
    }
    /*
        #[test]
        fn weak_instance_ref_externs_after_instance() -> Result<()> {
            let mut store = Store::default();
            let mut ctx = WasmerContext::new(&store, ());
            let wat = r#"(module
      (memory (export "mem") 1)
      (type $sum_t (func (param i32 i32) (result i32)))
      (func $sum_f (type $sum_t) (param $x i32) (param $y i32) (result i32)
        local.get $x
        local.get $y
        i32.add)
      (export "sum" (func $sum_f)))
    "#;

            let f = {
                let module = Module::new(&store, wat)?;
                let instance = Instance::new(&mut ctx, &module, &imports! {})?;
                let f: TypedFunction<(i32, i32), i32> =
                    instance.exports.get_with_generics_weak("sum")?;

                assert_eq!(f.call(&mut ctx, 4, 5)?, 9);
                f
            };

            assert_eq!(f.call(&mut ctx, 4, 5)?, 9);

            Ok(())
        }
        */
    #[test]
    fn manually_generate_wasmer_env() -> Result<()> {
        let mut store = Store::default();
        #[derive(Clone)]
        struct MyEnv {
            val: u32,
            memory: Option<Memory>,
        }

        fn host_function(ctx: FunctionEnvMut<MyEnv>, arg1: u32, arg2: u32) -> u32 {
            ctx.data().val + arg1 + arg2
        }

        let mut env = MyEnv {
            val: 5,
            memory: None,
        };
        let mut ctx = WasmerContext::new(&store, env);

        let result = host_function(ctx.as_store_mut(), 7, 9);
        assert_eq!(result, 21);

        let memory = Memory::new(&mut ctx, MemoryType::new(0, None, false))?;
        ctx.data_mut().memory = Some(memory);

        let result = host_function(ctx.as_store_mut(), 1, 2);
        assert_eq!(result, 8);

        Ok(())
    }
}
