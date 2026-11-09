use macro_wasmer_engine_test::engine_test;
use wasmer::*;

#[cfg(feature = "js")]
use wasm_bindgen_test::wasm_bindgen_test;

const COUNTER_WAT: &str = r#"(module
    (func $bump (import "env" "bump") (result i32))
    (func (export "run") (result i32) call $bump)
    (func (export "trap") unreachable))"#;

fn bump(mut env: FunctionEnvMut<u32>) -> u32 {
    *env.data_mut() += 1;
    *env.data()
}

fn instantiate(store: &mut Store) -> anyhow::Result<(Instance, FunctionEnv<u32>)> {
    let module = Module::new(store, COUNTER_WAT)?;
    let env = FunctionEnv::new(store, 0u32);
    let imports = imports! {
        "env" => { "bump" => Function::new_typed_with_env(store, &env, bump) },
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
