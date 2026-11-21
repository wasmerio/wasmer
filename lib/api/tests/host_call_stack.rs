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
//! otherwise, or always when it is set to run there. Getting this wrong
//! overflows the Wasm stack inside host code, which kills the process, so
//! these tests have a test binary of their own.

use std::{
    hint::black_box,
    mem::MaybeUninit,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use wasmer::{
    Function, FunctionEnv, FunctionEnvMut, FunctionType, Instance, Module, RuntimeError, Store,
    Type, TypedFunction, Value, imports, sys::vm::HOST_CALL_RESERVE,
};
use wasmer_types::TrapCode;

/// The tests that look where host functions run share `CALLER_FRAME`: one at
/// a time.
static CALLER: Mutex<()> = Mutex::new(());

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
    const { assert!(HOST_CALL_RESERVE > 192 * 1024) };

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
/// fails, and checks their results; with `on_native_stack`, every host
/// function is set to run on the native stack.
fn run_host_calls(on_native_stack: bool) -> Result<(), Box<dyn std::error::Error>> {
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
    if on_native_stack {
        for function in [&typed, &with_env, &dynamic, &reenter, &fail] {
            function.set_calls_on_native_stack(&mut store, true);
        }
    }
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
    run_host_calls(false)
}

/// The same calls, with every host call switching to the native stack.
#[test]
fn host_calls_on_the_native_stack() -> Result<(), Box<dyn std::error::Error>> {
    run_host_calls(true)
}

/// The address of a local of the test that last called into Wasm.
static CALLER_FRAME: AtomicUsize = AtomicUsize::new(0);

/// Whether this runs on the native stack: within 512 KiB below the frame of
/// the test that called into Wasm. A Wasm stack is a mapping of its own, so it
/// is never there.
#[inline(never)]
fn on_native_stack() -> bool {
    let local = 0u8;
    let here = black_box(&local) as *const u8 as usize;
    let caller = CALLER_FRAME.load(Ordering::Relaxed);
    here < caller && caller - here < 512 * 1024
}

/// A host function set to run on the native stack does from its next call
/// on, while the others go on running on the Wasm stack, and runs there again
/// once the setting is taken back.
#[test]
fn a_host_function_can_run_on_the_native_stack() -> Result<(), Box<dyn std::error::Error>> {
    let _lock = CALLER.lock().unwrap_or_else(|e| e.into_inner());

    let mut store = Store::default();
    let module = Module::new(
        &store,
        r#"(module
            (import "host" "typed" (func $typed (result i32)))
            (import "host" "with_env" (func $with_env (result i32)))
            (import "host" "dynamic" (func $dynamic (result i32)))
            (func (export "typed") (result i32) call $typed)
            (func (export "with_env") (result i32) call $with_env)
            (func (export "dynamic") (result i32) call $dynamic))"#,
    )?;
    let env = FunctionEnv::new(&mut store, ());
    let typed = Function::new_typed(&mut store, || i32::from(on_native_stack()));
    let with_env = Function::new_typed_with_env(&mut store, &env, |_: FunctionEnvMut<()>| {
        i32::from(on_native_stack())
    });
    let dynamic = Function::new(
        &mut store,
        FunctionType::new([], [Type::I32]),
        |_: &[Value]| Ok(vec![Value::I32(i32::from(on_native_stack()))]),
    );
    let imports = imports! {
        "host" => {
            "typed" => typed.clone(),
            "with_env" => with_env.clone(),
            "dynamic" => dynamic.clone(),
        }
    };
    let instance = Instance::new(&mut store, &module, &imports)?;
    let ran_on_native_stack = |store: &mut Store, name: &str| {
        let call: TypedFunction<(), i32> = instance.exports.get_typed_function(store, name)?;
        let local = 0u8;
        CALLER_FRAME.store(black_box(&local) as *const u8 as usize, Ordering::Relaxed);
        Ok::<_, Box<dyn std::error::Error>>(call.call(store)? == 1)
    };

    let functions = [
        ("typed", typed),
        ("with_env", with_env),
        ("dynamic", dynamic),
    ];
    for (name, function) in &functions {
        assert!(!function.calls_on_native_stack(&store), "{name}");
        assert!(!ran_on_native_stack(&mut store, name)?, "{name}");
    }
    for (name, function) in &functions {
        function.set_calls_on_native_stack(&mut store, true);
        assert!(function.calls_on_native_stack(&store), "{name}");
        assert!(ran_on_native_stack(&mut store, name)?, "{name}");
        // The other functions stay on the Wasm stack.
        for (other, _) in functions.iter().filter(|(other, _)| other != name) {
            assert!(!ran_on_native_stack(&mut store, other)?, "{other}");
        }
        function.set_calls_on_native_stack(&mut store, false);
        assert!(!function.calls_on_native_stack(&store), "{name}");
        assert!(!ran_on_native_stack(&mut store, name)?, "{name}");
    }

    // A function defined in Wasm has no such setting.
    let exported = instance.exports.get_function("typed")?;
    exported.set_calls_on_native_stack(&mut store, true);
    assert!(!exported.calls_on_native_stack(&store));
    Ok(())
}

