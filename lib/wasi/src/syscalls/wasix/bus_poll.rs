use super::*;
use crate::syscalls::*;

/// Polls for any outstanding events from a particular
/// bus process by its handle
///
/// ## Parameters
///
/// * `timeout` - Timeout before the poll returns, if one passed 0
///   as the timeout then this call is non blocking.
/// * `events` - An events buffer that will hold any received bus events
/// * `malloc` - Name of the function that will be invoked to allocate memory
///   Function signature fn(u64) -> u64
///
/// ## Return
///
/// Returns the number of events that have occured
pub fn bus_poll<M: MemorySize>(
    _ctx: FunctionEnvMut<'_, WasiEnv>,
    _timeout: Timestamp,
    _ref_events: WasmPtr<__wasi_busevent_t, M>,
    _maxevents: M::Offset,
    _ret_nevents: WasmPtr<M::Offset, M>,
) -> Result<BusErrno, WasiError> {
    Ok(BusErrno::Unsupported)
}
