use super::*;
use crate::syscalls::*;

/// ### `sock_status()`
/// Returns the current status of a socket
pub fn sock_status<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ret_status: WasmPtr<Sockstatus, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_status (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

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

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    wasi_try_mem!(ret_status.write(&memory, status));
    Errno::Success
}
