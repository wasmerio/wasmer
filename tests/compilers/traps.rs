use anyhow::Result;
use std::panic::{self, AssertUnwindSafe};
use wasmer::*;

#[compiler_test(traps)]
fn test_trap_return(config: crate::Config) -> Result<()> {
    let mut store = config.store();
    let wat = r#"
        (module
        (func $hello (import "" "hello"))
        (func (export "run") (call $hello))
        )
    "#;

    let module = Module::new(&store, wat)?;
    let hello_type = FunctionType::new(vec![], vec![]);
    let hello_func = Function::new(&mut store, &hello_type, |_| {
        Err(RuntimeError::new("test 123"))
    });

    let instance = Instance::new(
        &mut store,
        &module,
        &imports! {
            "" => {
                "hello" => hello_func
            }
        },
    )?;
    let run_func = instance
        .exports
        .get_function("run")
        .expect("expected function export");

    let e = run_func
        .call(&mut store, &[])
        .expect_err("error calling function");

    assert_eq!(e.message(), "test 123");

    Ok(())
}

#[cfg_attr(target_env = "musl", ignore)]
#[compiler_test(traps)]
fn test_trap_trace(config: crate::Config) -> Result<()> {
    let mut store = config.store();
    let wat = r#"
        (module $hello_mod
            (func (export "run") (call $hello))
            (func $hello (unreachable))
        )
    "#;

    let module = Module::new(&store, wat)?;
    let instance = Instance::new(&mut store, &module, &imports! {})?;
    let run_func = instance
        .exports
        .get_function("run")
        .expect("expected function export");

    let e = run_func
        .call(&mut store, &[])
        .expect_err("error calling function");

    let trace = e.trace();
    assert_eq!(trace.len(), 2);
    assert_eq!(trace[0].module_name(), "hello_mod");
    assert_eq!(trace[0].func_index(), 1);
    assert_eq!(trace[0].function_name(), Some("hello"));
    assert_eq!(trace[1].module_name(), "hello_mod");
    assert_eq!(trace[1].func_index(), 0);
    assert_eq!(trace[1].function_name(), None);
    assert!(
        e.message().contains("unreachable"),
        "wrong message: {}",
        e.message()
    );

    Ok(())
}

#[compiler_test(traps)]
fn test_trap_trace_cb(config: crate::Config) -> Result<()> {
    let mut store = config.store();
    let wat = r#"
        (module $hello_mod
            (import "" "throw" (func $throw))
            (func (export "run") (call $hello))
            (func $hello (call $throw))
        )
    "#;

    let fn_type = FunctionType::new(vec![], vec![]);
    let fn_func = Function::new(&mut store, &fn_type, |_| Err(RuntimeError::new("cb throw")));

    let module = Module::new(&store, wat)?;
    let instance = Instance::new(
        &mut store,
        &module,
        &imports! {
            "" => {
                "throw" => fn_func
            }
        },
    )?;
    let run_func = instance
        .exports
        .get_function("run")
        .expect("expected function export");

    let e = run_func
        .call(&mut store, &[])
        .expect_err("error calling function");

    let trace = e.trace();
    println!("Trace {:?}", trace);
    // TODO: Reenable this (disabled as it was not working with llvm/singlepass)
    // assert_eq!(trace.len(), 2);
    // assert_eq!(trace[0].module_name(), "hello_mod");
    // assert_eq!(trace[0].func_index(), 2);
    // assert_eq!(trace[1].module_name(), "hello_mod");
    // assert_eq!(trace[1].func_index(), 1);
    assert_eq!(e.message(), "cb throw");

    Ok(())
}

#[cfg_attr(target_env = "musl", ignore)]
#[compiler_test(traps)]
fn test_trap_stack_overflow(config: crate::Config) -> Result<()> {
    let mut store = config.store();
    let wat = r#"
        (module $rec_mod
            (func $run (export "run") (call $run))
        )
    "#;

    let module = Module::new(&store, wat)?;
    let instance = Instance::new(&mut store, &module, &imports! {})?;
    let run_func = instance
        .exports
        .get_function("run")
        .expect("expected function export");

    let e = run_func
        .call(&mut store, &[])
        .expect_err("error calling function");

    // We specifically don't check the stack trace here: stack traces after
    // stack overflows are not generally possible due to unreliable unwinding
    // information.
    assert!(e.message().contains("call stack exhausted"));

    Ok(())
}

