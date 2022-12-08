use super::*;
use crate::syscalls::*;

/// ### `http_status()`
/// Retrieves the status of a HTTP request
///
/// ## Parameters
///
/// * `fd` - Handle of the HTTP request
/// * `status` - Pointer to a buffer that will be filled with the current
///   status of this HTTP request
pub fn http_status<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ref_status: WasmPtr<HttpStatus, M>,
) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::http_status",
        ctx.data().pid(),
        ctx.data().tid()
    );

    let mut env = ctx.data();

    let http_status = wasi_try_ok!(__sock_actor(
        &mut ctx,
        sock,
        Rights::empty(),
        move |socket| async move { socket.http_status() }
    )?);
    env = ctx.data();

    // Write everything else and return the status to the caller
    let status = HttpStatus {
        ok: Bool::True,
        redirect: match http_status.redirected {
            true => Bool::True,
            false => Bool::False,
        },
        size: wasi_try_ok!(Ok(http_status.size)),
        status: http_status.status,
    };

    let memory = env.memory_view(&ctx);
    let ref_status = ref_status.deref(&memory);
    wasi_try_mem_ok!(ref_status.write(status));

    Ok(Errno::Success)
}
