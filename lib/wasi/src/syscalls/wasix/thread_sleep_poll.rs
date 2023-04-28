use std::task::Waker;

use wasmer_wasix_types::wasi::WakerId;

use super::*;
use crate::{state::conv_waker_id, syscalls::*};

/// ### `thread_sleep_poll()`
///
/// Polls the current thread to sleep for a period of time
/// The registered waker will be woken when the sleep period is reached
///
/// ## Parameters
///
/// * `duration` - Amount of time that the thread should sleep
/// * `waker` - Waker ID that will be passed back to the program when the waker is triggered
///
#[instrument(level = "debug", skip_all, fields(%duration), ret, err)]
pub fn thread_sleep_poll<M: MemorySize + 'static>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    duration: Timestamp,
    waker: WakerId,
) -> Result<Errno, WasiError> {
    let waker = conv_waker_id(ctx.data().state(), waker);
    thread_sleep_internal::<M>(ctx, duration, Some(&waker))
}
