use super::*;
use crate::state::MAIN_CONTEXT_ID;
use crate::{run_wasi_func, run_wasi_func_start, syscalls::*};
use anyhow::Result;
use core::panic;
use futures::TryFutureExt;
use futures::task::LocalSpawnExt;
use futures::{FutureExt, channel::oneshot};
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
    target_context_id: u64,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let memory: MemoryView<'_> = unsafe { env.memory_view(&ctx) };

    // TODO: Review which Ordering is appropriate here
    let own_context_id = env.current_context_id.load(Ordering::SeqCst);
    if own_context_id == target_context_id {
        tracing::trace!(
            "Context {} tried to delete itself, which is not allowed",
            target_context_id
        );
        return Ok(Errno::Inval);
    }

    if target_context_id == MAIN_CONTEXT_ID {
        tracing::trace!(
            "Context {} tried to delete the main context, which is not allowed",
            own_context_id
        );
        return Ok(Errno::Inval);
    }

    // TODO: actually delete the context
    let removed_future = env.contexts.remove(&target_context_id);
    let Some((_id, _val)) = removed_future else {
        // Context did not exist, so we do not need to remove it
        tracing::trace!(
            "Context {} tried to delete context {} but it is already removed",
            own_context_id,
            target_context_id
        );
        return Ok(Errno::Success);
    };

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

    let entrypoint = data
        .inner()
        .indirect_function_table_lookup(&mut store, entrypoint)
        .expect("Function not found in table");

    // Setup sender and receiver for the new context
    let (sender, receiver) = oneshot::channel::<Result<(), RuntimeError>>();
    let wait_for_unblock = receiver.unwrap_or_else(|_canceled| {
        // TODO: Handle canceled properly
        todo!("Context was canceled. Cleanup not implemented yet so we just panic");
    });
    let maybe_old_value = data.contexts.insert(new_context_id, sender);
    if maybe_old_value.is_some() {
        panic!(
            "Context ID {} was already taken when creatin new context",
            new_context_id
        );
    }

    // SAFETY: This is fine if we can ensure that ???
    //  A: The future does not outlive the store
    //  B: we now have multiple mutable references, this is dangerous
    let mut unsafe_static_store =
        unsafe { std::mem::transmute::<StoreMut<'_>, StoreMut<'static>>(store.as_store_mut()) };

    let contexts_cloned = data.contexts.clone();
    let spawner = data
        .current_spawner
        .clone()
        .expect("No async spawner set on WasiEnv. Did you enter the async env before?");
    spawner.spawn_local(async move {
        wait_for_unblock.await;
        let result = entrypoint.call_async(&mut unsafe_static_store, &[]).await;

        let main_context = contexts_cloned
            .remove(&MAIN_CONTEXT_ID)
            .map(|(_id, val)| val)
            .expect("The main context should always be suspended when another context returns.");

        // Take the underlying error, or create a new error if the context returned a value
        let error = match result {
            Err(e) => e,
            Ok(v) => {
                // TODO: Handle this properly
                RuntimeError::user(
                    format!("Context {new_context_id} returned a value ({v:?}). This is not allowed for now")
                    .into(),
                )
            }
        };
        main_context
            .send(Err(error))
            .expect("Failed to send error to main context, this should not happen");
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
    target_context_id: u64,
) -> impl Future<Output = Result<Errno, RuntimeError>> + Send + 'static + use<> {
    // TODO: Should we call do_pending_operations here?
    match WasiEnv::do_pending_operations(&mut ctx) {
        Ok(()) => {}
        Err(e) => {
            // TODO: Move this to the end to only need a single async block
            panic!("Error in do_pending_operations");
            // return async move {Err(RuntimeError::user(Box::new(e)))}
        }
    }

    let (data) = ctx.data_mut();
    // TODO: Review which Ordering is appropriate here
    let own_context_id = data
        .current_context_id
        .swap(target_context_id, Ordering::SeqCst);

    if own_context_id == target_context_id {
        panic!("Switching to self is not allowed");
    }

    let (sender, receiver) = oneshot::channel::<Result<(), RuntimeError>>();
    let wait_for_unblock = receiver.unwrap_or_else(|_canceled| {
        // TODO: Handle canceled properly
        todo!("Context was canceled. Cleanup not implemented yet so we just panic");
    });

    // let this_one = contexts.get_mut(&current_context_id).unwrap();
    // let receiver_promise = this_one.suspend();

    let maybe_old_value = data.contexts.insert(own_context_id, sender);
    if maybe_old_value.is_some() {
        panic!(
            "Context ID {} was already suspended when switching context to {}",
            own_context_id, target_context_id
        );
    }

    // Unblock the other context
    let unblock_target = data
        .contexts
        .remove(&target_context_id)
        .map(|(_id, val)| val)
        .expect("Context to switch to does not exist");
    unblock_target
        .send(Ok(()))
        .expect("Failed to unblock target context, this should not happen");

    let current_context_id = data.current_context_id.clone();
    async move {
        let result = wait_for_unblock.await;

        current_context_id.store(own_context_id, Ordering::SeqCst);

        // If we get relayed a trap, propagate it. Other wise return success
        result.and(Ok(Errno::Success))
    }
}
