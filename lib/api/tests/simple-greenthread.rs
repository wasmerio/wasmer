use std::collections::BTreeMap;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, RwLock};

use anyhow::Result;
use futures::task::LocalSpawnExt;
use futures::{FutureExt, channel::oneshot};
use wasmer::{
    AsyncFunctionEnvMut, Function, FunctionEnv, FunctionEnvMut, FunctionType, Instance, Memory,
    Module, RuntimeError, Store, Type, Value, imports,
};

struct GreenEnv {
    logs: Vec<String>,
    memory: Option<Memory>,
    greenthreads: Arc<RwLock<BTreeMap<u32, Greenthread>>>,
    current_greenthread_id: Arc<RwLock<u32>>,
    next_free_id: AtomicU32,
    entrypoint: Option<Function>,
    spawner: Option<futures::executor::LocalSpawner>,
}

// Required for carrying the spawner around. Safe because we don't do threads.
// It worked before with a thread-local! spawner, so this is equivalent.
// The thread-local version does not work with multiple tests
unsafe impl Send for GreenEnv {}
unsafe impl Sync for GreenEnv {}

impl GreenEnv {
    fn new() -> Self {
        Self {
            logs: Vec::new(),
            memory: None,
            greenthreads: Arc::new(RwLock::new(BTreeMap::new())),
            current_greenthread_id: Arc::new(RwLock::new(0)),
            next_free_id: AtomicU32::new(1),
            entrypoint: None,
            spawner: None,
        }
    }
}

struct Greenthread {
    entrypoint: Option<u32>,
    resumer: Option<oneshot::Sender<()>>,
}

impl Clone for Greenthread {
    fn clone(&self) -> Self {
        if self.resumer.is_some() {
            panic!("Cannot clone a greenthread with a resumer");
        }
        Self {
            entrypoint: self.entrypoint,
            resumer: None,
        }
    }
}

fn greenthread_new(
    mut env: FunctionEnvMut<GreenEnv>,
    entrypoint_data: u32,
) -> core::result::Result<u32, RuntimeError> {
    let async_store = env.as_async_store();
    let data = env.data_mut();
    let new_greenthread_id = data
        .next_free_id
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    let function = data.entrypoint.clone().expect("entrypoint set");
    let (sender, receiver) = oneshot::channel::<()>();

    let new_greenthread = Greenthread {
        entrypoint: Some(entrypoint_data),
        resumer: Some(sender),
    };

    data.greenthreads
        .write()
        .unwrap()
        .insert(new_greenthread_id, new_greenthread);

    let spawner = env.data().spawner.as_ref().expect("spawner set").clone();
    spawner
        .spawn_local(async move {
            receiver.await.unwrap();
            let resumer = function
                .call_async(&async_store, &[Value::I32(entrypoint_data as i32)])
                .await;
            panic!("Greenthread function returned {:?}", resumer);
        })
        .unwrap();

    Ok(new_greenthread_id)
}

async fn greenthread_switch(env: AsyncFunctionEnvMut<GreenEnv>, next_greenthread_id: u32) {
    let (receiver, current_id_arc, current_greenthread_id) = {
        let mut write_lock = env.write().await;
        let data = write_lock.data_mut();

        let current_greenthread_id = {
            let mut current = data.current_greenthread_id.write().unwrap();
            let old = *current;
            *current = next_greenthread_id;
            old
        };

        if current_greenthread_id == next_greenthread_id {
            panic!("Switching to self is not allowed");
        }

        let (sender, receiver) = oneshot::channel::<()>();

        {
            let mut greenthreads = data.greenthreads.write().unwrap();
            let this_one = greenthreads.get_mut(&current_greenthread_id).unwrap();
            if this_one.resumer.is_some() {
                panic!("Switching from a greenthread that is already switched out");
            }
            this_one.resumer = Some(sender);
        }

        {
            let mut greenthreads = data.greenthreads.write().unwrap();
            let next_one = greenthreads.get_mut(&next_greenthread_id).unwrap();
            let Some(resumer) = next_one.resumer.take() else {
                panic!("Switching to greenthread that has no resumer");
            };
            resumer.send(()).unwrap();
        }
        let current_id_arc = data.current_greenthread_id.clone();

        (receiver, current_id_arc, current_greenthread_id)
    };

    let _ = receiver.map(|_| ()).await;

    *current_id_arc.write().unwrap() = current_greenthread_id;
}

