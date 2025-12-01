use super::*;
use crate::syscalls::*;
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

/// Destroy a suspended or terminated context
///
/// After a successful call its identifier becomes invalid and its
/// resources are released. Destroying an already deleted context is a no-op.
///
/// Refer to the wasix-libc [`wasix/context.h`] header for authoritative
/// documentation.
///
/// [`wasix/context.h`]: https://github.com/wasix-org/wasix-libc/blob/main/libc-bottom-half/headers/public/wasix/context.h
#[instrument(level = "trace", skip(ctx), ret)]
pub fn context_destroy(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    target_context_id: u64,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let memory: MemoryView<'_> = unsafe { env.memory_view(&ctx) };

    // Verify that we are in an async context
    let contexts = match &env.context_switching_context {
        Some(c) => c,
        None => {
            tracing::trace!("Context switching is not enabled");
            return Ok(Errno::Again);
        }
    };

    let own_context_id = contexts.active_context_id();
    let main_context_id = contexts.main_context_id();

    if own_context_id == target_context_id {
        tracing::trace!(
            "Context {} tried to delete itself, which is not allowed",
            target_context_id
        );
        return Ok(Errno::Inval);
    }

    if target_context_id == main_context_id {
        tracing::trace!(
            "Context {} tried to delete the main context, which is not allowed",
            own_context_id
        );
        return Ok(Errno::Inval);
    }

    let removed_future = contexts.remove_unblocker(&target_context_id);
    // As soon as the Sender is dropped, the corresponding context will be able unblocked,
    // the executor will continue executing it. The context will respond to the
    // cancelation by terminating gracefully.

    let Some(_) = removed_future else {
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
