use std::collections::BTreeMap;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, RwLock};

use anyhow::Result;
use futures::{FutureExt, channel::oneshot};
use wasmer::{
    AsStoreMut, Function, FunctionEnv, FunctionEnvMut, FunctionType, Instance, Memory, Module,
    Store, StoreMut, Type, Value, imports,
};

const GREENTHREAD_WAT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/examples/simple-greenthread.wat"
));

struct GreenEnv {
    logs: Vec<String>,
    memory: Option<Memory>,
    greenthreads: Arc<RwLock<BTreeMap<u32, Greenthread>>>,
    current_greenthread_id: Arc<RwLock<u32>>,
    next_free_id: AtomicU32,
    entrypoint: Option<Function>,
}

impl GreenEnv {
    fn new() -> Self {
        Self {
            logs: Vec::new(),
            memory: None,
            greenthreads: Arc::new(RwLock::new(BTreeMap::new())),
            current_greenthread_id: Arc::new(RwLock::new(0)),
            next_free_id: AtomicU32::new(1),
            entrypoint: None,
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

pub struct SendWrapper<T>(pub T);
unsafe impl<T> Send for SendWrapper<T> {}
unsafe impl<T> Sync for SendWrapper<T> {}

#[test]
fn green_threads_switch_and_log_in_expected_order() -> Result<()> {
    let mut store = Store::default();
    let module = Module::new(&store, GREENTHREAD_WAT)?;

    let env = FunctionEnv::new(&mut store, GreenEnv::new());

    // log(ptr, len)
    let log_fn = Function::new_with_env(
        &mut store,
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

    // greenthread_new(fn_id) -> id
    let greenthread_new = Function::new_with_env(
        &mut store,
        &env,
        FunctionType::new(vec![Type::I32], vec![Type::I32]),
        |mut env: FunctionEnvMut<GreenEnv>, params: &[Value]| {
            let (data, mut store) = env.data_and_store_mut();
            let new_greenthread_id =
                data.next_free_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

            let function = data.entrypoint.clone().expect("entrypoint set");
            let (sender, receiver) = oneshot::channel::<()>();

            let entrypoint_data = params[0].unwrap_i32() as u32;
            let new_greenthread = Greenthread {
                entrypoint: Some(entrypoint_data),
                resumer: Some(sender),
            };

            data.greenthreads.write().unwrap().insert(new_greenthread_id, new_greenthread);

            let store_ref: StoreMut<'_> = store.as_store_mut();
            let store_raw = SendWrapper(store_ref.as_raw());

            tokio::task::spawn_local(async move {
                let mut store = unsafe { StoreMut::<'static>::from_raw(store_raw.0) };
                receiver.await.unwrap();
                let resumer = function
                    .call_async(&mut store, &[Value::I32(entrypoint_data as i32)])
                    .await;
                panic!("Greenthread function returned {:?}", resumer);
            });

            Ok(vec![Value::I32(new_greenthread_id as i32)])
        },
    );

    // greenthread_switch(to_id) -> (suspends current greenthread until resumed)
    let greenthread_switch = Function::new_with_env_async(
        &mut store,
        &env,
        FunctionType::new(vec![Type::I32], Vec::<Type>::new()),
        |mut env: FunctionEnvMut<GreenEnv>, params: &[Value]| {
            let next_greenthread_id = params[0].unwrap_i32() as u32;

            let (data, _store) = env.data_and_store_mut();
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

            async move {
                let _ = receiver.map(|_| ()).await;

                *current_id_arc.write().unwrap() = current_greenthread_id;

                Ok(vec![])
            }
        },
    );

    let import_object = imports! {
        "test" => {
            "log" => log_fn,
            "greenthread_new" => greenthread_new,
            "greenthread_switch" => greenthread_switch,
        }
    };

    let instance = Instance::new(&mut store, &module, &import_object)?;

    let entrypoint = instance.exports.get_function("entrypoint")?.clone();
    env.as_mut(&mut store).entrypoint = Some(entrypoint);

    let memory = instance.exports.get_memory("memory")?.clone();
    env.as_mut(&mut store).memory = Some(memory);

    let main_fn = instance.exports.get_function("_main")?;

    let main_greenthread = Greenthread {
        entrypoint: None,
        resumer: None,
    };

    env.as_mut(&mut store).greenthreads.write().unwrap().insert(0, main_greenthread);

    // Run main asynchronously
    let tokio_runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    tokio_runtime.block_on(async {
        let local_set = tokio::task::LocalSet::new();
        local_set
            .run_until(main_fn.call_async(&mut store, &[]))
            .await
            .unwrap();
    });

    // Expected log order
    let expected = [
        "[gr1] main  -> test1",
        "[gr2] test1 -> test2",
        "[gr1] test1 <- test2",
        "[gr2] test1 -> test2",
        "[gr1] test1 <- test2",
        "[main] main <- test1",
    ];

    let logs = &env.as_ref(&store).logs;
    assert_eq!(
        logs.len(),
        expected.len(),
        "Unexpected number of log entries: {:?}",
        logs
    );
    for (i, exp) in expected.iter().enumerate() {
        assert_eq!(logs[i], *exp, "Log entry mismatch at index {i}: {:?}", logs);
    }

    Ok(())
}
