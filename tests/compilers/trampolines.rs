wasmer_compilers! {
    use wasmer::*;
    use anyhow::Result;
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

    #[test]
    fn dynamic_function() -> Result<()> {
        static HITS: AtomicUsize = AtomicUsize::new(0);

        let wat = r#"
            (import "" "" (func (param i32)))
            (func $foo
                i32.const 10
                call 0
            )
            (start $foo)
        "#;
        let store = get_store();
        let module = Module::new(&store, &wat)?;
        let func_type = FunctionType::new(vec![ValType::I32], vec![]);
        Instance::new(
            &module,
            &imports! {
                "" => {
                    "" => Function::new_dynamic(&store, &func_type, |params| {
                        assert_eq!(HITS.fetch_add(params[0].unwrap_i32() as _, SeqCst), 0);
                        Ok(vec![])
                    }),
                },
            },
        )?;
        assert_eq!(HITS.load(SeqCst), 10);
        Ok(())
    }

    #[test]
    fn dynamic_function_with_env() -> Result<()> {
        let wat = r#"
            (import "" "" (func (param i32)))

            (func $foo
                i32.const 10
                call 0
            )
            (start $foo)
        "#;
        let store = get_store();
        let module = Module::new(&store, &wat)?;
        struct MyEnv<'a> {
            pub num: i32,
            pub name: &'a str,
        };
        let mut env = MyEnv { num: 2, name: "a" };
        assert_eq!(env.num, 2);
        let func_type = FunctionType::new(vec![ValType::I32], vec![]);
        let imports = imports! {
            "" => {
                "" => Function::new_dynamic_env(&store, &func_type, &mut env, |env: &mut MyEnv, args: &[Val]| {
                    let x = &args[0];
                    assert_eq!(env.num, 2);
                    env.num = x.unwrap_i32();
                    Ok(vec![])
                }),
            },
        };
        Instance::new(&module, &imports)?;
        assert_eq!(env.num, 10);
        Ok(())
    }


    #[test]
    fn native_function() -> Result<()> {
        static HITS: AtomicUsize = AtomicUsize::new(0);

        let wat = r#"
            (import "host" "0" (func))
            (import "host" "1" (func (param i32) (result i32)))
            (import "host" "2" (func (param i32) (param i64)))
            (import "host" "3" (func (param i32 i64 i32 f32 f64)))

            (func $foo
                call 0
                i32.const 0
                call 1
                i32.const 1
                i32.add
                i64.const 3
                call 2

                i32.const 100
                i64.const 200
                i32.const 300
                f32.const 400
                f64.const 500
                call 3
            )
            (start $foo)
        "#;
        let store = get_store();
        let module = Module::new(&store, &wat)?;
        Instance::new(
            &module,
            &imports! {
                "host" => {
                    "0" => Function::new(&store, || {
                        assert_eq!(HITS.fetch_add(1, SeqCst), 0);
                    }),
                    "1" => Function::new(&store, |x: i32| -> i32 {
                        assert_eq!(x, 0);
                        assert_eq!(HITS.fetch_add(1, SeqCst), 1);
                        1
                    }),
                    "2" => Function::new(&store, |x: i32, y: i64| {
                        assert_eq!(x, 2);
                        assert_eq!(y, 3);
                        assert_eq!(HITS.fetch_add(1, SeqCst), 2);
                    }),
                    "3" => Function::new(&store, |a: i32, b: i64, c: i32, d: f32, e: f64| {
                        assert_eq!(a, 100);
                        assert_eq!(b, 200);
                        assert_eq!(c, 300);
                        assert_eq!(d, 400.0);
                        assert_eq!(e, 500.0);
                        assert_eq!(HITS.fetch_add(1, SeqCst), 3);
                    }),
                },
            },
        )?;
        assert_eq!(HITS.load(SeqCst), 4);
        Ok(())
    }

    #[test]
    fn native_function_with_env() -> Result<()> {
        let wat = r#"
            (import "" "" (func (param i32)))

            (func $foo
                i32.const 10
                call 0
            )
            (start $foo)
        "#;
        let store = get_store();
        let module = Module::new(&store, &wat)?;
        struct MyEnv {
            pub num: i32,
        };
        let mut env = MyEnv { num: 2 };
        assert_eq!(env.num, 2);
        Instance::new(
            &module,
            &imports! {
                "" => {
                    "" => Function::new_env(&store, &mut env, |env: &mut MyEnv, x: i32| {
                        assert_eq!(env.num, 2);
                        env.num = x;
                    }),
                },
            },
        )?;
        assert_eq!(env.num, 10);
        Ok(())
    }
}
