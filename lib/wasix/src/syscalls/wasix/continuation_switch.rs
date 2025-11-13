use super::*;
use crate::{run_wasi_func, run_wasi_func_start, syscalls::*};
use anyhow::Result;
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

pub struct Greenthread {
    resumer: Option<oneshot::Sender<()>>,
}

impl Clone for Greenthread {
    fn clone(&self) -> Self {
        if self.resumer.is_some() {
            panic!("Cannot clone a coroutine with a resumer");
        }
        Self { resumer: None }
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
    let mut runtime_builder = tokio::runtime::Builder::new_current_thread();
    let runtime = runtime_builder.enable_all().build().unwrap();
    let local = tokio::task::LocalSet::new();
    let cloned_params = params.to_vec();

    let main_greenthread = Greenthread { resumer: None };

    let env = ctx.data_mut(store);
    env.greenthreads
        .write()
        .unwrap()
        .insert(0, main_greenthread);

    let mut localpool = futures::executor::LocalPool::new();
    let local_spawner = localpool.spawner();
    LOCAL_SPAWNER.with(|spawner_lock| {
        spawner_lock
            .set(local_spawner)
            .expect("Failed to set local spawner");
    });
    let result = localpool.run_until(entrypoint.call_async(&mut *store, &[]));

    result
}

/// ### `greenthread_delete()`
#[instrument(level = "trace", skip(ctx), ret)]
pub fn continuation_delete(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    coroutine: u32,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let memory: MemoryView<'_> = unsafe { env.memory_view(&ctx) };
    // TODO: implement

    Ok(Errno::Success)
}

/// ### `greenthread_new()`
#[instrument(level = "trace", skip(ctx), ret)]
pub fn continuation_new<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    new_coroutine_ptr: WasmPtr<u32, M>,
    entrypoint: u32,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let (data, mut store) = ctx.data_and_store_mut();
    let new_greenthread_id = data
        .next_free_id
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    let function = data
        .inner()
        .indirect_function_table_lookup(&mut store, entrypoint)
        .expect("Function not found in table");
    // let function = function.as_ref().unwrap_or(entrypoint);

    let (sender, receiver) = oneshot::channel::<()>();

    let new_greenthread = Greenthread {
        // entrypoint: Some(entrypoint_data),
        resumer: Some(sender),
    };

    data.greenthreads
        .write()
        .unwrap()
        .insert(new_greenthread_id, new_greenthread);

    // SAFETY: This is fine if we can ensure that ???
    //  A: The future does not outlive the store
    //  B: we now have multiple mutable references, this is dangerous
    let mut unsafe_static_store =
        unsafe { std::mem::transmute::<_, StoreMut<'static>>(store.as_store_mut()) };

    tokio::task::spawn_local(async move {
        receiver.await.unwrap();
        let resumer = function.call_async(&mut unsafe_static_store, &[]).await;
        panic!("Greenthread function returned {:?}", resumer);
    });

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    new_coroutine_ptr
        .write(&memory, new_greenthread_id as u32)
        .unwrap();

    Ok(Errno::Success)
}

/// Switch to another coroutine
// #[instrument(level = "trace", skip(ctx), ret)]
pub fn continuation_switch(
    mut ctx: FunctionEnvMut<WasiEnv>,
    next_greenthread_id: u32,
    // params: &[Value],
) -> impl Future<Output = Result<Errno, RuntimeError>> + Send + 'static + use<> {
    // let next_continuation_id = next_continuation_id
    //     .first()
    //     .expect("Expected one argument for continuation_switch")
    //     .unwrap_i32() as u32;

    match WasiEnv::do_pending_operations(&mut ctx) {
        Ok(()) => {}
        Err(e) => {
            // TODO: Move this to the end to only need a single async block
            panic!("Error in do_pending_operations");
            // return async move {Err(RuntimeError::user(Box::new(e)))}
        }
    }
    // let next_greenthread_id = params[0].unwrap_i32() as u32;

    let (data, _store) = ctx.data_and_store_mut();
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

        Ok(Errno::Success) // TODO: Errno::success
    }
}
