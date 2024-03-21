use super::*;
use crate::syscalls::*;

/// ### `sock_connect()`
/// Initiate a connection on a socket to the specified address
///
/// Polling the socket handle will wait for data to arrive or for
/// the socket status to change which can be queried via 'sock_status'
///
/// Note: This is similar to `connect` in POSIX
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `addr` - Address of the socket to connect to
#[instrument(level = "debug", skip_all, fields(%sock, addr = field::Empty), ret)]
pub fn sock_connect<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let addr = wasi_try_ok!(crate::net::read_ip_port(&memory, addr));
    let peer_addr = SocketAddr::new(addr.0, addr.1);
    Span::current().record("addr", &format!("{:?}", peer_addr));

    wasi_try_ok!(sock_connect_internal(&mut ctx, sock, peer_addr)?);

    #[cfg(feature = "journal")]
    if ctx.data().enable_journal {
        let local_addr = wasi_try_ok!(__sock_actor(
            &mut ctx,
            sock,
            Rights::empty(),
            |socket, _| socket.addr_local()
        ));
        JournalEffector::save_sock_connect(&mut ctx, sock, local_addr, peer_addr).map_err(
            |err| {
                tracing::error!("failed to save sock_connected event - {}", err);
                WasiError::Exit(ExitCode::Errno(Errno::Fault))
            },
        )?;
    }

    Ok(Errno::Success)
}

pub(crate) fn sock_connect_internal(
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
        Rights::SOCK_CONNECT,
        move |mut socket| async move { socket.connect(tasks.deref(), net.deref(), addr, None).await }
    ));

    Ok(Ok(()))
}
