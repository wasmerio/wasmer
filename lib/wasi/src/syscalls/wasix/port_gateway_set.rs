use super::*;
use crate::syscalls::*;

/// ### `port_gateway_set()`
/// Adds a default gateway to the port
///
/// ## Parameters
///
/// * `addr` - Address of the default gateway
#[instrument(level = "debug", skip_all, fields(ip = field::Empty), ret, err)]
pub fn port_gateway_set<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    ip: WasmPtr<__wasi_addr_t, M>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let ip = wasi_try_ok!(crate::net::read_ip(&memory, ip));
    Span::current().record("ip", &format!("{:?}", ip));

    let net = env.net().clone();
    wasi_try_ok!(__asyncify(&mut ctx, None, async {
        net.gateway_set(ip).map_err(net_error_into_wasi_err)
    })?);
    Ok(Errno::Success)
}
