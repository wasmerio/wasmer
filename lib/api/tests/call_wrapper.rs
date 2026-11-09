use macro_wasmer_engine_test::engine_test;
use wasmer::*;

#[cfg(feature = "js")]
use wasm_bindgen_test::wasm_bindgen_test;

const COUNTER_WAT: &str = r#"(module
    (func $bump (import "env" "bump") (result i32))
    (func $bump_then_panic (import "env" "bump_then_panic") (result i32))
    (func (export "run") (result i32) call $bump)
    (func (export "run_then_panic") (result i32) call $bump_then_panic)
    (func (export "trap") unreachable)
    (func (export "add_one") (param i32) (result i32)
        (i32.add (local.get 0) (i32.const 1)))
    (func (export "sum3") (param i32 i32 i32) (result i32)
        (i32.add (i32.add (local.get 0) (local.get 1)) (local.get 2))))"#;

fn bump(mut env: FunctionEnvMut<u32>) -> u32 {
    *env.data_mut() += 1;
    *env.data()
}

/// Like `bump`, but panics from the second entry on.
fn bump_then_panic(env: FunctionEnvMut<u32>) -> u32 {
    let count = bump(env);
    assert!(count < 2, "entered {count} times");
    count
}

fn instantiate(store: &mut Store) -> anyhow::Result<(Instance, FunctionEnv<u32>)> {
    let module = Module::new(store, COUNTER_WAT)?;
    let env = FunctionEnv::new(store, 0u32);
    let imports = imports! {
        "env" => {
            "bump" => Function::new_typed_with_env(store, &env, bump),
            "bump_then_panic" => Function::new_typed_with_env(store, &env, bump_then_panic),
        },
    };
    Ok((Instance::new(store, &module, &imports)?, env))
}

#[engine_test]
fn call_wrapped_can_reenter_the_function() -> anyhow::Result<()> {
    let mut store = Store::default();
    let (instance, env) = instantiate(&mut store)?;
    let mut enter_three_times = |call: &mut WasmCall<'_>| -> Result<(), RuntimeError> {
        for _ in 0..3 {
            call.invoke()?;
        }
        Ok(())
    };

    let run = instance.exports.get_function("run")?;
    let results = run.call_wrapped(&mut store, &mut enter_three_times, &[])?;
    assert_eq!(results.to_vec(), vec![Value::I32(3)]);

    let run: TypedFunction<(), u32> = run.typed(&store)?;
    assert_eq!(run.call_wrapped(&mut store, &mut enter_three_times)?, 6);
    assert_eq!(*env.as_ref(&store), 6);
    Ok(())
}

#[engine_test]
fn call_wrapped_rejects_ok_without_a_completed_call() -> anyhow::Result<()> {
    let mut store = Store::default();
    let (instance, env) = instantiate(&mut store)?;

    let run = instance.exports.get_function("run")?;
    let mut skip = |_: &mut WasmCall<'_>| -> Result<(), RuntimeError> { Ok(()) };
    assert!(run.call_wrapped(&mut store, &mut skip, &[]).is_err());
    assert_eq!(*env.as_ref(&store), 0);

    let trap = instance.exports.get_function("trap")?;
    let mut swallow = |call: &mut WasmCall<'_>| -> Result<(), RuntimeError> {
        let _ = call.invoke();
        Ok(())
    };
    assert!(trap.call_wrapped(&mut store, &mut swallow, &[]).is_err());
    Ok(())
}

/// Sets the counter through the call's store, then delegates to `inner`.
struct SetCounter<W> {
    env: FunctionEnv<u32>,
    value: u32,
    inner: W,
}

impl<W: CallWrapper> CallWrapper for SetCounter<W> {
    fn wrap(&mut self, call: &mut WasmCall<'_>) -> Result<(), RuntimeError> {
        *self.env.as_mut(call) = self.value;
        self.inner.wrap(call)
    }
}