#[cfg_attr(target_env = "musl", ignore)]
#[compiler_test(traps)]
fn trap_display_pretty(config: crate::Config) -> Result<()> {
    let mut store = config.store();
    let wat = r#"
        (module $m
            (func $die unreachable)
            (func call $die)
            (func $foo call 1)
            (func (export "bar") call $foo)
        )
    "#;

    let module = Module::new(&store, wat)?;
    let instance = Instance::new(&mut store, &module, &imports! {})?;
    let run_func = instance
        .exports
        .get_function("bar")
        .expect("expected function export");

    let e = run_func
        .call(&mut store, &[])
        .expect_err("error calling function");
    assert_eq!(
        e.to_string(),
        "\
RuntimeError: unreachable
    at die (m[0]:0x23)
    at <unnamed> (m[1]:0x27)
    at foo (m[2]:0x2c)
    at <unnamed> (m[3]:0x31)"
    );
    Ok(())
}

#[cfg_attr(target_env = "musl", ignore)]
#[compiler_test(traps)]
fn trap_display_multi_module(config: crate::Config) -> Result<()> {
    let mut store = config.store();
    let wat = r#"
        (module $a
            (func $die unreachable)
            (func call $die)
            (func $foo call 1)
            (func (export "bar") call $foo)
        )
    "#;

    let module = Module::new(&store, wat)?;
    let instance = Instance::new(&mut store, &module, &imports! {})?;
    let bar = instance.exports.get_function("bar")?.clone();

    let wat = r#"
        (module $b
            (import "" "" (func $bar))
            (func $middle call $bar)
            (func (export "bar2") call $middle)
        )
    "#;
    let module = Module::new(&store, wat)?;
    let instance = Instance::new(
        &mut store,
        &module,
        &imports! {
            "" => {
                "" => bar
            }
        },
    )?;
    let bar2 = instance
        .exports
        .get_function("bar2")
        .expect("expected function export");

    let e = bar2
        .call(&mut store, &[])
        .expect_err("error calling function");
    assert_eq!(
        e.to_string(),
        "\
RuntimeError: unreachable
    at die (a[0]:0x23)
    at <unnamed> (a[1]:0x27)
    at foo (a[2]:0x2c)
    at <unnamed> (a[3]:0x31)
    at middle (b[1]:0x29)
    at <unnamed> (b[2]:0x2e)"
    );
    Ok(())
}

#[compiler_test(traps)]
fn trap_start_function_import(config: crate::Config) -> Result<()> {
    let mut store = config.store();
    let binary = r#"
        (module $a
            (import "" "" (func $foo))
            (start $foo)
        )
    "#;

    let module = Module::new(&store, binary)?;
    let sig = FunctionType::new(vec![], vec![]);
    let func = Function::new(&mut store, &sig, |_| Err(RuntimeError::new("user trap")));
    let err = Instance::new(
        &mut store,
        &module,
        &imports! {
            "" => {
                "" => func
            }
        },
    )
    .err()
    .unwrap();
    match err {
        InstantiationError::Link(_)
        | InstantiationError::DifferentStores
        | InstantiationError::DifferentArchOS
        | InstantiationError::CpuFeature(_) => {
            panic!("It should be a start error")
        }
        InstantiationError::Start(err) => {
            assert_eq!(err.message(), "user trap");
        }
    }

    Ok(())
}

