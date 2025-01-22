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
#[instrument(level = "trace", skip_all, fields(%sock, addr = field::Empty), ret)]
pub fn sock_bind<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    let addr = wasi_try_ok!(crate::net::read_ip_port(&memory, addr));
    let addr = SocketAddr::new(addr.0, addr.1);
    Span::current().record("addr", format!("{addr:?}"));

    wasi_try_ok!(sock_bind_internal(&mut ctx, sock, addr)?);

    #[cfg(feature = "journal")]
    if ctx.data().enable_journal {
        JournalEffector::save_sock_bind(&mut ctx, sock, addr).map_err(|err| {
            tracing::error!("failed to save sock_bind event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub(crate) fn sock_bind_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    addr: SocketAddr,
) -> Result<Result<(), Errno>, WasiError> {
    let env = ctx.data();
    let net = env.net().clone();

    let tasks = ctx.data().tasks().clone();
    wasi_try_ok_ok!(__sock_upgrade(
        ctx,
        sock,
        Rights::SOCK_BIND,
        move |socket, _| async move { socket.bind(tasks.deref(), net.deref(), addr).await }
    ));

    Ok(Ok(()))
}
