use super::*;
use crate::{run_wasi_func, run_wasi_func_start, syscalls::*};
use anyhow::Result;
pub use context::Context;
use core::panic;
use futures::task::LocalSpawnExt;
use futures::{FutureExt, channel::oneshot};
use rkyv::vec;
use std::collections::BTreeMap;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, OnceLock, RwLock};
use wasmer::{
    AsStoreMut, Function, FunctionEnv, FunctionEnvMut, FunctionType, Instance, Memory, Module,
    RuntimeError, Store, Value, imports,
};
use wasmer::{StoreMut, Tag, Type};

mod context {
    use futures::{FutureExt, channel::oneshot};
    use wasmer::RuntimeError;

    pub struct Context {
        resumer: Option<oneshot::Sender<Result<(), RuntimeError>>>,
    }

    impl Context {
        // Create a new non-suspended context
        pub fn new() -> Self {
            Self { resumer: None }
        }
        // Lock this context until resumed
        //
        // Panics if the context is already locked
        pub fn suspend(&mut self) -> impl Future<Output = Result<(), RuntimeError>> + use<> {
            let (sender, receiver) = oneshot::channel();
            if self.resumer.is_some() {
                panic!("Switching from a context that is already switched out");
            }
            self.resumer = Some(sender);
            receiver.map(|r| {
                match r {
                    Ok(v) => v,
                    Err(_canceled) => {
                        // TODO: Handle canceled properly
                        panic!("Context was canceled");
                    }
                }
            })
            // TODO: Think about whether canceled should be handled
        }

        // Allow this context to be resumed
        pub fn resume(&mut self, value: Result<(), RuntimeError>) -> () {
            let resumer = self
                .resumer
                .take()
                .expect("Resuming a context that is not switched out");
            resumer.send(value).unwrap();
        }
    }
    impl Clone for Context {
        fn clone(&self) -> Self {
            if self.resumer.is_some() {
                panic!("Cannot clone a context with a resumer");
            }
            Self { resumer: None }
        }
    }
}

thread_local! {
    static LOCAL_SPAWNER: OnceLock<futures::executor::LocalSpawner> = OnceLock::new();
}
fn spawn_local<F>(future: F)
where
    F: std::future::Future<Output = ()> + 'static,
{
    LOCAL_SPAWNER.with(|spawner_lock| {
        let spawner = spawner_lock.get().expect("Local spawner not initialized");
        spawner
            .spawn_local(future)
            .expect("Failed to spawn local future");
    });
}

#[instrument(level = "trace", skip(ctx, store), ret)]
pub fn call_in_async_runtime<'a>(
    ctx: &WasiFunctionEnv,
    store: &mut Store,
    entrypoint: wasmer::Function,
    params: &'a [wasmer::Value],
) -> Result<Box<[Value]>, RuntimeError> {
    let cloned_params = params.to_vec();

    let main_context = Context::new();

    let env = ctx.data_mut(store);
    env.contexts.write().unwrap().insert(0, main_context);

    let mut localpool = futures::executor::LocalPool::new();
    let local_spawner = localpool.spawner();
    LOCAL_SPAWNER.with(|spawner_lock| {
        spawner_lock
            .set(local_spawner)
            .expect("Failed to set local spawner");
    });
    let result = localpool.run_until(entrypoint.call_async(&mut *store, &cloned_params));

    result
}

/// ### `context_delete()`
#[instrument(level = "trace", skip(ctx), ret)]
pub fn context_delete(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    context_id: u64,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let memory: MemoryView<'_> = unsafe { env.memory_view(&ctx) };
    // TODO: implement

    Ok(Errno::Success)
}

/// ### `context_new()`
#[instrument(level = "trace", skip(ctx), ret)]
pub fn context_new<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    new_context_ptr: WasmPtr<u64, M>,
    entrypoint: u32,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let (data, mut store) = ctx.data_and_store_mut();

    let new_context_id = data
        .next_available_context_id
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    let function = data
        .inner()
        .indirect_function_table_lookup(&mut store, entrypoint)
        .expect("Function not found in table");

    let mut new_context = Context::new();
    let new_context_resume = new_context.suspend();

    data.contexts
        .write()
        .unwrap()
        .insert(new_context_id, new_context);

    // SAFETY: This is fine if we can ensure that ???
    //  A: The future does not outlive the store
    //  B: we now have multiple mutable references, this is dangerous
    let mut unsafe_static_store =
        unsafe { std::mem::transmute::<_, StoreMut<'static>>(store.as_store_mut()) };

    let contexts_arc = data.contexts.clone();

    spawn_local(async move {
        new_context_resume.await;
        let result = function.call_async(&mut unsafe_static_store, &[]).await;

        let mut main_context = contexts_arc.write().unwrap();
        let main_context = main_context.get_mut(&0).unwrap();
        match result {
            Err(e) => {
                eprintln!("Context {} returned error {:?}", new_context_id, e);
                main_context.resume(Err(e));
            }
            Ok(v) => {
                panic!(
                    "Context {} returned a value ({:?}). This is not allowed for now",
                    new_context_id, v
                );
                // TODO: Handle this properly
            }
        }
        // TODO: Delete own context
    });

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    new_context_ptr.write(&memory, new_context_id).unwrap();

    Ok(Errno::Success)
}

/// Switch to another context
#[instrument(level = "trace", skip(ctx), ret)]
pub fn context_switch(
    mut ctx: FunctionEnvMut<WasiEnv>,
    next_context_id: u64,
) -> impl Future<Output = Result<Errno, RuntimeError>> + Send + 'static + use<> {
    match WasiEnv::do_pending_operations(&mut ctx) {
        Ok(()) => {}
        Err(e) => {
            // TODO: Move this to the end to only need a single async block
            panic!("Error in do_pending_operations");
            // return async move {Err(RuntimeError::user(Box::new(e)))}
        }
    }

    let (data, _store) = ctx.data_and_store_mut();
    let current_context_id = {
        let mut current = data.current_context_id.write().unwrap();
        let old = *current;
        *current = next_context_id;
        old
    };

    if current_context_id == next_context_id {
        panic!("Switching to self is not allowed");
    }

    let mut contexts = data.contexts.write().unwrap();
    let this_one = contexts.get_mut(&current_context_id).unwrap();
    let receiver_promise = this_one.suspend();

    let next_one = contexts.get_mut(&next_context_id).unwrap();
    next_one.resume(Ok(()));

    let current_id_arc = data.current_context_id.clone();

    async move {
        let result = receiver_promise.await;

        *current_id_arc.write().unwrap() = current_context_id;

        // If we get relayed a trap, propagate it. Other wise return success
        result.and(Ok(Errno::Success))
    }
}
