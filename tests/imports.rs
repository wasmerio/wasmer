#[macro_use]
mod utils;

use std::{convert::TryInto, sync::Arc};
use wabt::wat2wasm;
use wasmer::compiler::{compile_with, compiler_for_backend, Backend};
use wasmer::error::RuntimeError;
use wasmer::units::Pages;
use wasmer::wasm::{DynFunc, Func, FuncSig, Global, Instance, Memory, MemoryType, Type, Value};
use wasmer::{imports, vm, DynamicFunc};

fn runtime_core_new_api_works(backend: Backend) {
    let wasm = r#"
    (module
    (type $type (func (param i32) (result i32)))
    (global (export "my_global") i32 (i32.const 45))
    (func (export "add_one") (type $type)
        (i32.add (get_local 0)
                (i32.const 1)))
    (func (export "double") (type $type)
        (i32.mul (get_local 0)
                (i32.const 2)))
    )"#;
    let wasm_binary = wat2wasm(wasm.as_bytes()).expect("WAST not valid or malformed");
    let compiler = compiler_for_backend(backend).expect("Backend not recognized");
    let module = compile_with(&wasm_binary, &*compiler).unwrap();
    let import_object = imports! {};
    let instance = module.instantiate(&import_object).unwrap();

    let my_global: Global = instance.exports.get("my_global").unwrap();
    assert_eq!(my_global.get(), Value::I32(45));
    let double: Func<i32, i32> = instance.exports.get("double").unwrap();
    assert_eq!(double.call(5).unwrap(), 10);
    let add_one: DynFunc = instance.exports.get("add_one").unwrap();
    assert_eq!(add_one.call(&[Value::I32(5)]).unwrap(), &[Value::I32(6)]);
    let add_one_memory: Result<DynFunc, _> = instance.exports.get("my_global");
    assert!(add_one_memory.is_err());
}

macro_rules! call_and_assert {
    ($instance:ident, $function:ident( $( $inputs:ty ),* ) -> $output:ty, ( $( $arguments:expr ),* ) == $expected_value:expr) => {
        #[allow(unused_parens)]
        let $function: Func<( $( $inputs ),* ), $output> = $instance.exports.get(stringify!($function)).expect(concat!("Failed to get the `", stringify!($function), "` export function."));

        let result = $function.call( $( $arguments ),* );

        match (result, $expected_value) {
            (Ok(value), expected_value) => assert_eq!(
                Ok(value),
                expected_value,
                concat!("Expected right when calling `", stringify!($function), "`.")
            ),
            (Err(RuntimeError(data)), Err(RuntimeError(expected_data))) => {
                if let (Some(data), Some(expected_data)) = (
                    data.downcast_ref::<&str>(),
                    expected_data.downcast_ref::<&str>(),
                ) {
                    assert_eq!(
                        data, expected_data,
                        concat!("Expected right when calling `", stringify!($function), "`.")
                    )
                } else if let (Some(data), Some(expected_data)) = (
                    data.downcast_ref::<String>(),
                    expected_data.downcast_ref::<String>(),
                ) {
                    assert_eq!(
                        data, expected_data,
                        concat!("Expected right when calling `", stringify!($function), "`.")
                    )
                } else {
                    assert!(false, "Unexpected error, cannot compare it.")
                }
            }
            (result, expected_value) => assert!(
                false,
                format!(
                    "Unexpected assertion for `{}`: left = `{:?}`, right = `{:?}`.",
                    stringify!($function),
                    result,
                    expected_value
                )
            ),
        }
    };
    }

/// The shift that is set in the instance memory. The value is part of
/// the result returned by the imported functions if the memory is
/// read properly.
const SHIFT: i32 = 10;

/// The shift that is captured in the environment of a closure. The
/// value is part of the result returned by the imported function if
/// the closure captures its environment properly.
#[allow(non_upper_case_globals)]
const shift: i32 = 100;

