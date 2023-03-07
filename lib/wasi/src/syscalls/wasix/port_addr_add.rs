use super::*;
use crate::syscalls::*;

/// ### `port_addr_add()`
/// Adds another static address to the local port
///
/// ## Parameters
///
/// * `addr` - Address to be added
#[instrument(level = "debug", skip_all, fields(ip = field::Empty), ret, err)]
pub fn port_addr_add<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    ip: WasmPtr<__wasi_cidr_t, M>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let cidr = wasi_try_ok!(crate::net::read_cidr(&memory, ip));
    Span::current().record("ip", &format!("{:?}", cidr));

    let net = env.net().clone();
    wasi_try_ok!(__asyncify(&mut ctx, None, async {
        net.ip_add(cidr.ip, cidr.prefix)
            .map_err(net_error_into_wasi_err)
    })?);
    Ok(Errno::Success)
}
