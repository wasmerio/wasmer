use super::*;
use crate::syscalls::*;

/// ### `port_addr_remove()`
/// Removes an address from the local port
///
/// ## Parameters
///
/// * `addr` - Address to be removed
pub fn port_addr_remove<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    ip: WasmPtr<__wasi_addr_t, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::port_addr_remove",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let ip = wasi_try!(crate::state::read_ip(&memory, ip));
    wasi_try!(env.net().ip_remove(ip).map_err(net_error_into_wasi_err));
    Errno::Success
}