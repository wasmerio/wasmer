use crate::{WasiEnv, WasiError};
use futures::FutureExt;
use tracing::instrument;
use wasmer::{
    AsStoreRef, Function, FunctionEnvMut, MemorySize, RuntimeError, StoreMut, TypedFunction, Value,
    WasmPtr,
};
use wasmer_wasix_types::wasi::Errno;

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

    // TODO: Remove this check and return a TypedFunction once all backends support types
    #[cfg(not(feature = "js"))]
    {
        let entrypoint_type = entrypoint.ty(&store);
        if !entrypoint_type.params().is_empty() && !entrypoint_type.results().is_empty() {
            tracing::trace!(
                "Entrypoint function {entrypoint_id} has invalid signature: expected () -> (), got {:?} -> {:?}",
                entrypoint_type.params(),
                entrypoint_type.results()
            );
            return Err(Errno::Inval);
        }
    }

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

    // Verify that we are in an async context
    // We need to do this first, before we borrow the store mutably
    let Some(async_store) = ctx.as_store_async() else {
        tracing::warn!(
            "The WASIX context-switching API is only available in engines supporting async execution"
        );
        return Ok(Errno::Notsup);
    };

    let (data, mut store) = ctx.data_and_store_mut();

    // Get the context-switching environment
    let Some(environment) = &data.context_switching_environment else {
        tracing::warn!(
            "The WASIX context-switching API is only available in a context-switching environment"
        );
        return Ok(Errno::Notsup);
    };

    // Lookup and check the entrypoint function
    let entrypoint = match lookup_typechecked_entrypoint(data, &mut store, entrypoint) {
        Ok(func) => func,
        Err(err) => {
            return Ok(err);
        }
    };

    // Create the new context
    let new_context_id = environment.create_context(
        entrypoint
            .call_async(&async_store, vec![])
            .map(|r| r.map(|_| ())),
    );

    // Write the new context ID into memory
    let memory = unsafe { data.memory_view(&store) };
    wasi_try_mem_ok!(new_context_ptr.write(&memory, new_context_id));

    // Return success
    return Ok(Errno::Success);
}
