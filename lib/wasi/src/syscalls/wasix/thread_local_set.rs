use super::*;
use crate::syscalls::*;

/// ### `thread_local_set()`
/// Sets the value of a thread local variable
///
/// ## Parameters
///
/// * `key` - Thread key that this local variable will be associated with
/// * `val` - Value to be set for the thread local variable
#[instrument(level = "trace", skip_all, fields(%key, %val), ret)]
pub fn thread_local_set(ctx: FunctionEnvMut<'_, WasiEnv>, key: TlKey, val: TlVal) -> Errno {
    let env = ctx.data();

    let current_thread = ctx.data().thread.tid();
    let mut inner = env.process.write();
    inner.thread_local.insert((current_thread, key), val);
    Errno::Success
}
