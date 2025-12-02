use crate::{WasiEnv, WasiError};
use tracing::instrument;
use wasmer::{
    Function, FunctionEnvMut, MemorySize, RuntimeError, StoreMut, TypedFunction, Value, WasmPtr,
};
use wasmer_wasix_types::wasi::Errno;

/// Return the function corresponding to the given entrypoint index if it exists and has the signature `() -> ()`
pub fn lookup_typechecked_entrypoint(
    data: &WasiEnv,
    mut store: &mut StoreMut<'_>,
    entrypoint_id: u32,
) -> Result<TypedFunction<(), ()>, Errno> {
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
    let Ok(entrypoint) = entrypoint.typed::<(), ()>(&store) else {
        tracing::trace!(
            "Entrypoint function {entrypoint_id} has invalid signature: expected () -> (), got {:?} -> {:?}",
            entrypoint_type.params(),
            entrypoint_type.results()
        );
        return Err(Errno::Inval);
    };

    Ok(entrypoint)
}

/// Create a new context.
///
/// Creates a new context in the suspended state. On its first resumption,
/// `entrypoint` is invoked within that context.
///
/// Refer to the wasix-libc [`wasix/context.h`] header for authoritative
/// documentation.
///
/// [`wasix/context.h`]: https://github.com/wasix-org/wasix-libc/blob/main/libc-bottom-half/headers/public/wasix/context.h
#[instrument(level = "trace", skip(ctx), ret)]
pub fn context_create<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    new_context_ptr: WasmPtr<u64, M>,
    entrypoint: u32,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    // TODO: Unify this check with the one below for context_switching_context
    let async_store = match ctx.as_store_async() {
        Some(c) => c,
        None => {
            tracing::trace!("The current store is not async");
            return Ok(Errno::Again);
        }
    };

    let (data, mut store) = ctx.data_and_store_mut();

    // Verify that we are in an async context
    let environment = match &data.context_switching_environment {
        Some(c) => c,
        None => {
            tracing::trace!("Context switching is not enabled");
            return Ok(Errno::Again);
        }
    };

    // Lookup and check the entrypoint function
    let entrypoint = match lookup_typechecked_entrypoint(data, &mut store, entrypoint) {
        Ok(func) => func,
        Err(e) => {
            return Ok(e);
        }
    };

    // Create the new context
    let new_context_id = environment.create_context(|new_context_id| {
        // Sync part (not needed for now, but will make it easier to work with more complex entrypoints later)
        async move {
            // Call the entrypoint function
            let result: Result<(), RuntimeError> = entrypoint.call_async(&async_store).await;

            // If that function returns, we need to resume the main context with an error
            // Take the underlying error, or create a new error if the context returned a value
            result.map_or_else(
                |e| e,
                |v| {
                    // TODO: Proper error type
                    RuntimeError::user(
                format!(
                    "Context {new_context_id} returned a value ({v:?}). This is not allowed for now"
                )
                .into(),
            )
                },
            )
        }
    });

    // Write the new context ID into memory
    let memory = unsafe { data.memory_view(&store) };
    wasi_try_mem_ok!(new_context_ptr.write(&memory, new_context_id));

    // Return success
    return Ok(Errno::Success);
}
