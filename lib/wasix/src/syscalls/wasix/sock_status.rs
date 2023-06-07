use super::*;
use crate::syscalls::*;

/// ### `sock_status()`
/// Returns the current status of a socket
#[instrument(level = "debug", skip_all, fields(%sock, status = field::Empty), ret)]
pub fn sock_status<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ret_status: WasmPtr<Sockstatus, M>,
) -> Errno {
    let status = wasi_try!(__sock_actor(
        &mut ctx,
        sock,
        Rights::empty(),
        |socket, _| socket.status()
    ));

    use crate::net::socket::WasiSocketStatus;
    let status = match status {
        WasiSocketStatus::Opening => Sockstatus::Opening,
        WasiSocketStatus::Opened => Sockstatus::Opened,
        WasiSocketStatus::Closed => Sockstatus::Closed,
        WasiSocketStatus::Failed => Sockstatus::Failed,
    };
    Span::current().record("status", &format!("{:?}", status));

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    wasi_try_mem!(ret_status.write(&memory, status));
    Errno::Success
}
