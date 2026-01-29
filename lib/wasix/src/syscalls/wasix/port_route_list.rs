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
#[instrument(level = "trace", skip_all, fields(nroutes = field::Empty, max_routes = field::Empty), ret)]
pub fn port_route_list<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    routes_ptr: WasmPtr<Route, M>,
    nroutes_ptr: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let mut env = ctx.data();
    let mut memory = unsafe { env.memory_view(&ctx) };
    let ref_nroutes = nroutes_ptr.deref(&memory);
    let max_routes: usize = wasi_try_ok!(
        wasi_try_mem_ok!(ref_nroutes.read())
            .try_into()
            .map_err(|_| Errno::Inval)
    );
    Span::current().record("max_routes", max_routes);
    if max_routes > 0 {
        let routes_bytes = wasi_try_ok!(max_routes
            .checked_mul(176)
            .ok_or(Errno::Overflow));
        let start: u64 = routes_ptr.offset().into();
        let end = wasi_try_ok!(start
            .checked_add(routes_bytes as u64)
            .ok_or(Errno::Overflow));
        if end > memory.data_size() {
            return Ok(Errno::Memviolation);
        }
    }

    let net = env.net().clone();
    let routes = wasi_try_ok!(__asyncify(&mut ctx, None, async {
        net.route_list().await.map_err(net_error_into_wasi_err)
    })?);
    Span::current().record("nroutes", routes.len());

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    let routes_len: M::Offset = wasi_try_ok!(routes.len().try_into().map_err(|_| Errno::Inval));
    let nroutes = nroutes_ptr.deref(&memory);
    wasi_try_mem_ok!(nroutes.write(routes_len));
    if routes.len() > max_routes {
        return Ok(Errno::Overflow);
    }

    for n in 0..routes.len() {
        let base: u64 = routes_ptr.offset().into();
        let elem_offset = wasi_try_ok!(base
            .checked_add((n as u64) * 176)
            .ok_or(Errno::Overflow));
        let ptr = WasmPtr::<Route, M>::new(
            wasi_try_ok!(M::Offset::try_from(elem_offset).map_err(|_| Errno::Overflow)),
        );
        wasi_try_ok!(crate::net::write_route(
            &memory,
            ptr,
            routes.get(n).unwrap().clone(),
        ));
    }

    Ok(Errno::Success)
}
