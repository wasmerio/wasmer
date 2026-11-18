// With `v8-default` (the Windows test lane) `Store::default()` is a V8 store,
// which has no Wasm stacks of its own.
#![cfg(all(
    feature = "sys",
    not(feature = "v8-default"),
    not(target_arch = "wasm32")
))]
//! Where host functions called from Wasm run.
//!
//! A synchronous host function runs on the Wasm stack it was called from
//! when at least the host-call reserve is left on it, and on the native stack
//! otherwise, or always when host calls are set to run there. Getting this
//! wrong overflows the Wasm stack inside host code, which kills the process,
//! so these tests have a test binary of their own.

use std::{hint::black_box, mem::MaybeUninit, sync::Mutex};

use wasmer::{
    Function, FunctionEnv, FunctionEnvMut, FunctionType, Instance, Module, RuntimeError, Store,
    Type, TypedFunction, Value, imports,
    sys::vm::{
        DEFAULT_HOST_CALL_RESERVE, get_host_call_reserve, host_calls_on_native_stack,
        set_host_calls_on_native_stack,
    },
};
use wasmer_types::TrapCode;

/// The host-call settings are process-wide: one test at a time.
static SETTINGS: Mutex<()> = Mutex::new(());

/// Puts back the host-call setting a test changes.
struct RestoreNativeStack(bool);

impl Drop for RestoreNativeStack {
    fn drop(&mut self) {
        set_host_calls_on_native_stack(self.0);
    }
}

/// Uses a stack frame of 192 KiB: less than the default reserve, but more
/// than the Wasm stack has left near its end. The frame is not written, but
/// the probes of a frame this large touch each of its pages, so it faults on
/// the guard page if it does not fit.
#[inline(never)]
fn use_192_kib_of_stack() {
    let mut frame = MaybeUninit::<[u8; 192 * 1024]>::uninit();
    black_box(frame.as_mut_ptr());
}

const RECURSE_WITH_HOST_CALLS: &str = r#"
    (module
        (import "host" "deep" (func $deep))
        (func $recurse (export "recurse") (param i32)
            call $deep
            local.get 0
            i32.const 1
            i32.add
            call $recurse))
"#;

/// The guest recurses, calling the host function at every level, until it
/// overflows its stack. Near the end there is not room for the host
/// function's frame on the Wasm stack, so it has to run on the native stack;
/// the guest then traps cleanly with `StackOverflow`.
#[test]
fn host_calls_switch_stacks_when_the_wasm_stack_runs_low() -> Result<(), Box<dyn std::error::Error>>
{
    let _lock = SETTINGS.lock().unwrap_or_else(|e| e.into_inner());
    let _restore = RestoreNativeStack(host_calls_on_native_stack());
    set_host_calls_on_native_stack(false);
    assert!(get_host_call_reserve() >= DEFAULT_HOST_CALL_RESERVE);

    let mut store = Store::default();
    let module = Module::new(&store, RECURSE_WITH_HOST_CALLS)?;
    let deep = Function::new_typed(&mut store, use_192_kib_of_stack);
    let instance = Instance::new(
        &mut store,
        &module,
        &imports! { "host" => { "deep" => deep } },
    )?;
    let recurse: TypedFunction<i32, ()> = instance.exports.get_typed_function(&store, "recurse")?;

    let error = recurse.call(&mut store, 0).unwrap_err();
    assert_eq!(
        error.clone().to_trap(),
        Some(TrapCode::StackOverflow),
        "{error}"
    );

    // The store and the thread are fine afterwards.
    let error = recurse.call(&mut store, 0).unwrap_err();
    assert_eq!(
        error.clone().to_trap(),
        Some(TrapCode::StackOverflow),
        "{error}"
    );
    Ok(())
}

const CALLS: &str = r#"
    (module
        (import "host" "typed" (func $typed (param i32) (result i32)))
        (import "host" "with_env" (func $with_env (param i32) (result i32)))
        (import "host" "dynamic" (func $dynamic (param i32) (result i32)))
        (import "host" "reenter" (func $reenter (param i32) (result i32)))
        (import "host" "fail" (func $fail))
        (func (export "inc") (param i32) (result i32)
            local.get 0
            i32.const 1
            i32.add)
        (func (export "all") (param i32) (result i32)
            local.get 0
            call $typed
            call $with_env
            call $dynamic
            call $reenter)
        (func (export "fail")
            call $fail))
"#;

struct Env {
    calls: u32,
    inc: Option<TypedFunction<i32, i32>>,
}

/// Runs typed, environment, dynamic and re-entrant host calls, and one that
/// fails, and checks their results.
fn run_host_calls() -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::default();
    let module = Module::new(&store, CALLS)?;
    let env = FunctionEnv::new(
        &mut store,
        Env {
            calls: 0,
            inc: None,
        },
    );
    let typed = Function::new_typed(&mut store, |x: i32| x + 1);
    let with_env =
        Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>, x: i32| {
            env.data_mut().calls += 1;
            x + 10
        });
    let dynamic = Function::new(
        &mut store,
        FunctionType::new(vec![Type::I32], vec![Type::I32]),
        |args: &[Value]| Ok(vec![Value::I32(args[0].unwrap_i32() + 100)]),
    );
    let reenter =
        Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>, x: i32| {
            let (data, mut store) = env.data_and_store_mut();
            data.inc.as_ref().unwrap().call(&mut store, x).unwrap()
        });
    let fail = Function::new_typed(&mut store, || -> Result<(), RuntimeError> {
        Err(RuntimeError::new("failed on purpose"))
    });
    let imports = imports! {
        "host" => {
            "typed" => typed,
            "with_env" => with_env,
            "dynamic" => dynamic,
            "reenter" => reenter,
            "fail" => fail,
        }
    };
    let instance = Instance::new(&mut store, &module, &imports)?;
    let inc: TypedFunction<i32, i32> = instance.exports.get_typed_function(&store, "inc")?;
    env.as_mut(&mut store).inc = Some(inc);
    let all: TypedFunction<i32, i32> = instance.exports.get_typed_function(&store, "all")?;
    let fail: TypedFunction<(), ()> = instance.exports.get_typed_function(&store, "fail")?;

    for i in 0..3 {
        assert_eq!(all.call(&mut store, i)?, i + 112);
        let error = fail.call(&mut store).unwrap_err();
        assert_eq!(error.message(), "failed on purpose");
    }
    assert_eq!(env.as_ref(&store).calls, 3);
    Ok(())
}

#[test]
fn host_calls_run_on_the_wasm_stack() -> Result<(), Box<dyn std::error::Error>> {
    let _lock = SETTINGS.lock().unwrap_or_else(|e| e.into_inner());
    let _restore = RestoreNativeStack(host_calls_on_native_stack());
    set_host_calls_on_native_stack(false);
    run_host_calls()
}

/// The same calls, with every host call switching to the native stack.
#[test]
fn host_calls_on_the_native_stack() -> Result<(), Box<dyn std::error::Error>> {
    let _lock = SETTINGS.lock().unwrap_or_else(|e| e.into_inner());
    let _restore = RestoreNativeStack(host_calls_on_native_stack());
    set_host_calls_on_native_stack(true);
    assert!(host_calls_on_native_stack());
    run_host_calls()
}
