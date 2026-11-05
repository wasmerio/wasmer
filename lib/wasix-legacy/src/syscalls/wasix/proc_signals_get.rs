use super::*;
use crate::syscalls::*;

/// ### `proc_signals_get()`
/// Gets signals with overridden handlers.
///
/// Inputs:
/// - `__wasi_signal_and_action_t *buf`
///     A pointer to a buffer to write the signal data.
#[instrument(level = "trace", skip_all, ret)]
pub fn proc_signals_get<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    buf: WasmPtr<SignalDisposition, M>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let (memory, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let signals = state.signals.lock().unwrap();

    let buf = wasi_try_mem_ok!(buf.slice(&memory, wasi_try_ok!(to_offset::<M>(signals.len()))));

    for (idx, (sig, act)) in signals.iter().enumerate() {
        wasi_try_mem_ok!(buf.index(idx as u64).write(SignalDisposition {
            sig: *sig,
            disp: *act
        }));
    }

    Ok(Errno::Success)
}