#[cfg(all(unix, target_arch = "x86_64"))]
fn imported_functions_forms(backend: Backend, test: &dyn Fn(&Instance)) {
    const MODULE: &str = r#"
    (module
    (type $type (func (param i32) (result i32)))
    (import "env" "memory" (memory 1 1))
    (import "env" "callback_fn" (func $callback_fn (type $type)))
    (import "env" "callback_closure" (func $callback_closure (type $type)))
    (import "env" "callback_fn_dynamic" (func $callback_fn_dynamic (type $type)))
    (import "env" "callback_fn_dynamic_panic" (func $callback_fn_dynamic_panic (type $type)))
    (import "env" "callback_closure_dynamic_0" (func $callback_closure_dynamic_0))
    (import "env" "callback_closure_dynamic_1" (func $callback_closure_dynamic_1 (param i32) (result i32)))
    (import "env" "callback_closure_dynamic_2" (func $callback_closure_dynamic_2 (param i32 i64) (result i64)))
    (import "env" "callback_closure_dynamic_3" (func $callback_closure_dynamic_3 (param i32 i64 f32) (result f32)))
    (import "env" "callback_closure_dynamic_4" (func $callback_closure_dynamic_4 (param i32 i64 f32 f64) (result f64)))
    (import "env" "callback_closure_with_env" (func $callback_closure_with_env (type $type)))
    (import "env" "callback_fn_with_vmctx" (func $callback_fn_with_vmctx (type $type)))
    (import "env" "callback_closure_with_vmctx" (func $callback_closure_with_vmctx (type $type)))
    (import "env" "callback_closure_with_vmctx_and_env" (func $callback_closure_with_vmctx_and_env (type $type)))
    (import "env" "callback_fn_trap" (func $callback_fn_trap (type $type)))
    (import "env" "callback_closure_trap" (func $callback_closure_trap (type $type)))
    (import "env" "callback_fn_trap_with_vmctx" (func $callback_fn_trap_with_vmctx (type $type)))
    (import "env" "callback_closure_trap_with_vmctx" (func $callback_closure_trap_with_vmctx (type $type)))
    (import "env" "callback_closure_trap_with_vmctx_and_env" (func $callback_closure_trap_with_vmctx_and_env (type $type)))

    (func (export "function_fn") (type $type)
        get_local 0
        call $callback_fn)

    (func (export "function_closure") (type $type)
        get_local 0
        call $callback_closure)

    (func (export "function_fn_dynamic") (type $type)
        get_local 0
        call $callback_fn_dynamic)

    (func (export "function_fn_dynamic_panic") (type $type)
        get_local 0
        call $callback_fn_dynamic_panic)

    (func (export "function_closure_dynamic_0")
        call $callback_closure_dynamic_0)

    (func (export "function_closure_dynamic_1") (param i32) (result i32)
        get_local 0
        call $callback_closure_dynamic_1)

    (func (export "function_closure_dynamic_2") (param i32 i64) (result i64)
        get_local 0
        get_local 1
        call $callback_closure_dynamic_2)

    (func (export "function_closure_dynamic_3") (param i32 i64 f32) (result f32)
        get_local 0
        get_local 1
        get_local 2
        call $callback_closure_dynamic_3)

    (func (export "function_closure_dynamic_4") (param i32 i64 f32 f64) (result f64)
        get_local 0
        get_local 1
        get_local 2
        get_local 3
        call $callback_closure_dynamic_4)

    (func (export "function_closure_with_env") (type $type)
        get_local 0
        call $callback_closure_with_env)

    (func (export "function_fn_with_vmctx") (type $type)
        get_local 0
        call $callback_fn_with_vmctx)

    (func (export "function_closure_with_vmctx") (type $type)
        get_local 0
        call $callback_closure_with_vmctx)

    (func (export "function_closure_with_vmctx_and_env") (type $type)
        get_local 0
        call $callback_closure_with_vmctx_and_env)

    (func (export "function_fn_trap") (type $type)
        get_local 0
        call $callback_fn_trap)

    (func (export "function_closure_trap") (type $type)
        get_local 0
        call $callback_closure_trap)

    (func (export "function_fn_trap_with_vmctx") (type $type)
        get_local 0
        call $callback_fn_trap_with_vmctx)

    (func (export "function_closure_trap_with_vmctx") (type $type)
        get_local 0
        call $callback_closure_trap_with_vmctx)

    (func (export "function_closure_trap_with_vmctx_and_env") (type $type)
        get_local 0
        call $callback_closure_trap_with_vmctx_and_env))
    "#;

    let wasm_binary = wat2wasm(MODULE.as_bytes()).expect("WAST not valid or malformed");
    let compiler = compiler_for_backend(backend).expect("Backend not recognized");
    let module = compile_with(&wasm_binary, &*compiler).unwrap();
    let memory_descriptor = MemoryType::new(Pages(1), Some(Pages(1)), false).unwrap();
    let memory = Memory::new(memory_descriptor).unwrap();

    memory.view()[0].set(SHIFT);

    let import_object = imports! {
        "env" => {
            "memory" => memory.clone(),

            // Regular function.
            "callback_fn" => Func::new(callback_fn),

            // Closure without a captured environment.
            "callback_closure" => Func::new(|n: i32| -> Result<i32, ()> {
                Ok(n + 1)
            }),

            // Regular polymorphic function.
            "callback_fn_dynamic" => DynamicFunc::new(
                Arc::new(FuncSig::new(vec![Type::I32], vec![Type::I32])),
                callback_fn_dynamic,
            ),

            // Polymorphic function that panics.
            "callback_fn_dynamic_panic" => DynamicFunc::new(
                Arc::new(FuncSig::new(vec![Type::I32], vec![Type::I32])),
                callback_fn_dynamic_panic,
            ),

            // Polymorphic closure “closures”.
            "callback_closure_dynamic_0" => DynamicFunc::new(
                Arc::new(FuncSig::new(vec![], vec![])),
                |_, inputs: &[Value]| -> Vec<Value> {
                    assert!(inputs.is_empty());

                    vec![]
                }
            ),
            "callback_closure_dynamic_1" => DynamicFunc::new(
                Arc::new(FuncSig::new(vec![Type::I32], vec![Type::I32])),
                move |vmctx: &mut vm::Ctx, inputs: &[Value]| -> Vec<Value> {
                    assert_eq!(inputs.len(), 1);

                    let memory = vmctx.memory(0);
                    let shift_ = shift + memory.view::<i32>()[0].get();
                    let n: i32 = (&inputs[0]).try_into().unwrap();

                    vec![Value::I32(shift_ + n)]
                }
            ),
            "callback_closure_dynamic_2" => DynamicFunc::new(
                Arc::new(FuncSig::new(vec![Type::I32, Type::I64], vec![Type::I64])),
                move |vmctx: &mut vm::Ctx, inputs: &[Value]| -> Vec<Value> {
                    assert_eq!(inputs.len(), 2);

                    let memory = vmctx.memory(0);
                    let shift_ = shift + memory.view::<i32>()[0].get();
                    let i: i32 = (&inputs[0]).try_into().unwrap();
                    let j: i64 = (&inputs[1]).try_into().unwrap();

                    vec![Value::I64(shift_ as i64 + i as i64 + j)]
                }
            ),
            "callback_closure_dynamic_3" => DynamicFunc::new(
                Arc::new(FuncSig::new(vec![Type::I32, Type::I64, Type::F32], vec![Type::F32])),
                move |vmctx: &mut vm::Ctx, inputs: &[Value]| -> Vec<Value> {
                    assert_eq!(inputs.len(), 3);

                    let memory = vmctx.memory(0);
                    let shift_ = shift + memory.view::<i32>()[0].get();
                    let i: i32 = (&inputs[0]).try_into().unwrap();
                    let j: i64 = (&inputs[1]).try_into().unwrap();
                    let k: f32 = (&inputs[2]).try_into().unwrap();

                    vec![Value::F32(shift_ as f32 + i as f32 + j as f32 + k)]
                }
            ),
            "callback_closure_dynamic_4" => DynamicFunc::new(
                Arc::new(FuncSig::new(vec![Type::I32, Type::I64, Type::F32, Type::F64], vec![Type::F64])),
                move |vmctx: &mut vm::Ctx, inputs: &[Value]| -> Vec<Value> {
                    assert_eq!(inputs.len(), 4);

                    let memory = vmctx.memory(0);
                    let shift_ = shift + memory.view::<i32>()[0].get();
                    let i: i32 = (&inputs[0]).try_into().unwrap();
                    let j: i64 = (&inputs[1]).try_into().unwrap();
                    let k: f32 = (&inputs[2]).try_into().unwrap();
                    let l: f64 = (&inputs[3]).try_into().unwrap();

                    vec![Value::F64(shift_ as f64 + i as f64 + j as f64 + k as f64 + l)]
                }
            ),

            // Closure with a captured environment (a single variable + an instance of `Memory`).
            "callback_closure_with_env" => Func::new(move |n: i32| -> Result<i32, ()> {
                let shift_ = shift + memory.view::<i32>()[0].get();

                Ok(shift_ + n + 1)
            }),

            // Regular function with an explicit `vmctx`.
            "callback_fn_with_vmctx" => Func::new(callback_fn_with_vmctx),

            // Closure without a captured environment but with an explicit `vmctx`.
            "callback_closure_with_vmctx" => Func::new(|vmctx: &mut vm::Ctx, n: i32| -> Result<i32, ()> {
                let memory = vmctx.memory(0);
                let shift_: i32 = memory.view()[0].get();

                Ok(shift_ + n + 1)
            }),

            // Closure with a captured environment (a single variable) and with an explicit `vmctx`.
            "callback_closure_with_vmctx_and_env" => Func::new(move |vmctx: &mut vm::Ctx, n: i32| -> Result<i32, ()> {
                let memory = vmctx.memory(0);
                let shift_ = shift + memory.view::<i32>()[0].get();

                Ok(shift_ + n + 1)
            }),

            // Trap a regular function.
            "callback_fn_trap" => Func::new(callback_fn_trap),

            // Trap a closure without a captured environment.
            "callback_closure_trap" => Func::new(|n: i32| -> Result<i32, String> {
                Err(format!("bar {}", n + 1))
            }),

            // Trap a regular function with an explicit `vmctx`.
            "callback_fn_trap_with_vmctx" => Func::new(callback_fn_trap_with_vmctx),

            // Trap a closure without a captured environment but with an explicit `vmctx`.
            "callback_closure_trap_with_vmctx" => Func::new(|vmctx: &mut vm::Ctx, n: i32| -> Result<i32, String> {
                let memory = vmctx.memory(0);
                let shift_: i32 = memory.view()[0].get();

                Err(format!("qux {}", shift_ + n + 1))
            }),

            // Trap a closure with a captured environment (a single variable) and with an explicit `vmctx`.
            "callback_closure_trap_with_vmctx_and_env" => Func::new(move |vmctx: &mut vm::Ctx, n: i32| -> Result<i32, String> {
                let memory = vmctx.memory(0);
                let shift_ = shift + memory.view::<i32>()[0].get();

                Err(format!("! {}", shift_ + n + 1))
            }),
        },
    };
    let instance = module.instantiate(&import_object).unwrap();

    test(&instance);
}

