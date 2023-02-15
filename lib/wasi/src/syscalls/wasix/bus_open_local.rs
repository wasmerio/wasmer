use super::*;
use crate::syscalls::*;

/// Spawns a new bus process for a particular web WebAssembly
/// binary that is referenced by its process name.
///
/// ## Parameters
///
/// * `name` - Name of the process to be spawned
/// * `reuse` - Indicates if the existing processes should be reused
///   if they are already running
///
/// ## Return
///
/// Returns a bus process id that can be used to invoke calls
pub fn bus_open_local<M: MemorySize>(
    _ctx: FunctionEnvMut<'_, WasiEnv>,
    _name: WasmPtr<u8, M>,
    _name_len: M::Offset,
    _reuse: Bool,
    _ret_bid: WasmPtr<Bid, M>,
) -> Result<BusErrno, WasiError> {
    Ok(BusErrno::Unsupported)
}
