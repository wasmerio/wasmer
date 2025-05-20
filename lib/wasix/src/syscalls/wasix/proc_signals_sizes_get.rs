use super::*;
use crate::syscalls::*;

/// ### `proc_signals_count_get()`
/// Gets the number of signals with overridden handlers.
///
/// Outputs:
/// - `size_t *signal_count`
///     The number of signals.
#[instrument(level = "trace", skip_all, fields(signal_count = field::Empty), ret)]
pub fn proc_signals_sizes_get<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    signal_count: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let (memory, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let signal_count = signal_count.deref(&memory);

    let count_val: M::Offset = wasi_try_ok!(state
        .signals
        .lock()
        .unwrap()
        .len()
        .try_into()
        .map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(signal_count.write(count_val));

    Span::current().record("signal_count", u64::try_from(count_val).unwrap());

    Ok(Errno::Success)
}
