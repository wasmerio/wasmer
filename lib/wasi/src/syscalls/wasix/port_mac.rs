use super::*;
use crate::syscalls::*;

/// ### `port_mac()`
/// Returns the MAC address of the local port
pub fn port_mac<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    ret_mac: WasmPtr<__wasi_hardwareaddress_t, M>,
) -> Errno {
    debug!("wasi[{}:{}]::port_mac", ctx.data().pid(), ctx.data().tid());
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let mac = wasi_try!(env.net().mac().map_err(net_error_into_wasi_err));
    let mac = __wasi_hardwareaddress_t { octs: mac };
    wasi_try_mem!(ret_mac.write(&memory, mac));
    Errno::Success
}