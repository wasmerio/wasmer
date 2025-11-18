use super::*;
use crate::state::MAIN_CONTEXT_ID;
use crate::utils::thread_local_executor::ThreadLocalSpawnerError;
use crate::{run_wasi_func, run_wasi_func_start, syscalls::*};
use core::panic;
use futures::TryFutureExt;
use futures::channel::oneshot::{Receiver, Sender};
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

/// Return the function corresponding to the given entrypoint index if it exists and has the signature `() -> ()`
pub fn lookup_typechecked_entrypoint(
    data: &WasiEnv,
    mut store: &mut StoreMut<'_>,
    entrypoint_id: u32,
) -> Result<Function, Errno> {
    let entrypoint = match data
        .inner()
        .indirect_function_table_lookup(&mut store, entrypoint_id)
    {
        Ok(func) => func,
        Err(e) => {
            tracing::trace!(
                "Failed to lookup entrypoint function {}: {:?}",
                entrypoint_id,
                e
            );
            return Err(Errno::Inval);
        }
    };

    let entrypoint_type = entrypoint.ty(&store);
    if !entrypoint_type.params().is_empty() || !entrypoint_type.results().is_empty() {
        tracing::trace!(
            "Entrypoint function {} has invalid signature: expected () -> (), got {:?} -> {:?}",
            entrypoint_id,
            entrypoint_type.params(),
            entrypoint_type.results()
        );
        return Err(Errno::Inval);
    }

    return Ok(entrypoint);
}

async fn launch_function(
    mut unsafe_static_store: StoreMut<'static>,
    wait_for_unblock: Receiver<Result<(), RuntimeError>>,
    current_context_id: Arc<AtomicU64>,
    new_context_id: u64,
    contexts_cloned: Arc<dashmap::DashMap<u64, Sender<Result<(), RuntimeError>>>>,
    typechecked_entrypoint: Function,
) -> () {
    // Wait for the context to be unblocked
    let prelaunch_result = wait_for_unblock.await;
    current_context_id.store(new_context_id, Ordering::SeqCst);

    match prelaunch_result {
        Ok(_) => (),
        Err(canceled) => {
            tracing::trace!(
                "Context {new_context_id} was canceled before it even started: {canceled}",
            );
            // TODO: Handle cancellation properly
            panic!("Sender was dropped: {canceled}");
        }
    };

    let result = typechecked_entrypoint
        .call_async(&mut unsafe_static_store, &[])
        .await;

    let main_context = contexts_cloned
        .remove(&MAIN_CONTEXT_ID)
        .map(|(_id, val)| val)
        .expect("The main context should always be suspended when another context returns.");

    // Take the underlying error, or create a new error if the context returned a value
    let error = match result {
        Err(e) => e,
        Ok(v) => {
            // TODO: Handle returning functions with a real error type
            RuntimeError::user(
                format!(
                    "Context {new_context_id} returned a value ({v:?}). This is not allowed for now"
                )
                .into(),
            )
        }
    };
    main_context
        .send(Err(error))
        .expect("Failed to send error to main context, this should not happen");
}

#[instrument(level = "trace", skip(ctx), ret)]
pub fn context_new<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    new_context_ptr: WasmPtr<u64, M>,
    entrypoint: u32,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let (data, mut store) = ctx.data_and_store_mut();

    // Lookup and check the entrypoint function
    let typechecked_entrypoint = match lookup_typechecked_entrypoint(data, &mut store, entrypoint) {
        Ok(func) => func,
        Err(e) => {
            return Ok(e);
        }
    };

    // Verify that we are in an async context
    let Some(spawner) = data.current_spawner.clone() else {
        tracing::trace!("No async spawner set on WasiEnv. Did you enter the async env before?");
        return Ok(Errno::Again);
    };

    // TODO: Review which Ordering is appropriate here
    let new_context_id = data
        .next_available_context_id
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    {
        let memory = unsafe { data.memory_view(&mut store) };
        wasi_try_mem_ok!(new_context_ptr.write(&memory, new_context_id));
    }

    // Setup sender and receiver for the new context
    let (sender, wait_for_unblock) = oneshot::channel::<Result<(), RuntimeError>>();

    let maybe_old_value = data.contexts.insert(new_context_id, sender);
    if maybe_old_value.is_some() {
        // This should never happen, and if it does, it is an error in WASIX
        panic!(
            "There already is a context suspended with ID {new_context_id}. How did we get here?",
        );
    }

    // SAFETY: This is fine if we can ensure that ???
    //  A: The future does not outlive the store
    //  B: we now have multiple mutable references, this is dangerous
    let mut unsafe_static_store =
        unsafe { std::mem::transmute::<StoreMut<'_>, StoreMut<'static>>(store.as_store_mut()) };

    let contexts_cloned: Arc<
        dashmap::DashMap<u64, oneshot::Sender<std::result::Result<(), RuntimeError>>>,
    > = data.contexts.clone();
    let current_context_id: Arc<AtomicU64> = data.current_context_id.clone();

    let entrypoint_future = launch_function(
        unsafe_static_store,
        wait_for_unblock,
        current_context_id,
        new_context_id,
        contexts_cloned,
        typechecked_entrypoint,
    );
    // Queue the
    match spawner.spawn_local(entrypoint_future) {
        Ok(()) => Ok(Errno::Success),
        Err(ThreadLocalSpawnerError::LocalPoolShutDown) => {
            // TODO: Handle cancellation properly
            panic!(
                "Failed to spawn context {new_context_id} because the local executor has been shut down",
            );
        }
        Err(ThreadLocalSpawnerError::NotOnTheCorrectThread { expected, found }) => {
            // Not on the correct host thread. If this error happens, it is a bug in WASIX.
            panic!(
                "Failed to spawn context {new_context_id} because the current thread ({found:?}) is not the expected thread ({expected:?}) for the local executor"
            )
        }
        Err(ThreadLocalSpawnerError::SpawnError) => {
            // This should never happen
            panic!("Failed to spawn_local context {new_context_id} , this should not happen");
        }
    }
}
