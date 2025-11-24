use super::*;
use crate::os::task::thread::context_switching::ContextSwitchError;
use crate::{run_wasi_func, run_wasi_func_start, syscalls::*};
use MaybeLater::{Later, Now};
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

/// Switch to another context
#[instrument(level = "trace", skip(ctx))]
pub fn context_switch(
    mut ctx: FunctionEnvMut<WasiEnv>,
    target_context_id: u64,
) -> impl Future<Output = Result<Errno, RuntimeError>> + Send + 'static + use<> {
    inner_context_switch(ctx, target_context_id).future()
}

enum MaybeLater<
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static = Result<Errno, RuntimeError>,
> {
    Now(T),
    Later(F),
}
impl<F: Future<Output = T> + Send + 'static, T: Send + 'static> MaybeLater<F, T> {
    fn future(self) -> impl Future<Output = T> + Send + 'static {
        async move {
            match self {
                MaybeLater::Now(v) => v,
                MaybeLater::Later(fut) => fut.await,
            }
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
) -> MaybeLater<impl Future<Output = Result<Errno, RuntimeError>> + Send + 'static + use<>> {
    // TODO: Should we call do_pending_operations here?
    match WasiEnv::do_pending_operations(&mut ctx) {
        Ok(()) => {}
        Err(e) => {
            return Now(Err(RuntimeError::user(Box::new(e))));
        }
    }

    let (data) = ctx.data_mut();

    // Verify that we are in an async context
    let contexts = match &data.context_switching_context {
        Some(c) => c,
        None => {
            tracing::trace!("Context switching is not enabled");
            return Now(Ok(Errno::Again));
        }
    };

    // Get own context ID
    let own_context_id = contexts.active_context_id();

    // If switching to self, do nothing
    if own_context_id == target_context_id {
        tracing::trace!("Switching context {own_context_id} to itself, which is a no-op");
        return Now(Ok(Errno::Success));
    }

    // Try to unblock the target and put our unblock function into the env, if successful
    let wait_for_unblock = match contexts.switch(target_context_id) {
        Ok(wait_for_unblock) => wait_for_unblock,
        Err(ContextSwitchError::SwitchTargetMissing) => {
            tracing::trace!(
                "Context {own_context_id} tried to switch to context {target_context_id} but it does not exist or is not suspended"
            );
            return Now(Ok(Errno::Inval));
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
            return Now(Ok(Errno::Inval));
        }
    };

    // Wait until we are unblocked again
    Later(wait_for_unblock.map(|v| v.map(|_| Errno::Success)))
}
