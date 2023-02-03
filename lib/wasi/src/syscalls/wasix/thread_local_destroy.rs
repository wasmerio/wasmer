use super::*;
use crate::syscalls::*;

/// ### `thread_local_destroy()`
/// Destroys a thread local variable
///
/// ## Parameters
///
/// * `user_data` - User data that will be passed to the destructor
///   when the thread variable goes out of scope
/// * `key` - Thread key that was previously created
pub fn thread_local_destroy(mut ctx: FunctionEnvMut<'_, WasiEnv>, key: TlKey) -> Errno {
    trace!(
        "wasi[{}:{}]::thread_local_destroy (key={})",
        ctx.data().pid(),
        ctx.data().tid(),
        key
    );
    let process = ctx.data().process.clone();
    let mut inner = process.write();

    let data = inner
        .thread_local
        .iter()
        .filter(|((_, k), _)| *k == key)
        .map(|(_, v)| *v)
        .collect::<Vec<_>>();
    inner.thread_local.retain(|(_, k), _| *k != key);

    if let Some(user_data) = inner.thread_local_user_data.remove(&key) {
        drop(inner);

        if let Some(thread_local_destroy) =
            ctx.data().inner().thread_local_destroy.as_ref().cloned()
        {
            for val in data {
                let user_data_low: u32 = (user_data & 0xFFFFFFFF) as u32;
                let user_data_high: u32 = (user_data >> 32) as u32;

                let val_low: u32 = (val & 0xFFFFFFFF) as u32;
                let val_high: u32 = (val >> 32) as u32;

                let _ = thread_local_destroy.call(
                    &mut ctx,
                    user_data_low as i32,
                    user_data_high as i32,
                    val_low as i32,
                    val_high as i32,
                );
            }
        }
    }
    Errno::Success
}
