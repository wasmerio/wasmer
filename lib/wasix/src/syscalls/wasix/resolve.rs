use super::*;
use crate::syscalls::*;

/// ### `resolve()`
/// Resolves a hostname and a port to one or more IP addresses.
///
/// Note: This is similar to `getaddrinfo` in POSIX
///
/// When successful, the contents of the output buffer consist of a sequence of
/// IPv4 and/or IPv6 addresses. Each address entry consists of a addr_t object.
/// This function fills the output buffer as much as possible.
///
/// ## Parameters
///
/// * `host` - Host to resolve
/// * `port` - Port hint (zero if no hint is supplied)
/// * `addrs` - The buffer where addresses will be stored
///
/// ## Return
///
/// The number of IP addresses returned during the DNS resolution.
#[instrument(level = "trace", skip_all, fields(host = field::Empty, %port), ret)]
pub fn resolve<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    host: WasmPtr<u8, M>,
    host_len: M::Offset,
    port: u16,
    addrs: WasmPtr<__wasi_addr_t, M>,
    naddrs: M::Offset,
    ret_naddrs: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    let naddrs: usize = wasi_try_ok!(naddrs.try_into().map_err(|_| Errno::Inval));
    let mut env = ctx.data();
    let host_str = {
        let memory = unsafe { env.memory_view(&ctx) };
        unsafe { get_input_str_ok!(&memory, host, host_len) }
    };
    Span::current().record("host", host_str.as_str());

    let port = if port > 0 { Some(port) } else { None };

    let net = env.net().clone();
    let tasks = env.tasks().clone();
    let found_ips = wasi_try_ok!(__asyncify(&mut ctx, None, async move {
        net.resolve(host_str.as_str(), port, None)
            .await
            .map_err(net_error_into_wasi_err)
    })?);
    env = ctx.data();

    let mut idx = 0;
    let memory = unsafe { env.memory_view(&ctx) };
    let addrs = wasi_try_mem_ok!(addrs.slice(&memory, wasi_try_ok!(to_offset::<M>(naddrs))));
    for found_ip in found_ips.iter().take(naddrs) {
        crate::net::write_ip(&memory, addrs.index(idx).as_ptr::<M>(), *found_ip);
        idx += 1;
    }

    let idx: M::Offset = wasi_try_ok!(idx.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(ret_naddrs.write(&memory, idx));

    Ok(Errno::Success)
}
