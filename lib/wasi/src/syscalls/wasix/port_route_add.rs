use super::*;
use crate::syscalls::*;

/// ### `port_route_add()`
/// Adds a new route to the local port
pub fn port_route_add<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    cidr: WasmPtr<__wasi_cidr_t, M>,
    via_router: WasmPtr<__wasi_addr_t, M>,
    preferred_until: WasmPtr<OptionTimestamp, M>,
    expires_at: WasmPtr<OptionTimestamp, M>,
) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::port_route_add",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let cidr = wasi_try_ok!(crate::net::read_cidr(&memory, cidr));
    let via_router = wasi_try_ok!(crate::net::read_ip(&memory, via_router));
    let preferred_until = wasi_try_mem_ok!(preferred_until.read(&memory));
    let preferred_until = match preferred_until.tag {
        OptionTag::None => None,
        OptionTag::Some => Some(Duration::from_nanos(preferred_until.u)),
        _ => return Ok(Errno::Inval),
    };
    let expires_at = wasi_try_mem_ok!(expires_at.read(&memory));
    let expires_at = match expires_at.tag {
        OptionTag::None => None,
        OptionTag::Some => Some(Duration::from_nanos(expires_at.u)),
        _ => return Ok(Errno::Inval),
    };

    let net = env.net().clone();
    wasi_try_ok!(__asyncify(&mut ctx, None, async {
        net.route_add(cidr, via_router, preferred_until, expires_at)
            .map_err(net_error_into_wasi_err)
    })?);
    Ok(Errno::Success)
}
