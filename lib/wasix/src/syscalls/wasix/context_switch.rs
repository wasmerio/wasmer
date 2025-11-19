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

/// Switch to another context
#[instrument(level = "trace", skip(ctx), ret)]
pub fn context_switch(
    mut ctx: FunctionEnvMut<WasiEnv>,
    target_context_id: u64,
) -> impl Future<Output = Result<Errno, RuntimeError>> + Send + 'static + use<> {
    let sync_part = inner_context_switch(ctx, target_context_id);
    async move {
        match sync_part {
            Ok(fut) => fut.await,
            Err(res) => res,
        }
    }
}

/// Helper function that allows us to return from the synchronous part early
fn inner_context_switch(
    mut ctx: FunctionEnvMut<WasiEnv>,
    target_context_id: u64,
) -> Result<
    impl Future<Output = Result<Errno, RuntimeError>> + Send + 'static + use<>,
    Result<Errno, RuntimeError>,
> {
    // TODO: Should we call do_pending_operations here?
    match WasiEnv::do_pending_operations(&mut ctx) {
        Ok(()) => {}
        Err(e) => {
            return Err(Err(RuntimeError::user(Box::new(e))));
        }
    }

    let (data) = ctx.data_mut();

    // Get own context ID
    let own_context_id = data
        .current_context_id
        .swap(target_context_id, Ordering::Relaxed);

    // If switching to self, do nothing
    if own_context_id == target_context_id {
        tracing::trace!("Switching context {own_context_id} to itself, which is a no-op");
        return Err(Ok(Errno::Success));
    }

    // Setup sender and receiver for the new context
    let (unblock, wait_for_unblock) = oneshot::channel::<Result<(), RuntimeError>>();

    // Store the unblock function into the WasiEnv
    let previous_unblock = data.contexts.insert(own_context_id, unblock);
    if previous_unblock.is_some() {
        // This should never happen, and if it does, it is an error in WASIX
        panic!("There is already a unblock present for the current context {own_context_id}");
    }

    // Unblock the other context
    let Some(unblock_target) = data.contexts.remove(&target_context_id).map(|(_, val)| val) else {
        tracing::trace!(
            "Context {own_context_id} tried to switch to context {target_context_id} but it does not exist or is not suspended"
        );
        return Err(Ok(Errno::Inval));
    };
    let Ok(_) = unblock_target.send(Ok(())) else {
        // This should never happen, and if it does, it is an error in WASIX
        // TODO: Handle cancellation properly
        panic!(
            "Context {own_context_id} failed to unblock target context {target_context_id}. This should never happen"
        );
    };

    // Clone necessary arcs for the future
    let current_context_id = data.current_context_id.clone();

    // Create the future that will resolve when this context is switched back to
    // again
    Ok(async move {
        // Wait until we are unblocked again
        let result = wait_for_unblock.await;

        // Restore our own context ID
        current_context_id.store(own_context_id, Ordering::Relaxed);

        // Handle if we were canceled instead of beeing unblocked
        let result = match result {
            Ok(v) => v,
            Err(canceled) => {
                tracing::trace!(
                    "Context {own_context_id} was canceled while it was suspended: {}",
                    canceled
                );
                // TODO: Handle cancellation properly
                panic!("Sender was dropped: {canceled}");
            }
        };

        // If we get relayed a trap, propagate it. Other wise return success
        result.and(Ok(Errno::Success))
    })
}