#[compiler_test(traps)]
fn rust_panic_import(config: crate::Config) -> Result<()> {
    let mut store = config.store();
    let binary = r#"
        (module $a
            (import "" "foo" (func $foo))
            (import "" "bar" (func $bar))
            (func (export "foo") call $foo)
            (func (export "bar") call $bar)
        )
    "#;

    let module = Module::new(&store, binary)?;
    let sig = FunctionType::new(vec![], vec![]);
    let func = Function::new(&mut store, &sig, |_| panic!("this is a panic"));
    let f0 = Function::new_typed(&mut store, || panic!("this is another panic"));
    let instance = Instance::new(
        &mut store,
        &module,
        &imports! {
            "" => {
                "foo" => func,
                "bar" => f0
            }
        },
    )?;
    let func = instance.exports.get_function("foo")?.clone();
    let err = panic::catch_unwind(AssertUnwindSafe(|| {
        drop(func.call(&mut store, &[]));
    }))
    .unwrap_err();
    assert_eq!(err.downcast_ref::<&'static str>(), Some(&"this is a panic"));

    // TODO: Reenable this (disabled as it was not working with llvm/singlepass)
    // It doesn't work either with cranelift and `--test-threads=1`.
    // let func = instance.exports.get_function("bar")?.clone();
    // let err = panic::catch_unwind(AssertUnwindSafe(|| {
    //     drop(func.call(&[]));
    // }))
    // .unwrap_err();
    // assert_eq!(
    //     err.downcast_ref::<&'static str>(),
    //     Some(&"this is another panic")
    // );
    Ok(())
}

#[compiler_test(traps)]
fn rust_panic_start_function(config: crate::Config) -> Result<()> {
    let mut store = config.store();
    let binary = r#"
        (module $a
            (import "" "" (func $foo))
            (start $foo)
        )
    "#;

    let module = Module::new(&store, binary)?;
    let sig = FunctionType::new(vec![], vec![]);
    let func = Function::new(&mut store, &sig, |_| panic!("this is a panic"));
    let err = panic::catch_unwind(AssertUnwindSafe(|| {
        drop(Instance::new(
            &mut store,
            &module,
            &imports! {
                "" => {
                    "" => func
                }
            },
        ));
    }))
    .unwrap_err();
    assert_eq!(err.downcast_ref::<&'static str>(), Some(&"this is a panic"));

    let func = Function::new_typed(&mut store, || panic!("this is another panic"));
    let err = panic::catch_unwind(AssertUnwindSafe(|| {
        drop(Instance::new(
            &mut store,
            &module,
            &imports! {
                "" => {
                    "" => func
                }
            },
        ));
    }))
    .unwrap_err();
    assert_eq!(
        err.downcast_ref::<&'static str>(),
        Some(&"this is another panic")
    );
    Ok(())
}

#[compiler_test(traps)]
fn mismatched_arguments(config: crate::Config) -> Result<()> {
    let mut store = config.store();
    let binary = r#"
        (module $a
            (func (export "foo") (param i32))
        )
    "#;

    let module = Module::new(&store, binary)?;
    let instance = Instance::new(&mut store, &module, &imports! {})?;
    let func: &Function = instance.exports.get("foo")?;
    assert_eq!(
        func.call(&mut store, &[]).unwrap_err().message(),
        "Parameters of type [] did not match signature [I32] -> []"
    );
    assert_eq!(
        func.call(&mut store, &[Value::F32(0.0)])
            .unwrap_err()
            .message(),
        "Parameters of type [F32] did not match signature [I32] -> []",
    );
    assert_eq!(
        func.call(&mut store, &[Value::I32(0), Value::I32(1)])
            .unwrap_err()
            .message(),
        "Parameters of type [I32, I32] did not match signature [I32] -> []"
    );
    Ok(())
}

#[cfg_attr(target_env = "musl", ignore)]
#[compiler_test(traps)]
fn call_signature_mismatch(config: crate::Config) -> Result<()> {
    let mut store = config.store();
    let binary = r#"
        (module $a
            (func $foo
                i32.const 0
                call_indirect)
            (func $bar (param i32))
            (start $foo)

            (table 1 anyfunc)
            (elem (i32.const 0) 1)
        )
    "#;

    let module = Module::new(&store, binary)?;
    let err = Instance::new(&mut store, &module, &imports! {}).expect_err("expected error");
    assert_eq!(
        format!("{}", err),
        "\
RuntimeError: indirect call type mismatch
    at foo (a[0]:0x30)\
"
    );
    Ok(())
}

#[compiler_test(traps)]
#[cfg_attr(target_env = "musl", ignore)]
fn start_trap_pretty(config: crate::Config) -> Result<()> {
    let mut store = config.store();
    let wat = r#"
        (module $m
            (func $die unreachable)
            (func call $die)
            (func $foo call 1)
            (func $start call $foo)
            (start $start)
        )
    "#;

    let module = Module::new(&store, wat)?;
    let err = Instance::new(&mut store, &module, &imports! {}).expect_err("expected error");

    assert_eq!(
        format!("{}", err),
        "\
RuntimeError: unreachable
    at die (m[0]:0x1d)
    at <unnamed> (m[1]:0x21)
    at foo (m[2]:0x26)
    at start (m[3]:0x2b)\
"
    );
    Ok(())
}

#[compiler_test(traps)]
fn present_after_module_drop(config: crate::Config) -> Result<()> {
    let mut store = config.store();
    let module = Module::new(&store, r#"(func (export "foo") unreachable)"#)?;
    let instance = Instance::new(&mut store, &module, &imports! {})?;
    let func: Function = instance.exports.get_function("foo")?.clone();

    println!("asserting before we drop modules");
    assert_trap(func.call(&mut store, &[]).unwrap_err());
    drop((instance, module));

    println!("asserting after drop");
    assert_trap(func.call(&mut store, &[]).unwrap_err());
    return Ok(());

    fn assert_trap(t: RuntimeError) {
        println!("{}", t);
        // assert_eq!(t.trace().len(), 1);
        // assert_eq!(t.trace()[0].func_index(), 0);
    }
}
