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

    let mut new_context = crate::state::Context::new();
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

    let spawner = data
        .current_spawner
        .clone()
        .expect("No async spawner set on WasiEnv. Did you enter the async env before?");

    spawner.spawn_local(async move {
        new_context_resume.await;
        let result = function.call_async(&mut unsafe_static_store, &[]).await;

        let mut main_context = contexts_arc.write().unwrap();
        let main_context = main_context.get_mut(&0).unwrap();
        match result {
            Err(e) => {
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
