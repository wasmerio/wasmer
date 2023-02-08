use super::*;
use crate::syscalls::*;

/// ### `port_addr_remove()`
/// Removes an address from the local port
///
/// ## Parameters
///
/// * `addr` - Address to be removed
pub fn port_addr_remove<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    ip: WasmPtr<__wasi_addr_t, M>,
) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::port_addr_remove",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let ip = wasi_try_ok!(crate::net::read_ip(&memory, ip));
    let net = env.net().clone();
    wasi_try_ok!(__asyncify(&mut ctx, None, async {
        net.ip_remove(ip).map_err(net_error_into_wasi_err)
    })?);
    Ok(Errno::Success)
}
