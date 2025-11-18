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

/// ### `context_delete()`
#[instrument(level = "trace", skip(ctx), ret)]
pub fn context_delete(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    target_context_id: u64,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let memory: MemoryView<'_> = unsafe { env.memory_view(&ctx) };

    // TODO: Review which Ordering is appropriate here
    let own_context_id = env.current_context_id.load(Ordering::SeqCst);
    if own_context_id == target_context_id {
        tracing::trace!(
            "Context {} tried to delete itself, which is not allowed",
            target_context_id
        );
        return Ok(Errno::Inval);
    }

    if target_context_id == MAIN_CONTEXT_ID {
        tracing::trace!(
            "Context {} tried to delete the main context, which is not allowed",
            own_context_id
        );
        return Ok(Errno::Inval);
    }

    // TODO: actually delete the context
    let removed_future = env.contexts.remove(&target_context_id);
    let Some((_id, _val)) = removed_future else {
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
