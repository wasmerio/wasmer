use super::*;
use crate::syscalls::*;

/// ### `port_ip_list()`
/// Returns a list of all the addresses owned by the local port
/// This function fills the output buffer as much as possible.
/// If the buffer is not big enough then the naddrs address will be
/// filled with the buffer size needed and the EOVERFLOW will be returned
///
/// ## Parameters
///
/// * `addrs` - The buffer where addresses will be stored
///
/// ## Return
///
/// The number of addresses returned.
#[instrument(level = "debug", skip_all, fields(naddrs = field::Empty), ret)]
pub fn port_addr_list<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    addrs_ptr: WasmPtr<__wasi_cidr_t, M>,
    naddrs_ptr: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    let mut env = ctx.data();
    let mut memory = unsafe { env.memory_view(&ctx) };
    let max_addrs = wasi_try_mem_ok!(naddrs_ptr.read(&memory));
    let max_addrs: u64 = wasi_try_ok!(max_addrs.try_into().map_err(|_| Errno::Overflow));

    let net = env.net().clone();
    let addrs = wasi_try_ok!(__asyncify(&mut ctx, None, async {
        net.ip_list().await.map_err(net_error_into_wasi_err)
    })?);
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    Span::current().record("naddrs", addrs.len());

    let addrs_len: M::Offset = wasi_try_ok!(addrs.len().try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(naddrs_ptr.write(&memory, addrs_len));
    if addrs.len() as u64 > max_addrs {
        return Ok(Errno::Overflow);
    }

    let ref_addrs = wasi_try_mem_ok!(
        addrs_ptr.slice(&memory, wasi_try_ok!(to_offset::<M>(max_addrs as usize)))
    );
    for n in 0..addrs.len() {
        let nip = ref_addrs.index(n as u64);
        crate::net::write_cidr(&memory, nip.as_ptr::<M>(), *addrs.get(n).unwrap());
    }

    Ok(Errno::Success)
}
