use super::*;
use crate::os::task::thread::context_switching::ContextSwitchError;
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
use thiserror::Error;
use wasmer::{
    AsStoreMut, Function, FunctionEnv, FunctionEnvMut, FunctionType, Instance, Memory, Module,
    RuntimeError, Store, Value, imports,
};
use wasmer::{StoreMut, Tag, Type};

/// Error type for errors internal to context switching
///
/// Will be returned as a RuntimeError::User
#[derive(Error, Debug)]
pub(crate) enum ContextError {
    // Should always be handled by the launch_entrypoint function and thus never propagated
    // to the user
    #[error("Context was cancelled. If you see this message, something went wrong.")]
    Cancelled,
}

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
///
/// The order of operations in here is quite delicate, so be careful when
/// modifying this function. It's important to not leave the env in
/// an inconsistent state.
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

    // Verify that we are in an async context
    let contexts = match &data.context_switching_context {
        Some(c) => c,
        None => {
            tracing::trace!("Context switching is not enabled");
            return Err(Ok(Errno::Again));
        }
    };

    // Get own context ID
    let own_context_id = contexts.active_context_id();

    // If switching to self, do nothing
    if own_context_id == target_context_id {
        tracing::trace!("Switching context {own_context_id} to itself, which is a no-op");
        return Err(Ok(Errno::Success));
    }

    // TODO: Move into switch
    // Setup sender and receiver for the new context
    let (unblock, wait_for_unblock) = oneshot::channel::<Result<(), RuntimeError>>();

    // Try to unblock the target and put our unblock function into the env, if successful
    match contexts.switch(target_context_id, unblock) {
        Ok(()) => {}
        Err(ContextSwitchError::SwitchTargetMissing) => {
            tracing::trace!(
                "Context {own_context_id} tried to switch to context {target_context_id} but it does not exist or is not suspended"
            );
            return Err(Ok(Errno::Inval));
        }
        Err(ContextSwitchError::OwnContextAlreadyBlocked) => {
            // This should never happen, because the active context should never have an unblock function (as it is not suspended)
            // If it does, it is an error in WASIX
            panic!("There is already a unblock present for the current context {own_context_id}");
        }
        Err(ContextSwitchError::SwitchUnblockFailed) => {
            // If there is no target to unblock, we assume it exited, but the unblock
            // function was not removed. For now we treat this like a missing context
            // It can't happen again, as we already removed the unblock function
            //
            // TODO: Think about whether this is correct
            tracing::trace!(
                "Context {own_context_id} tried to switch to context {target_context_id} but it could not be unblocked (perhaps it exited?)"
            );
            return Err(Ok(Errno::Inval));
        }
    };

    // Clone necessary arcs for the future
    let contexts_cloned = contexts.clone();
    // Create the future that will resolve when this context is switched back to again
    Ok(async move {
        // Wait until we are unblocked again
        let result = wait_for_unblock.await;
        // Restore our own context ID
        contexts_cloned.set_active_context_id(own_context_id);

        // Handle if we were canceled instead of beeing unblocked
        let result = match result {
            Ok(v) => v,
            Err(canceled) => {
                tracing::trace!(
                    "Context {own_context_id} was canceled while it was suspended: {}",
                    canceled
                );

                let err = ContextError::Cancelled.into();
                return Err(RuntimeError::user(err));
            }
        };

        // If we get relayed a trap, propagate it. Other wise return success
        result.and(Ok(Errno::Success))
    })
}