fn callback_fn(n: i32) -> Result<i32, ()> {
    Ok(n + 1)
}

fn callback_fn_dynamic(_: &mut vm::Ctx, inputs: &[Value]) -> Vec<Value> {
    match inputs[0] {
        Value::I32(x) => vec![Value::I32(x + 1)],
        _ => unreachable!(),
    }
}

fn callback_fn_dynamic_panic(_: &mut vm::Ctx, _: &[Value]) -> Vec<Value> {
    panic!("test");
}

fn callback_fn_with_vmctx(vmctx: &mut vm::Ctx, n: i32) -> Result<i32, ()> {
    let memory = vmctx.memory(0);
    let shift_: i32 = memory.view()[0].get();

    Ok(shift_ + n + 1)
}

fn callback_fn_trap(n: i32) -> Result<i32, String> {
    Err(format!("foo {}", n + 1))
}

fn callback_fn_trap_with_vmctx(vmctx: &mut vm::Ctx, n: i32) -> Result<i32, String> {
    let memory = vmctx.memory(0);
    let shift_: i32 = memory.view()[0].get();

    Err(format!("baz {}", shift_ + n + 1))
}

macro_rules! test {
    ($test_name:ident, $function:ident( $( $inputs:ty ),* ) -> $output:ty, ( $( $arguments:expr ),* ) == $expected_value:expr) => {
        #[cfg(all(unix, target_arch = "x86_64"))]
        #[test]
        fn $test_name() {
            imported_functions_forms(get_backend(), &|instance| {
                call_and_assert!(instance, $function( $( $inputs ),* ) -> $output, ( $( $arguments ),* ) == $expected_value);
            });
        }
    }
}

