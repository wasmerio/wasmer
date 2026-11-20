//! A module asyncified by `wasm-opt` suspends inside a host import and is
//! rewound to where it left off.

use wasmer::{
    Function, FunctionEnv, FunctionEnvMut, Instance, Module, Store, TypedFunction, imports,
};
use wasmer_asyncify::{
    Asyncify, AsyncifyCallWrapper, AsyncifyData, AsyncifyEnv, AsyncifyFunction, AsyncifyState,
    OnCalledAction, OnCalledHandler, on_called,
};

/// `fixtures/sleep.wat`: `run(x)` returns `x * 10 + sleep(x)`.
const SLEEP_WASM: &[u8] = include_bytes!("fixtures/sleep.asyncify.wasm");

/// Header at 1 KiB, buffer up to the end of the first page.
const DATA: AsyncifyData = AsyncifyData {
    header: 1024,
    end: 65536,
};

#[derive(Default)]
struct Sleeper {
    asyncify: Option<Asyncify>,
    on_called: Option<OnCalledHandler>,
    entries: u32,
    saved_stack_sizes: Vec<u64>,
}

impl AsyncifyEnv for Sleeper {
    fn take_on_called(&mut self) -> Option<OnCalledHandler> {
        self.on_called.take()
    }
}

/// Suspends the guest, then returns `ms * 2` once it has been rewound.
fn sleep(mut ctx: FunctionEnvMut<Sleeper>, ms: i32) -> i32 {
    ctx.data_mut().entries += 1;
    let asyncify = ctx.data().asyncify.clone().unwrap();
    if asyncify.state(&mut ctx).unwrap() == AsyncifyState::Rewinding {
        asyncify.stop_rewind(&mut ctx).unwrap();
        return ms * 2;
    }

    asyncify.start_unwind(&mut ctx, DATA).unwrap();
    let env = ctx.as_ref();
    ctx.data_mut().on_called = Some(on_called(move |mut store| {
        let saved = asyncify.stop_unwind(&mut store, DATA)?;
        // The guest is suspended here; an embedder would await its work.
        let saved_len = saved.end - saved.start;
        env.as_mut(&mut store).saved_stack_sizes.push(saved_len);
        asyncify.start_rewind(&mut store, DATA, saved_len)?;
        Ok(OnCalledAction::InvokeAgain)
    }));
    // Ignored: the guest is unwinding.
    0
}

fn instantiate(store: &mut Store) -> anyhow::Result<(Instance, FunctionEnv<Sleeper>)> {
    let module = Module::new(store, SLEEP_WASM)?;
    let env = FunctionEnv::new(store, Sleeper::default());
    let imports = imports! {
        "env" => { "sleep" => Function::new_typed_with_env(store, &env, sleep) },
    };
    let instance = Instance::new(store, &module, &imports)?;
    env.as_mut(store).asyncify = Some(Asyncify::from_instance(&*store, &instance)?);
    Ok((instance, env))
}

#[test]
fn guest_suspends_and_resumes_where_it_left_off() -> anyhow::Result<()> {
    let mut store = Store::default();
    let (instance, env) = instantiate(&mut store)?;

    let run = instance.exports.get_function("run")?;
    let results = run.call_asyncify(&mut store, &env, &[4.into()])?;
    assert_eq!(results.to_vec(), vec![48.into()]);

    let run: TypedFunction<i32, i32> = run.typed(&store)?;
    let mut asyncify = AsyncifyCallWrapper::new(env.clone());
    assert_eq!(run.call_wrapped(&mut store, &mut asyncify, 5)?, 60);

    let sleeper = env.as_ref(&store);
    assert_eq!(sleeper.entries, 4);
    assert_eq!(sleeper.saved_stack_sizes.len(), 2);
    assert!(sleeper.saved_stack_sizes.iter().all(|size| *size > 0));
    let asyncify = sleeper.asyncify.clone().unwrap();
    assert_eq!(asyncify.state(&mut store)?, AsyncifyState::Normal);
    Ok(())
}

#[test]
fn modules_without_asyncify_are_rejected() -> anyhow::Result<()> {
    let mut store = Store::default();
    let module = Module::new(&store, r#"(module (memory (export "memory") 1))"#)?;
    let instance = Instance::new(&mut store, &module, &imports! {})?;
    assert!(Asyncify::from_instance(&store, &instance).is_err());
    Ok(())
}