#[engine_test]
fn call_wrappers_compose_and_use_the_store() -> anyhow::Result<()> {
    let mut store = Store::default();
    let (instance, env) = instantiate(&mut store)?;
    let run: TypedFunction<(), u32> = instance.exports.get_typed_function(&store, "run")?;

    let mut wrapper = SetCounter {
        env: env.clone(),
        value: 10,
        inner: |call: &mut WasmCall<'_>| -> Result<(), RuntimeError> {
            call.invoke()?;
            call.invoke()
        },
    };
    assert_eq!(run.call_wrapped(&mut store, &mut wrapper)?, 12);
    Ok(())
}

#[engine_test]
fn call_wrapped_reenters_with_the_original_arguments() -> anyhow::Result<()> {
    let mut store = Store::default();
    let (instance, _env) = instantiate(&mut store)?;
    let mut enter_twice = |call: &mut WasmCall<'_>| -> Result<(), RuntimeError> {
        call.invoke()?;
        call.invoke()
    };

    // `add_one` has as many results as arguments, `sum3` has more arguments.
    let add_one = instance.exports.get_function("add_one")?;
    let results = add_one.call_wrapped(&mut store, &mut enter_twice, &[Value::I32(1)])?;
    assert_eq!(results.to_vec(), vec![Value::I32(2)]);
    let sum3 = instance.exports.get_function("sum3")?;
    let args = [Value::I32(1), Value::I32(2), Value::I32(3)];
    let results = sum3.call_wrapped(&mut store, &mut enter_twice, &args)?;
    assert_eq!(results.to_vec(), vec![Value::I32(6)]);

    let add_one: TypedFunction<i32, i32> = add_one.typed(&store)?;
    assert_eq!(add_one.call_wrapped(&mut store, &mut enter_twice, 1)?, 2);
    let sum3: TypedFunction<(i32, i32, i32), i32> = sum3.typed(&store)?;
    assert_eq!(sum3.call_wrapped(&mut store, &mut enter_twice, 1, 2, 3)?, 6);
    Ok(())
}

/// The wrapper holds the store between entries, so nothing may take it from
/// the store context then.
#[engine_test]
fn call_wrappers_cannot_take_the_store_from_the_context() -> anyhow::Result<()> {
    let mut store = Store::default();
    let (instance, _env) = instantiate(&mut store)?;
    let mut lendable = Vec::new();
    let mut check_store = |call: &mut WasmCall<'_>| -> Result<(), RuntimeError> {
        lendable.push(Store::with_current(|_| ()).is_some());
        call.invoke()?;
        lendable.push(Store::with_current(|_| ()).is_some());
        Ok(())
    };

    let run = instance.exports.get_function("run")?;
    run.call_wrapped(&mut store, &mut check_store, &[])?;
    assert_eq!(lendable, [false, false]);

    lendable.clear();
    let run: TypedFunction<(), u32> = run.typed(&store)?;
    let mut check_store = |call: &mut WasmCall<'_>| -> Result<(), RuntimeError> {
        lendable.push(Store::with_current(|_| ()).is_some());
        call.invoke()
    };
    run.call_wrapped(&mut store, &mut check_store)?;
    assert_eq!(lendable, [false]);
    Ok(())
}

/// An entry that panics never completes the call, even when the wrapper
/// catches the panic after an earlier entry succeeded.
///
/// Native only: panics abort on `wasm32-unknown-unknown`.
#[cfg(not(target_arch = "wasm32"))]
#[engine_test]
fn call_wrapped_rejects_ok_after_a_panicking_entry() -> anyhow::Result<()> {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let mut store = Store::default();
    let (instance, _env) = instantiate(&mut store)?;
    let mut catch_second_entry = |call: &mut WasmCall<'_>| -> Result<(), RuntimeError> {
        call.invoke()?;
        // The panic is the point of the test; keep its backtrace out of a
        // passing run.
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let _ = catch_unwind(AssertUnwindSafe(|| call.invoke()));
        std::panic::set_hook(hook);
        Ok(())
    };

    let run = instance.exports.get_function("run_then_panic")?;
    assert!(
        run.call_wrapped(&mut store, &mut catch_second_entry, &[])
            .is_err()
    );
    Ok(())
}
