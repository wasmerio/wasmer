use super::*;
use crate::syscalls::*;

/// ### `port_route_list()`
/// Returns a list of all the routes owned by the local port
/// This function fills the output buffer as much as possible.
/// If the buffer is too small this will return EOVERFLOW and
/// fill nroutes with the size of the buffer needed.
///
/// ## Parameters
///
/// * `routes` - The buffer where routes will be stored
pub fn port_route_list<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    routes: WasmPtr<Route, M>,
    nroutes: WasmPtr<M::Offset, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::port_route_list",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let nroutes = nroutes.deref(&memory);
    let max_routes: usize = wasi_try!(wasi_try_mem!(nroutes.read())
        .try_into()
        .map_err(|_| Errno::Inval));
    let ref_routes = wasi_try_mem!(routes.slice(&memory, wasi_try!(to_offset::<M>(max_routes))));

    let routes = wasi_try!(env.net().route_list().map_err(net_error_into_wasi_err));

    let routes_len: M::Offset = wasi_try!(routes.len().try_into().map_err(|_| Errno::Inval));
    wasi_try_mem!(nroutes.write(routes_len));
    if routes.len() > max_routes {
        return Errno::Overflow;
    }

    for n in 0..routes.len() {
        let nroute = ref_routes.index(n as u64);
        crate::state::write_route(
            &memory,
            nroute.as_ptr::<M>(),
            routes.get(n).unwrap().clone(),
        );
    }

    Errno::Success
}
