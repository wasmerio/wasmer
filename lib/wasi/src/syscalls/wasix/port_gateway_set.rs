use super::*;
use crate::syscalls::*;

/// ### `port_gateway_set()`
/// Adds a default gateway to the port
///
/// ## Parameters
///
/// * `addr` - Address of the default gateway
pub fn port_gateway_set<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    ip: WasmPtr<__wasi_addr_t, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::port_gateway_set",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let ip = wasi_try!(crate::state::read_ip(&memory, ip));

    wasi_try!(env.net().gateway_set(ip).map_err(net_error_into_wasi_err));
    Errno::Success
}