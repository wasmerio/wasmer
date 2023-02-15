use super::*;
use crate::syscalls::*;

/// ### `port_mac()`
/// Returns the MAC address of the local port
pub fn port_mac<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    ret_mac: WasmPtr<__wasi_hardwareaddress_t, M>,
) -> Result<Errno, WasiError> {
    debug!("wasi[{}:{}]::port_mac", ctx.data().pid(), ctx.data().tid());
    let mut env = ctx.data();
    let mut memory = env.memory_view(&ctx);

    let net = env.net().clone();
    let mac = wasi_try_ok!(__asyncify(&mut ctx, None, async {
        net.mac().map_err(net_error_into_wasi_err)
    })?);
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let mac = __wasi_hardwareaddress_t { octs: mac };
    wasi_try_mem_ok!(ret_mac.write(&memory, mac));
    Ok(Errno::Success)
}