/// An asynchronous host function always runs on the native stack, where
/// its future may suspend the coroutine the call came from: setting it to
/// run on the Wasm stack changes nothing.
#[cfg(feature = "experimental-async")]
#[test]
fn an_async_host_function_always_runs_on_the_native_stack() {
    let engine: wasmer::Engine = wasmer::sys::EngineBuilder::headless().engine().into();
    let mut store = Store::new(engine);
    let function = Function::new_async(
        &mut store,
        FunctionType::new([], [Type::I32]),
        |_: &[Value]| async { Ok(vec![Value::I32(1)]) },
    );
    assert!(function.calls_on_native_stack(&store));
    function.set_calls_on_native_stack(&mut store, false);
    assert!(function.calls_on_native_stack(&store));
}

/// Records the frame of the caller, for `on_native_stack`.
fn mark_caller_frame(local: &u8) {
    CALLER_FRAME.store(black_box(local) as *const u8 as usize, Ordering::Relaxed);
}

/// A host function running on the Wasm stack that calls back into the guest
/// makes the call from the native stack: what that call runs on the native
/// stack (here a host function set to) runs there, not on the Wasm stack of
/// the host function, with what is left of its reserve.
#[test]
fn a_call_back_into_wasm_starts_from_the_native_stack() -> Result<(), Box<dyn std::error::Error>> {
    let _lock = CALLER.lock().unwrap_or_else(|e| e.into_inner());

    let mut store = Store::default();
    let module = Module::new(
        &store,
        r#"(module
            (import "host" "reenter" (func $reenter (result i32)))
            (import "host" "native" (func $native (result i32)))
            (import "host" "in_place" (func $in_place (result i32)))
            (func (export "outer") (result i32) call $reenter)
            (func (export "inner") (result i32)
                call $native
                i32.const 2
                i32.mul
                call $in_place
                i32.add))"#,
    )?;
    let env = FunctionEnv::new(&mut store, None::<TypedFunction<(), i32>>);
    let reenter = Function::new_typed_with_env(
        &mut store,
        &env,
        |mut env: FunctionEnvMut<Option<TypedFunction<(), i32>>>| {
            let in_place = !on_native_stack();
            let (inner, mut store) = env.data_and_store_mut();
            let inner = inner.as_ref().unwrap().call(&mut store).unwrap();
            inner * 2 + i32::from(in_place)
        },
    );
    let native = Function::new_typed(&mut store, || i32::from(on_native_stack()));
    native.set_calls_on_native_stack(&mut store, true);
    let in_place = Function::new_typed(&mut store, || i32::from(!on_native_stack()));
    let imports = imports! {
        "host" => {
            "reenter" => reenter,
            "native" => native,
            "in_place" => in_place,
        }
    };
    let instance = Instance::new(&mut store, &module, &imports)?;
    *env.as_mut(&mut store) = Some(instance.exports.get_typed_function(&store, "inner")?);
    let outer: TypedFunction<(), i32> = instance.exports.get_typed_function(&store, "outer")?;

    let local = 0u8;
    mark_caller_frame(&local);
    // Bits, from the lowest: `reenter` ran on the Wasm stack, `in_place` did
    // too, and `native` ran on the native stack.
    assert_eq!(outer.call(&mut store)?, 0b111);
    Ok(())
}

/// A host function's setting is read and changed without running Wasm, so
/// that Miri can check it (`cargo +nightly miri test -p wasmer --test
/// host_call_stack -- the_stack_setting`): the setting lives inside the
/// function's state, which its `VMFunction` owns and moves with it.
#[test]
fn the_stack_setting_of_a_host_function() {
    let engine: wasmer::Engine = wasmer::sys::EngineBuilder::headless().engine().into();
    let mut store = Store::new(engine);
    let env = FunctionEnv::new(&mut store, ());
    let typed = Function::new_typed(&mut store, |x: i32| x + 1);
    let with_env = Function::new_typed_with_env(&mut store, &env, |_: FunctionEnvMut<()>| 1);
    let dynamic = Function::new(
        &mut store,
        FunctionType::new([], [Type::I32]),
        |_: &[Value]| Ok(vec![Value::I32(1)]),
    );
    for function in [&typed, &with_env, &dynamic] {
        assert!(!function.calls_on_native_stack(&store));
        function.set_calls_on_native_stack(&mut store, true);
        assert!(function.calls_on_native_stack(&store));
        // Making more functions moves the list the others are in.
        Function::new_typed(&mut store, || ());
        assert!(function.calls_on_native_stack(&store));
        function.set_calls_on_native_stack(&mut store, false);
        assert!(!function.calls_on_native_stack(&store));
    }
}