fn run_greenthread_test(wat: &[u8]) -> Result<Vec<String>> {
    let mut store = Store::default();
    let module = Module::new(&store.engine(), wat)?;

    let mut store_mut = store.as_mut();
    let env = FunctionEnv::new(&mut store_mut, GreenEnv::new());

    // log(ptr, len)
    let log_fn = Function::new_with_env(
        &mut store_mut,
        &env,
        FunctionType::new(vec![Type::I32, Type::I32], vec![]),
        |mut env: FunctionEnvMut<GreenEnv>, params: &[Value]| {
            let ptr = params[0].unwrap_i32() as u32;
            let len = params[1].unwrap_i32() as u32;
            let (data, storemut) = env.data_and_store_mut();
            let memory = data.memory.as_ref().expect("memory set");
            let view = memory.view(&storemut);
            let mut bytes = Vec::with_capacity(len as usize);
            for i in ptr..ptr + len {
                bytes.push(view.read_u8(i as u64).expect("in bounds"));
            }
            let s = String::from_utf8_lossy(&bytes).to_string();
            data.logs.push(s);
            Ok(vec![])
        },
    );

    let greenthread_new = Function::new_typed_with_env(&mut store_mut, &env, greenthread_new);

    let greenthread_switch =
        Function::new_typed_with_env_async(&mut store_mut, &env, greenthread_switch);

    let import_object = imports! {
        "test" => {
            "log" => log_fn,
            "greenthread_new" => greenthread_new,
            "greenthread_switch" => greenthread_switch,
        }
    };

    let instance = Instance::new(&mut store_mut, &module, &import_object)?;

    let entrypoint = instance.exports.get_function("entrypoint")?.clone();
    env.as_mut(&mut store_mut).entrypoint = Some(entrypoint);

    let memory = instance.exports.get_memory("memory")?.clone();
    env.as_mut(&mut store_mut).memory = Some(memory);

    let main_fn = instance.exports.get_function("_main")?;

    let main_greenthread = Greenthread {
        entrypoint: None,
        resumer: None,
    };

    env.as_mut(&mut store_mut)
        .greenthreads
        .write()
        .unwrap()
        .insert(0, main_greenthread);

    let mut localpool = futures::executor::LocalPool::new();
    let local_spawner = localpool.spawner();
    env.as_mut(&mut store_mut).spawner = Some(local_spawner);

    drop(store_mut);

    localpool
        .run_until(main_fn.call_async(&store, &[]))
        .unwrap();

    let mut store_mut = store.as_mut();
    return Ok(env.as_ref(&mut store_mut).logs.clone());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn green_threads_switch_and_log_in_expected_order() -> Result<()> {
    let logs = run_greenthread_test(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/examples/simple-greenthread.wat"
    )))?;

    // Expected logs
    let expected = [
        "[gr1] main  -> test1",
        "[gr2] test1 -> test2",
        "[gr1] test1 <- test2",
        "[gr2] test1 -> test2",
        "[gr1] test1 <- test2",
        "[main] main <- test1",
    ];

    assert_eq!(logs.len(), expected.len(),);
    for (i, exp) in expected.iter().enumerate() {
        assert_eq!(logs[i], *exp, "Log entry mismatch at index {i}: {:?}", logs);
    }

    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn green_threads_switch_main_crashed() -> Result<()> {
    let logs = run_greenthread_test(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/examples/simple-greenthread2.wat"
    )))?;

    // Expected logs
    let expected = [
        "[main] switching to side",
        "[side] switching to main",
        "[main] switching to side",
        "[side] switching to main",
    ];

    eprintln!("logs: {:?}", logs);
    assert_eq!(logs.len(), expected.len());
    for (i, exp) in expected.iter().enumerate() {
        assert_eq!(logs[i], *exp, "Log entry mismatch at index {i}: {:?}", logs);
    }

    Ok(())
}
