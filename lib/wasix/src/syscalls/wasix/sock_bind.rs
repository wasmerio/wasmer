use super::*;
use crate::syscalls::*;

/// ### `sock_bind()`
/// Bind a socket
/// Note: This is similar to `bind` in POSIX using PF_INET
///
/// ## Parameters
///
/// * `fd` - File descriptor of the socket to be bind
/// * `addr` - Address to bind the socket to
#[instrument(level = "debug", skip_all, fields(%sock, addr = field::Empty), ret)]
pub fn sock_bind<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Errno {
    let env = ctx.data();
    let (memory, mut state, mut inodes) =
        unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };
    let addr = wasi_try!(crate::net::read_socket_addr(&memory, addr));
    Span::current().record("addr", &format!("{:?}", addr));

    let net = env.net().clone();

    let tasks = ctx.data().tasks().clone();
    wasi_try!(__sock_upgrade(
        &mut ctx,
        sock,
        Rights::SOCK_BIND,
        move |socket| async move {
            socket
                .bind(tasks.deref(), net.deref(), &state, &inodes, addr)
                .await
        }
    ));

    Errno::Success
}
