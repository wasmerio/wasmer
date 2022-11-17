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
    ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
    reuse: Bool,
    ret_bid: WasmPtr<Bid, M>,
) -> Result<BusErrno, WasiError> {
    let env = ctx.data();
    let bus = env.runtime.bus();
    let memory = env.memory_view(&ctx);
    let name = unsafe { get_input_str_bus_ok!(&memory, name, name_len) };
    let reuse = reuse == Bool::True;
    debug!(
        "wasi[{}:{}]::bus_open_local (name={}, reuse={})",
        ctx.data().pid(),
        ctx.data().tid(),
        name,
        reuse
    );

    bus_open_internal(ctx, name, reuse, None, None, ret_bid)
}
