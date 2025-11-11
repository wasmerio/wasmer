use std::collections::BTreeMap;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Once, OnceLock, RwLock};
use std::vec;
use std::{cell::RefCell, collections::HashMap};

use anyhow::Result;
use futures::{FutureExt, channel::oneshot, executor::block_on};
use wasmer::{
    AsStoreMut, Function, FunctionEnv, FunctionEnvMut, FunctionType, Instance, Memory, Module,
    Store, StoreMut, Type, Value, imports,
};

fn greenthread_module() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    const WAT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/examples/simple-greenthread.wat"
    );
    BYTES.get_or_init(|| wat::parse_file(WAT).expect("valid greenthread example module"))
}

struct Continuation {
    fn_id: i32,
    initialized: bool,
    // When this continuation is paused, we store a sender to resume it.
    resume_sender: Option<oneshot::Sender<()>>,
}

struct GreenEnv {
    continuations: HashMap<u32, Continuation>,
    current_id: u32,
    next_free_id: u32,
    logs: Vec<String>,
    memory: Option<Memory>,
    entrypoint: Option<Function>,
}

impl GreenEnv {
    fn new() -> Self {
        Self {
            continuations: HashMap::from([(
                0,
                Continuation {
                    fn_id: -1,
                    initialized: true,
                    resume_sender: None,
                },
            )]),
            current_id: 0,
            next_free_id: 1,
            logs: Vec::new(),
            memory: None,
            entrypoint: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoroutineState {
    Created,
    Active,
    Deleted,
    Failed,
}

struct CoroutineStack {
    entrypoint: Option<u32>,
    state: CoroutineState,
    resumer: Option<futures::channel::oneshot::Sender<()>>, // Placeholder for resumer lock
                                                            // pub resumer: Option<futures::lock::MutexGuard<'static, ()>>,
}

impl Clone for CoroutineStack {
    fn clone(&self) -> Self {
        if self.resumer.is_some() {
            panic!("Cannot clone a coroutine with a resumer");
        }
        Self {
            entrypoint: self.entrypoint,
            state: self.state,
            resumer: None, // Cannot clone the resumer
        }
    }
}

thread_local! {
    static CURRENT_COROUTINE_ID: std::cell::RefCell<u32> = std::cell::RefCell::new(0);
    static COROUTINES: RefCell<BTreeMap<u64, Arc<RwLock<CoroutineStack>>>> = Default::default();
    static ENTRYPOINT: OnceLock<Function> = OnceLock::new();
}
static FREE_COROUTINE_ID: AtomicU32 = AtomicU32::new(1);

pub struct SendWrapper<T>(pub T);
unsafe impl<T> Send for SendWrapper<T> {}
unsafe impl<T> Sync for SendWrapper<T> {}

#[test]
fn green_threads_switch_and_log_in_expected_order() -> Result<()> {
    let wasm = greenthread_module();
    let mut store = Store::default();
    let module = Module::new(&store, wasm)?;

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
            eprintln!("Log: {}", s);
            data.logs.push(s);
            Ok(vec![])
        },
    );

    // continuation_new(fn_id) -> id
    let continuation_new = Function::new_with_env(
        &mut store,
        &env,
        FunctionType::new(vec![Type::I32], vec![Type::I32]),
        |mut env: FunctionEnvMut<GreenEnv>, params: &[Value]| {
            eprintln!("Creating new continuation ");

            // let (env, mut store) = ctx.data_and_store_mut();
            // let function = env
            //     .inner()
            //     .indirect_function_table_lookup(&mut store, entrypoint)
            //     .expect("Function not found in table");
            let (data, mut store) = env.data_and_store_mut();

            let new_coroutine_id =
                FREE_COROUTINE_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

            // TODO: Support unstarted continuation references

            let function = ENTRYPOINT.with(|f| f.get().expect("entrypoint set").clone());
            let (sender, receiver) = oneshot::channel::<()>();

            let entrypoint_data = params[0].unwrap_i32() as u32;
            let new_coroutine = CoroutineStack {
                entrypoint: Some(entrypoint_data as u32),
                state: CoroutineState::Created,
                // resumer: Some(static_lifetime_lock),
                resumer: Some(sender),
            };

            COROUTINES.with_borrow_mut(|map| {
                map.insert(
                    new_coroutine_id as u64,
                    Arc::new(RwLock::new(new_coroutine)),
                )
            });

            let store_ref: StoreMut<'_> = store.as_store_mut();
            let fuckit = SendWrapper(store_ref.as_raw());

            tokio::task::spawn_local(async move {
                let fuckit = fuckit.0;
                let mut store = unsafe { StoreMut::<'static>::from_raw(fuckit) };
                receiver.await.unwrap();
                // Now run the function
                let resumer = function.call_async(&mut store, &[Value::I32(entrypoint_data as i32)]).await; // TODO: Handle params
                panic!("Coroutine function returned {:?}", resumer);
            });
            //   let function_id = coroutine.read().unwrap().entrypoint; // resolve function from index

            // function.call_async(store, params);

            // let env = ctx.data();
            // let memory = unsafe { env.memory_view(&ctx) };

            // let mut coroutines = env.coroutines.write().unwrap();
            // coroutines.insert(new_coroutine_id, Arc::new(RwLock::new(new_coroutine)));
            // new_coroutine_ptr
            //     .write(&memory, new_coroutine_id as u32)
            //     .unwrap();

            Ok(vec![Value::I32(new_coroutine_id as i32)])
        },
    );

    // continuation_switch(to_id) -> (suspends current continuation until resumed)
    let continuation_switch = Function::new_with_env_async(
        &mut store,
        &env,
        FunctionType::new(vec![Type::I32], Vec::<Type>::new()),
        |mut env: FunctionEnvMut<GreenEnv>, params: &[Value]| {
            let next_continuation_id = params[0].unwrap_i32() as u32;
            eprintln!("Switching to continuation {}", next_continuation_id);
            // let to_id = params[0].unwrap_i32() as u32;
            // let (data, mut storemut) = env.data_and_store_mut();
            // let from_id = data.current_id;
            // // Removed: guard that panics if already paused to allow nested switching
            // // if data.continuations.get(&from_id).and_then(|c| c.resume_sender.as_ref()).is_some() {
            // //     panic!("Current continuation already paused");
            // // }
            // // Prepare pause for current continuation
            // let (tx_pause, rx_pause) = oneshot::channel::<()>();
            // let (env, mut store) = ctx.data_and_store_mut();

            let current_coroutine_id =
                CURRENT_COROUTINE_ID.with(|c| c.replace(next_continuation_id));
            if current_coroutine_id == next_continuation_id {
                panic!("Switching to self is not allowed for now");
                // Switching to self could be a no-op

                // return Box::pin(async move { Ok(Errno::Success) });
                // return  Box::pin(async move {Ok(vec![Value::I32(0)])});
            }

            // Prepare a mutex to block on
            let (sender, receiver) = futures::channel::oneshot::channel::<()>();

            // Move the mutex guard to our own continuation
            // {
            //     .get_mut(&(current_coroutine_id as u64)).unwrap();
            //     let mut this_continuation = this_continuation.write().unwrap();
            //     if this_continuation.resumer.is_some() {
            //         panic!("Switching from a coroutine that is already switched out");
            //     }
            //     // this_continuation.resumer = Some(static_lifetime_lock);
            //     this_continuation.resumer = Some(());
            // }
            let this_continuation = COROUTINES.with_borrow_mut(|coroutines| {
                let mut this_one = coroutines
                    .get_mut(&(current_coroutine_id as u64))
                    .unwrap();
                let mut this_one = this_one
                    .write()
                    .unwrap();
                if this_one.resumer.is_some() {
                    panic!("Switching from a coroutine that is already switched out");
                }
                // this_continuation.resumer = Some(static_lifetime_lock);
                this_one.resumer = Some(sender);
            });

            let next_continuation = COROUTINES.with_borrow_mut(|coroutines| {
                let next_one = coroutines.get_mut(&(next_continuation_id as u64)).unwrap();
                let Some(next_one) = next_one.write().unwrap().resumer.take() else {
                    panic!("Switching to coroutine that has no resumer");
                };
                // this_continuation.resumer = Some(static_lifetime_lock);
                next_one.send(()).unwrap();
            });

            // Unlock the mutex from the next continuation
            // let next_continuation = coroutines
            //     .get_mut(&(next_continuation_id as u64))
            //     .expect("Switching to invalid coroutine is an error");
            // let Some(next_continuation_guard) = next_continuation.write().unwrap().resumer.take() else {
            //     panic!("Switching to coroutine that has no resumer");
            // };
            // drop(next_continuation_guard);

            async move {
                // Block until our own mutex is unlocked
                let _ = receiver.map(|_| ()).await;

                CURRENT_COROUTINE_ID.with(|c| {
                    *c.borrow_mut() = current_coroutine_id;
                });

                Ok(vec![])
            }
        },
    );
    // Ok(Errno::Success);
    //         async move {
    //             let _ = rx_pause.map(|_| ()).await;
    //             Ok(vec![])
    //         }
    //     },
    // );

    let import_object = imports! {
        "test" => {
            "log" => log_fn,
            "continuation_new" => continuation_new,
            "continuation_switch" => continuation_switch,
        }
    };

    let instance = Instance::new(&mut store, &module, &import_object)?;

    let entrypoint = instance.exports.get_function("entrypoint").unwrap().clone();
    ENTRYPOINT.with(move |a| a.set(entrypoint).unwrap());

    // Set memory and entrypoint in env
    let memory = instance.exports.get_memory("memory")?.clone();
    env.as_mut(&mut store).memory = Some(memory);
    // let entrypoint = instance.exports.get_function("entrypoint")?.clone();
    // env.as_mut(&mut store).entrypoint = Some(entrypoint);

    let main_fn = instance.exports.get_function("_main")?;

      let new_coroutine = CoroutineStack {
                entrypoint: None,
                state: CoroutineState::Created,
                // resumer: Some(static_lifetime_lock),
                resumer: None,
            };

            COROUTINES.with_borrow_mut(|map| {
                map.insert(
                    0 as u64,
                    Arc::new(RwLock::new(new_coroutine)),
                )
            });

    // Run main asynchronously
    let tokio_runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    tokio_runtime.block_on(async {
        let local_set = tokio::task::LocalSet::new();
        local_set.run_until(
        main_fn.call_async(&mut store, &[])).await.unwrap();
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
