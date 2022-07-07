#[cfg(feature = "js")]
mod js {
    use anyhow::Result;
    use wasm_bindgen_test::*;
    use wasmer::*;

    /*
     * For debugging, put web_sys in dev dependencies in Cargo.toml with the "console" feature
     * on.
     *
    extern crate web_sys;

    // A macro to provide `println!(..)`-style syntax for `console.log` logging.
    macro_rules! log {
        ( $( $t:tt )* ) => {
        web_sys::console::log_1(&format!( $( $t )* ).into());
        }
    }
    */
    macro_rules! log {
        ( $( $t:tt )* ) => {};
    }

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
        module
            .set_type_hints(ModuleTypeHints {
                imports: vec![],
                exports: vec![ExternType::Memory(MemoryType::new(Pages(1), None, false))],
            })
            .unwrap();

        let import_object = imports! {};
        let mut ctx = Context::new(&store, ());
        let instance = Instance::new(&mut ctx, &module, &import_object).unwrap();

        let memory = instance.exports.get_memory("mem").unwrap();
        assert!(memory.is_from_store(&ctx));
        assert_eq!(memory.ty(&ctx), MemoryType::new(Pages(1), None, false));
        assert_eq!(memory.size(&ctx), Pages(1));
        assert_eq!(memory.data_size(&ctx), 65536);

        memory.grow(&mut ctx, Pages(1)).unwrap();
        assert_eq!(memory.ty(&ctx), MemoryType::new(Pages(1), None, false));
        assert_eq!(memory.size(&ctx), Pages(2));
        assert_eq!(memory.data_size(&ctx), 65536 * 2);
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
        module
            .set_type_hints(ModuleTypeHints {
                imports: vec![],
                exports: vec![ExternType::Function(FunctionType::new(
                    vec![],
                    vec![Type::I32],
                ))],
            })
            .unwrap();

        let import_object = imports! {};
        let mut ctx = Context::new(&store, ());
        let instance = Instance::new(&mut ctx, &module, &import_object).unwrap();

        let get_magic = instance.exports.get_function("get_magic").unwrap();
        assert_eq!(
            get_magic.ty(&ctx).clone(),
            FunctionType::new(vec![], vec![Type::I32])
        );

