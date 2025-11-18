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
