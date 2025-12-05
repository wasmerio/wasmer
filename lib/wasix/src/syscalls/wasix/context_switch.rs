use crate::{WasiEnv, WasiError, state::context_switching::ContextSwitchError};
use futures::FutureExt;
use tracing::instrument;
use wasmer::{AsyncFunctionEnvMut, FunctionEnvMut, RuntimeError};
use wasmer_wasix_types::wasi::Errno;

/// Suspend the active context and resume another
///
/// The resumed context continues from where it was last suspended, or from its
/// entrypoint if it has never been resumed.
///
/// Refer to the wasix-libc [`wasix/context.h`] header for authoritative
/// documentation.
///
/// [`wasix/context.h`]: https://github.com/wasix-org/wasix-libc/blob/main/libc-bottom-half/headers/public/wasix/context.h
#[instrument(level = "trace", skip(ctx))]
pub async fn context_switch(
    mut ctx: AsyncFunctionEnvMut<WasiEnv>,
    target_context_id: u64,
) -> Result<Errno, RuntimeError> {
    let mut write_lock = ctx.write().await;

    let mut sync_env = write_lock.as_function_env_mut();
    match WasiEnv::do_pending_operations(&mut sync_env) {
        Ok(()) => {}
        Err(e) => {
            return Err(RuntimeError::user(e.into()));
        }
    }

    let data = write_lock.data_mut();

    // Verify that we are in an async context
    let environment = match &data.context_switching_environment {
        Some(c) => c,
        None => {
            tracing::warn!(
                "The WASIX context-switching API is only available after entering the main function"
            );
            return Ok(Errno::Notsup);
        }
    };

    // Get own context ID
    let active_context_id = environment.active_context_id();

    // If switching to self, do nothing
    if active_context_id == target_context_id {
        tracing::trace!("Switching context {active_context_id} to itself, which is a no-op");
        return Ok(Errno::Success);
    }

    // Try to unblock the target and get future to wait until we are unblocked again
    //
    // We must be careful not to return after this point without awaiting the resulting future
    let wait_for_unblock = match environment.switch_context(target_context_id) {
        Ok(wait_for_unblock) => wait_for_unblock,
        Err(ContextSwitchError::SwitchTargetMissing) => {
            tracing::trace!(
                "Context {active_context_id} tried to switch to context {target_context_id} but it does not exist or is not suspended"
            );
            return Ok(Errno::Inval);
        }
    };

    // Drop the write lock before we suspend ourself, as that would cause a deadlock
    drop(write_lock);
    tracing::trace!("Suspending context {active_context_id} to switch to {target_context_id}");

    // Wait until we are unblocked again
    let result = wait_for_unblock.map(|v| v.map(|_| Errno::Success)).await;
    tracing::trace!("Resumed context {active_context_id} after being switched back to");
    if let Err(e) = &result {
        tracing::trace!("But it has an error {e:?}");
    }
    result
}

/// This stub is used for context_switch, when the engine does not support async
///
/// It prints a warning and indicates that no context-switching environment is available.
#[instrument(level = "trace", skip(ctx))]
pub fn context_switch_not_supported(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    _target_context_id: u64,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    tracing::warn!(
        "The WASIX context-switching API is only available in engines supporting async execution"
    );
    // Indicate that no context-switching environment is available
    Ok(Errno::Notsup)
}