        let expected = vec![Val::I32(42)].into_boxed_slice();
        assert_eq!(get_magic.call(&mut ctx, &[]).unwrap(), expected);
    }

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
        module
            .set_type_hints(ModuleTypeHints {
                imports: vec![ExternType::Function(FunctionType::new(
                    vec![Type::I32],
                    vec![Type::I32],
                ))],
                exports: vec![ExternType::Function(FunctionType::new(
                    vec![Type::I32],
                    vec![Type::I32],
                ))],
            })
            .unwrap();
        let mut ctx = Context::new(&store, ());

        let imported_signature = FunctionType::new(vec![Type::I32], vec![Type::I32]);

        let imported = Function::new(&mut ctx, imported_signature, |_env, args| {
            log!("Calling `imported`...");
            let result = args[0].unwrap_i32() * 2;
            log!("Result of `imported`: {:?}", result);
            Ok(vec![Value::I32(result)])
        });

        let import_object = imports! {
            "env" => {
                "imported" => imported,
            }
        };
        let instance = Instance::new(&mut ctx, &module, &import_object).unwrap();

        let exported = instance.exports.get_function("exported").unwrap();

        let expected = vec![Val::I32(6)].into_boxed_slice();
        assert_eq!(exported.call(&mut ctx, &[Val::I32(3)]).unwrap(), expected);
    }

    // We comment it for now because in old versions of Node, only single return values are supported

    // #[wasm_bindgen_test]
    // fn test_imported_function_dynamic_multivalue() {
    //     let store = Store::default();
    //     let mut module = Module::new(
    //         &store,
    //         br#"
    //     (module
    //         (func $multivalue (import "env" "multivalue") (param i32 i32) (result i32 i32))
    //         (func (export "multivalue") (param i32 i32) (result i32 i32)
    //             (call $multivalue (local.get 0) (local.get 1))
    //         )
    //     )
    //     "#,
    //     )
    //     .unwrap();
    //     module.set_type_hints(ModuleTypeHints {
    //         imports: vec![
    //             ExternType::Function(FunctionType::new(
    //                 vec![Type::I32, Type::I32],
    //                 vec![Type::I32, Type::I32],
    //             )),
    //         ],
    //         exports: vec![
    //             ExternType::Function(FunctionType::new(
    //                 vec![Type::I32, Type::I32],
    //                 vec![Type::I32, Type::I32],
    //             )),
    //         ],
    //     });

    //     let multivalue_signature =
    //         FunctionType::new(vec![Type::I32, Type::I32], vec![Type::I32, Type::I32]);
    //     let multivalue = Function::new(&store, &multivalue_signature, |args| {
    //         log!("Calling `imported`...");
    //         // let result = args[0].unwrap_i32() * ;
    //         // log!("Result of `imported`: {:?}", result);
    //         Ok(vec![args[1].clone(), args[0].clone()])
    //     });

    //     let import_object = imports! {
    //         "env" => {
    //             "multivalue" => multivalue,
    //         }
    //     };
    //     let instance = Instance::new(&module, &import_object).unwrap();

    //     let exported_multivalue = instance
    //         .exports
    //         .get_function("multivalue")
    //         .unwrap();

    //     let expected = vec![Val::I32(2), Val::I32(3)].into_boxed_slice();
    //     assert_eq!(
    //         exported_multivalue.call(&[Val::I32(3), Val::I32(2)]),
    //         Ok(expected)
    //     );
    // }

    #[wasm_bindgen_test]
    fn test_imported_function_dynamic_with_env() {
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
        module
            .set_type_hints(ModuleTypeHints {
                imports: vec![ExternType::Function(FunctionType::new(
                    vec![Type::I32],
                    vec![Type::I32],
                ))],
                exports: vec![ExternType::Function(FunctionType::new(
                    vec![Type::I32],
                    vec![Type::I32],
                ))],
            })
            .unwrap();

        #[derive(Clone)]
        struct Env {
            multiplier: i32,
        }

        let mut ctx = Context::new(&store, Env { multiplier: 3 });

        let imported_signature = FunctionType::new(vec![Type::I32], vec![Type::I32]);
        let imported = Function::new(&mut ctx, &imported_signature, |ctx, args| {
            log!("Calling `imported`...");
            let result = args[0].unwrap_i32() * ctx.data().multiplier;
            log!("Result of `imported`: {:?}", result);
            Ok(vec![Value::I32(result)])
        });

        let import_object = imports! {
            "env" => {
                "imported" => imported,
            }
        };
        let instance = Instance::new(&mut ctx, &module, &import_object).unwrap();

        let exported = instance.exports.get_function("exported").unwrap();

        let expected = vec![Val::I32(9)].into_boxed_slice();
        assert_eq!(exported.call(&mut ctx, &[Val::I32(3)]), Ok(expected));
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
        module
            .set_type_hints(ModuleTypeHints {
                imports: vec![ExternType::Function(FunctionType::new(
                    vec![Type::I32],
                    vec![Type::I32],
                ))],
                exports: vec![ExternType::Function(FunctionType::new(
                    vec![Type::I32],
                    vec![Type::I32],
                ))],
            })
            .unwrap();

        fn imported_fn(_: ContextMut<'_, ()>, arg: u32) -> u32 {
            return arg + 1;
        }

        let mut ctx = Context::new(&store, ());
        let imported = Function::new_native(&mut ctx, imported_fn);

        let import_object = imports! {
            "env" => {
                "imported" => imported,
            }
        };
        let instance = Instance::new(&mut ctx, &module, &import_object).unwrap();

        let exported = instance.exports.get_function("exported").unwrap();

        let expected = vec![Val::I32(5)].into_boxed_slice();
        assert_eq!(exported.call(&mut ctx, &[Val::I32(4)]), Ok(expected));
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
        module
            .set_type_hints(ModuleTypeHints {
                imports: vec![ExternType::Function(FunctionType::new(
                    vec![Type::I32],
                    vec![Type::I32],
                ))],
                exports: vec![ExternType::Function(FunctionType::new(
                    vec![Type::I32],
                    vec![Type::I32],
                ))],
            })
            .unwrap();

        #[derive(Clone, Debug)]
        struct Env {
            multiplier: u32,
        }

        fn imported_fn(ctx: ContextMut<'_, Env>, arg: u32) -> u32 {
            log!("inside imported_fn: ctx.data is {:?}", ctx.data());
            // log!("inside call id is {:?}", store.objects().id);
            return ctx.data().multiplier * arg;
        }

        let mut ctx = Context::new(&store, Env { multiplier: 3 });

        let imported = Function::new_native(&mut ctx, imported_fn);

        let import_object = imports! {
            "env" => {
                "imported" => imported,
            }
        };
        let instance = Instance::new(&mut ctx, &module, &import_object).unwrap();

        let exported = instance.exports.get_function("exported").unwrap();

        let expected = vec![Val::I32(12)].into_boxed_slice();
        ctx.data_mut().multiplier = 3;
        assert_eq!(exported.call(&mut ctx, &[Val::I32(4)]), Ok(expected));
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
        module
            .set_type_hints(ModuleTypeHints {
                imports: vec![ExternType::Function(FunctionType::new(
                    vec![Type::I32],
                    vec![Type::I32],
                ))],
                exports: vec![
                    ExternType::Function(FunctionType::new(vec![Type::I32], vec![Type::I32])),
                    ExternType::Memory(MemoryType::new(Pages(1), None, false)),
                ],
            })
            .unwrap();

        #[derive(Clone, Debug)]
        struct Env {
            multiplier: u32,
            memory: Option<Memory>,
        }

        fn imported_fn(ctx: ContextMut<'_, Env>, arg: u32) -> u32 {
            let memory: &Memory = ctx.data().memory.as_ref().unwrap();
            let memory_val = memory.uint8view(&ctx).get_index(0);
            return (memory_val as u32) * ctx.data().multiplier * arg;
        }

        let mut ctx = Context::new(
            &store,
            Env {
                multiplier: 3,
                memory: None,
            },
        );
        let imported = Function::new_native(&mut ctx, imported_fn);

        let import_object = imports! {
            "env" => {
                "imported" => imported,
            }
        };
        let instance = Instance::new(&mut ctx, &module, &import_object).unwrap();

        let memory = instance.exports.get_memory("memory").unwrap();
        assert_eq!(memory.data_size(&ctx), 65536);
        let memory_val = memory.uint8view(&ctx).get_index(0);
        assert_eq!(memory_val, 0);

        memory.uint8view(&ctx).set_index(0, 2);
        let memory_val = memory.uint8view(&ctx).get_index(0);
        assert_eq!(memory_val, 2);

        ctx.data_mut().memory = Some(memory.clone());

        let exported = instance.exports.get_function("exported").unwrap();

        // It works with the provided memory
        let expected = vec![Val::I32(24)].into_boxed_slice();
        assert_eq!(exported.call(&mut ctx, &[Val::I32(4)]), Ok(expected));

        // It works if we update the memory
        memory.uint8view(&ctx).set_index(0, 3);
        let expected = vec![Val::I32(36)].into_boxed_slice();
        assert_eq!(exported.call(&mut ctx, &[Val::I32(4)]), Ok(expected));
    }

    #[wasm_bindgen_test]
    fn test_unit_native_function_env() {
        let store = Store::default();
        #[derive(Clone)]
        struct Env {
            multiplier: u32,
        }

        let mut ctx = Context::new(&store, Env { multiplier: 3 });

        fn imported_fn(ctx: ContextMut<'_, Env>, args: &[Val]) -> Result<Vec<Val>, RuntimeError> {
            let value = ctx.data().multiplier * args[0].unwrap_i32() as u32;
            return Ok(vec![Val::I32(value as _)]);
        }

        let imported_signature = FunctionType::new(vec![Type::I32], vec![Type::I32]);
        let imported = Function::new(&mut ctx, imported_signature, imported_fn);

        let expected = vec![Val::I32(12)].into_boxed_slice();
        assert_eq!(imported.call(&mut ctx, &[Val::I32(4)]), Ok(expected));
    }

    #[wasm_bindgen_test]
    fn test_imported_function_with_wasmer_env() {
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
        module
            .set_type_hints(ModuleTypeHints {
                imports: vec![ExternType::Function(FunctionType::new(
                    vec![Type::I32],
                    vec![Type::I32],
                ))],
                exports: vec![
                    ExternType::Function(FunctionType::new(vec![Type::I32], vec![Type::I32])),
                    ExternType::Memory(MemoryType::new(Pages(1), None, false)),
                ],
            })
            .unwrap();

        #[derive(Clone, Debug)]
        struct Env {
            multiplier: u32,
            memory: Option<Memory>,
        }

        fn imported_fn(ctx: ContextMut<'_, Env>, args: &[Val]) -> Result<Vec<Val>, RuntimeError> {
            let memory: &Memory = ctx.data().memory.as_ref().unwrap();
            let memory_val = memory.uint8view(&ctx).get_index(0);
            let value = (memory_val as u32) * ctx.data().multiplier * args[0].unwrap_i32() as u32;
            return Ok(vec![Val::I32(value as _)]);
        }

        let mut ctx = Context::new(
            &store,
            Env {
                multiplier: 3,
                memory: None,
            },
        );

        let imported_signature = FunctionType::new(vec![Type::I32], vec![Type::I32]);
        let imported = Function::new(&mut ctx, imported_signature, imported_fn);

        let import_object = imports! {
            "env" => {
                "imported" => imported,
            }
        };
        let instance = Instance::new(&mut ctx, &module, &import_object).unwrap();

        let memory = instance.exports.get_memory("memory").unwrap();
        assert_eq!(memory.data_size(&ctx), 65536);
        let memory_val = memory.uint8view(&ctx).get_index(0);
        assert_eq!(memory_val, 0);

        memory.uint8view(&ctx).set_index(0, 2);
        let memory_val = memory.uint8view(&ctx).get_index(0);
        assert_eq!(memory_val, 2);

        ctx.data_mut().memory = Some(memory.clone());

        let exported = instance.exports.get_function("exported").unwrap();

        // It works with the provided memory
        let expected = vec![Val::I32(24)].into_boxed_slice();
        assert_eq!(exported.call(&mut ctx, &[Val::I32(4)]), Ok(expected));

        // It works if we update the memory
        memory.uint8view(&ctx).set_index(0, 3);
        let expected = vec![Val::I32(36)].into_boxed_slice();
        assert_eq!(exported.call(&mut ctx, &[Val::I32(4)]), Ok(expected));
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
        module
            .set_type_hints(ModuleTypeHints {
                imports: vec![ExternType::Global(GlobalType::new(
                    ValType::I32,
                    Mutability::Var,
                ))],
                exports: vec![
                    ExternType::Function(FunctionType::new(vec![], vec![Type::I32])),
                    ExternType::Function(FunctionType::new(vec![], vec![])),
                ],
            })
            .unwrap();
        let mut ctx = Context::new(&store, ());
        let global = Global::new_mut(&mut ctx, Value::I32(0));
        let import_object = imports! {
            "" => {
                "global" => global.clone()
            }
        };
        let instance = Instance::new(&mut ctx, &module, &import_object).unwrap();

        let get_global = instance.exports.get_function("getGlobal").unwrap();
        assert_eq!(
            get_global.call(&mut ctx, &[]),
            Ok(vec![Val::I32(0)].into_boxed_slice())
        );

        global.set(&mut ctx, Value::I32(42)).unwrap();
        assert_eq!(
            get_global.call(&mut ctx, &[]),
            Ok(vec![Val::I32(42)].into_boxed_slice())
        );

        let inc_global = instance.exports.get_function("incGlobal").unwrap();
        inc_global.call(&mut ctx, &[]).unwrap();
        assert_eq!(
            get_global.call(&mut ctx, &[]),
            Ok(vec![Val::I32(43)].into_boxed_slice())
        );
        assert_eq!(global.get(&ctx), Val::I32(43));
    }

    #[wasm_bindgen_test]
    fn test_native_function() {
        let store = Store::default();
        let module = Module::new(
            &store,
            br#"(module
                (func $add (import "env" "sum") (param i32 i32) (result i32))
                (func (export "add_one") (param i32) (result i32)
                    (call $add (local.get 0) (i32.const 1))
                )
            )"#,
        )
        .unwrap();

        fn sum(_: ContextMut<'_, ()>, a: i32, b: i32) -> i32 {
            a + b
        }

        let mut ctx = Context::new(&store, ());

        let import_object = imports! {
            "env" => {
                "sum" => Function::new_native(&mut ctx, sum),
            }
        };

        let instance = Instance::new(&mut ctx, &module, &import_object).unwrap();

        let add_one: TypedFunction<i32, i32> = instance
            .exports
            .get_typed_function(&ctx, "add_one")
            .unwrap();
        assert_eq!(add_one.call(&mut ctx, 1), Ok(2));
    }

    #[wasm_bindgen_test]
    fn test_panic() {
        let store = Store::default();
        let module = Module::new(
            &store,
            br#"
    (module
      (type $run_t (func (param i32 i32) (result i32)))
      (type $early_exit_t (func (param) (result)))
      (import "env" "early_exit" (func $early_exit (type $early_exit_t)))
      (func $run (type $run_t) (param $x i32) (param $y i32) (result i32)
        (call $early_exit)
        (i32.add
            local.get $x
            local.get $y))
      (export "run" (func $run)))
    "#,
        )
        .unwrap();

        fn early_exit(_: ContextMut<'_, ()>) {
            panic!("Do panic")
        }
        let mut ctx = Context::new(&store, ());

        let import_object = imports! {
            "env" => {
                "early_exit" => Function::new_native(&mut ctx, early_exit),
            }
        };
        let instance = Instance::new(&mut ctx, &module, &import_object).unwrap();

        let run_func: TypedFunction<(i32, i32), i32> =
            instance.exports.get_typed_function(&ctx, "run").unwrap();

        assert!(
            run_func.call(&mut ctx, 1, 7).is_err(),
            "Expected early termination",
        );
        let run_func = instance.exports.get_function("run").unwrap();

        assert!(
            run_func
                .call(&mut ctx, &[Val::I32(1), Val::I32(7)])
                .is_err(),
            "Expected early termination",
        );
    }

    #[wasm_bindgen_test]
    fn test_custom_error() {
        let store = Store::default();
        let module = Module::new(
            &store,
            br#"
    (module
      (type $run_t (func (param i32 i32) (result i32)))
      (type $early_exit_t (func (param) (result)))
      (import "env" "early_exit" (func $early_exit (type $early_exit_t)))
      (func $run (type $run_t) (param $x i32) (param $y i32) (result i32)
        (call $early_exit)
        (i32.add
            local.get $x
            local.get $y))
      (export "run" (func $run)))
    "#,
        )
        .unwrap();

        let mut ctx = Context::new(&store, ());

        use std::fmt;

        #[derive(Debug, Clone, Copy)]
        struct ExitCode(u32);

        impl fmt::Display for ExitCode {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl std::error::Error for ExitCode {}

        fn early_exit(_: ContextMut<'_, ()>) -> Result<(), ExitCode> {
            Err(ExitCode(1))
        }

        let import_object = imports! {
            "env" => {
                "early_exit" => Function::new_native(&mut ctx, early_exit),
            }
        };

        let instance = Instance::new(&mut ctx, &module, &import_object).unwrap();

        fn test_result<T: core::fmt::Debug>(result: Result<T, RuntimeError>) {
            match result {
                Ok(result) => {
                    assert!(
                        false,
                        "Expected early termination with `ExitCode`, found: {:?}",
                        result
                    );
                }
                Err(e) => {
                    match e.downcast::<ExitCode>() {
                        // We found the exit code used to terminate execution.
                        Ok(exit_code) => {
                            assert_eq!(exit_code.0, 1);
                        }
                        Err(e) => {
                            assert!(false, "Unknown error `{:?}` found. expected `ErrorCode`", e);
                        }
                    }
                }
            }
        }

        let run_func: TypedFunction<(i32, i32), i32> =
            instance.exports.get_typed_function(&ctx, "run").unwrap();
        test_result(run_func.call(&mut ctx, 1, 7));

        let run_func = instance.exports.get_function("run").unwrap();
        test_result(run_func.call(&mut ctx, &[Val::I32(1), Val::I32(7)]));
    }

    #[wasm_bindgen_test]
    fn test_start_function_fails() {
        let store = Store::default();
        let module = Module::new(
            &store,
            br#"
    (module
        (func $start_function
            (i32.div_u
                (i32.const 1)
                (i32.const 0)
            )
            drop
        )
        (start $start_function)
    )
    "#,
        )
        .unwrap();

        let import_object = imports! {};
        let mut ctx = Context::new(&store, ());
        let result = Instance::new(&mut ctx, &module, &import_object);
        let err = result.unwrap_err();
        assert!(format!("{:?}", err).contains("zero"))
    }
}
