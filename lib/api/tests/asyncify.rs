use macro_wasmer_engine_test::engine_test;
use wasmer::{
    asyncify::{
        AsyncifyCallWrapper, AsyncifyEnv, AsyncifyFunction, OnCalledAction, OnCalledHandler,
    },
    *,
};

#[cfg(feature = "js")]
use wasm_bindgen_test::wasm_bindgen_test;

/// Environment whose host function leaves an unwind handler on its first
/// `unwinds_left` entries.
struct Unwinder {
    entries: u32,
    unwinds_left: u32,
    action: fn() -> OnCalledAction,
    on_called: Option<OnCalledHandler>,
}

impl AsyncifyEnv for Unwinder {
    fn take_on_called(&mut self) -> Option<OnCalledHandler> {
        self.on_called.take()
    }
}

fn enter(mut env: FunctionEnvMut<Unwinder>) -> u32 {
    let env = env.data_mut();
    env.entries += 1;
    if env.unwinds_left > 0 {
        env.unwinds_left -= 1;
        let action = env.action;
        env.on_called = Some(Box::new(move |_| Ok(action())));
    }
    env.entries
}

fn instantiate(
    store: &mut Store,
    unwinds_left: u32,
    action: fn() -> OnCalledAction,
) -> anyhow::Result<(Function, FunctionEnv<Unwinder>)> {
    let wat = r#"(module
        (func $enter (import "env" "enter") (result i32))
        (func (export "run") (result i32) call $enter))"#;
    let module = Module::new(store, wat)?;
    let env = FunctionEnv::new(
        store,
        Unwinder {
            entries: 0,
            unwinds_left,
            action,
            on_called: None,
        },
    );
    let imports = imports! {
        "env" => { "enter" => Function::new_typed_with_env(store, &env, enter) },
    };
    let instance = Instance::new(store, &module, &imports)?;
    Ok((instance.exports.get_function("run")?.clone(), env))
}

#[engine_test]
fn call_asyncify_reenters_while_handlers_ask_for_it() -> anyhow::Result<()> {
    let mut store = Store::default();
    let (run, env) = instantiate(&mut store, 2, || OnCalledAction::InvokeAgain)?;
    let results = run.call_asyncify(&mut store, &env, &[])?;
    assert_eq!(results.to_vec(), vec![Value::I32(3)]);

    env.as_mut(&mut store).unwinds_left = 1;
    let run: TypedFunction<(), u32> = run.typed(&store)?;
    let mut asyncify = AsyncifyCallWrapper::new(env.clone());
    assert_eq!(run.call_wrapped(&mut store, &mut asyncify)?, 5);
    Ok(())
}

#[engine_test]
fn call_asyncify_finishes_or_traps_as_the_handler_says() -> anyhow::Result<()> {
    let mut store = Store::default();
    let (run, env) = instantiate(&mut store, 1, || OnCalledAction::Finish)?;
    let results = run.call_asyncify(&mut store, &env, &[])?;
    assert_eq!(results.to_vec(), vec![Value::I32(1)]);

    let (run, env) = instantiate(&mut store, 1, || {
        OnCalledAction::Trap("unwind failed".into())
    })?;
    let err = run.call_asyncify(&mut store, &env, &[]).unwrap_err();
    assert!(err.to_string().contains("unwind failed"), "{err}");
    Ok(())
}
