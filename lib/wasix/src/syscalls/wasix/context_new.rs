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