wasmer_backends! {
    use super::*;

    test!( test_fn, function_fn(i32) -> i32, (1) == Ok(2));
    test!( test_closure, function_closure(i32) -> i32, (1) == Ok(2));
    test!( test_fn_dynamic, function_fn_dynamic(i32) -> i32, (1) == Ok(2));
    test!( test_fn_dynamic_panic, function_fn_dynamic_panic(i32) -> i32, (1) == Err(RuntimeError(Box::new("test"))));
    test!(

        test_closure_dynamic_0,
        function_closure_dynamic_0(()) -> (),
        () == Ok(())
    );
    test!(

        test_closure_dynamic_1,
        function_closure_dynamic_1(i32) -> i32,
        (1) == Ok(1 + shift + SHIFT)
    );
    test!(

        test_closure_dynamic_2,
        function_closure_dynamic_2(i32, i64) -> i64,
        (1, 2) == Ok(1 + 2 + shift as i64 + SHIFT as i64)
    );
    test!(

        test_closure_dynamic_3,
        function_closure_dynamic_3(i32, i64, f32) -> f32,
        (1, 2, 3.) == Ok(1. + 2. + 3. + shift as f32 + SHIFT as f32)
    );
    test!(

        test_closure_dynamic_4,
        function_closure_dynamic_4(i32, i64, f32, f64) -> f64,
        (1, 2, 3., 4.) == Ok(1. + 2. + 3. + 4. + shift as f64 + SHIFT as f64)
    );
    test!(

        test_closure_with_env,
        function_closure_with_env(i32) -> i32,
        (1) == Ok(2 + shift + SHIFT)
    );
    test!( test_fn_with_vmctx, function_fn_with_vmctx(i32) -> i32, (1) == Ok(2 + SHIFT));
    test!(

        test_closure_with_vmctx,
        function_closure_with_vmctx(i32) -> i32,
        (1) == Ok(2 + SHIFT)
    );
    test!(

        test_closure_with_vmctx_and_env,
        function_closure_with_vmctx_and_env(i32) -> i32,
        (1) == Ok(2 + shift + SHIFT)
    );
    test!(

        test_fn_trap,
        function_fn_trap(i32) -> i32,
        (1) == Err(RuntimeError(Box::new(format!("foo {}", 2))))
    );
    test!(

        test_closure_trap,
        function_closure_trap(i32) -> i32,
        (1) == Err(RuntimeError(Box::new(format!("bar {}", 2))))
    );
    test!(

        test_fn_trap_with_vmctx,
        function_fn_trap_with_vmctx(i32) -> i32,
        (1) == Err(RuntimeError(Box::new(format!("baz {}", 2 + SHIFT))))
    );
    test!(

        test_closure_trap_with_vmctx,
        function_closure_trap_with_vmctx(i32) -> i32,
        (1) == Err(RuntimeError(Box::new(format!("qux {}", 2 + SHIFT))))
    );
    test!(

        test_closure_trap_with_vmctx_and_env,
        function_closure_trap_with_vmctx_and_env(i32) -> i32,
        (1) == Err(RuntimeError(Box::new(format!("! {}", 2 + shift + SHIFT))))
    );

    #[test]
    fn runtime_core_new_api() {
        runtime_core_new_api_works(get_backend())
    }
}
