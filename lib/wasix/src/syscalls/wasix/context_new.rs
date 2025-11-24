use super::*;
use crate::os::task::thread::context_switching::{ContextCancelled, ContextSwitchingContext};
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

    Ok(entrypoint)
}

async fn async_entrypoint(
    mut unsafe_static_store: StoreMut<'static>,
    contexts: Arc<ContextSwitchingContext>,
    own_context_id: u64,
    typechecked_entrypoint: Function,
) -> () {
    // Restore our own context ID
    contexts.set_active_context_id(own_context_id);

    // Actually call the entrypoint function
    let result = typechecked_entrypoint
        .call_async(&mut unsafe_static_store, &[])
        .await;

    // If that function returns, we need to resume the main context with an error
    // Take the underlying error, or create a new error if the context returned a value
    let error = match result {
        Err(e) => match e.downcast_ref::<ContextError>() {
            Some(s) => {
                tracing::trace!("Context {own_context_id} exited with error string: {}", s);
                // Context was cancelled, so we can just exit here.
                //
                // At this point we don't need to do anything else
                return;
            }
            None => {
                // Propagate the runtime error to main
                e
            }
        },
        Ok(v) => {
            // Not really sure how we should handle this case
            //
            // TODO: Handle returning functions with a real error type
            RuntimeError::user(
                format!(
                    "Context {own_context_id} returned a value ({v:?}). This is not allowed for now"
                )
                .into(),
            )
        }
    };

    // Retrieve the main context
    let main_context = contexts.remove_unblocker(&MAIN_CONTEXT_ID).expect(
        "The main context should always be suspended when another context returns or traps.",
    );

    // Resume the main context with the error
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

    // Verify that we are in an async context
    let contexts = match &data.context_switching_context {
        Some(c) => c,
        None => {
            tracing::trace!("Context switching is not enabled");
            return Ok(Errno::Again);
        }
    };

    // Lookup and check the entrypoint function
    let typechecked_entrypoint = match lookup_typechecked_entrypoint(data, &mut store, entrypoint) {
        Ok(func) => func,
        Err(e) => {
            return Ok(e);
        }
    };

    // Clone necessary arcs for the entrypoint future
    // SAFETY: Will be made safe with the proper wasmer async API
    let mut unsafe_static_store =
        unsafe { std::mem::transmute::<StoreMut<'_>, StoreMut<'static>>(store.as_store_mut()) };
    let contexts_cloned = contexts.clone();

    // Create the new context
    let new_context_id = contexts.new_context(|new_context_id| {
        async_entrypoint(
            unsafe_static_store,
            contexts_cloned,
            new_context_id,
            typechecked_entrypoint,
        )
    });

    // Write the new context ID into memory
    let memory = unsafe { data.memory_view(&store) };
    wasi_try_mem_ok!(new_context_ptr.write(&memory, new_context_id));

    // Return success
    return Ok(Errno::Success);
}
