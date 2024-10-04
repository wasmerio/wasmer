use super::*;
use crate::syscalls::*;

/// ### `port_mac()`
/// Returns the MAC address of the local port
#[instrument(level = "trace", skip_all, fields(max = field::Empty), ret)]
pub fn port_mac<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    ret_mac: WasmPtr<__wasi_hardwareaddress_t, M>,
) -> Result<Errno, WasiError> {
    let mut env = ctx.data();
    let mut memory = unsafe { env.memory_view(&ctx) };

    let net = env.net().clone();
    let mac = wasi_try_ok!(__asyncify(&mut ctx, None, async {
        net.mac().await.map_err(net_error_into_wasi_err)
    })?);
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    Span::current().record("mac", hex::encode(mac.as_ref()).as_str());

    let mac = __wasi_hardwareaddress_t { octs: mac };
    wasi_try_mem_ok!(ret_mac.write(&memory, mac));
    Ok(Errno::Success)
}
