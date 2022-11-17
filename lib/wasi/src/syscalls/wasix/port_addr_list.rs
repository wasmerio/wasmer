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
pub fn port_addr_list<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    addrs: WasmPtr<__wasi_cidr_t, M>,
    naddrs: WasmPtr<M::Offset, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::port_addr_list",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let max_addrs = wasi_try_mem!(naddrs.read(&memory));
    let max_addrs: u64 = wasi_try!(max_addrs.try_into().map_err(|_| Errno::Overflow));
    let ref_addrs =
        wasi_try_mem!(addrs.slice(&memory, wasi_try!(to_offset::<M>(max_addrs as usize))));

    let addrs = wasi_try!(env.net().ip_list().map_err(net_error_into_wasi_err));

    let addrs_len: M::Offset = wasi_try!(addrs.len().try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem!(naddrs.write(&memory, addrs_len));
    if addrs.len() as u64 > max_addrs {
        return Errno::Overflow;
    }

    for n in 0..addrs.len() {
        let nip = ref_addrs.index(n as u64);
        crate::net::write_cidr(&memory, nip.as_ptr::<M>(), *addrs.get(n).unwrap());
    }

    Errno::Success
}
