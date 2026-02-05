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
#[instrument(level = "trace", skip_all, fields(%sock, addr = field::Empty), ret)]
pub fn sock_connect<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let addr = wasi_try_ok!(crate::net::read_ip_port(&memory, addr));
    let peer_addr = SocketAddr::new(addr.0, addr.1);
    Span::current().record("addr", format!("{peer_addr:?}"));

    match sock_connect_internal(&mut ctx, sock, peer_addr)? {
        Ok(()) => {}
        Err(err) => {
            let err = match err {
                Errno::Addrnotavail => Errno::Connrefused,
                _ => err,
            };
            return Ok(err);
        }
    }

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
                WasiError::Exit(ExitCode::from(Errno::Fault))
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
        move |mut socket, flags| async move {
            // Auto-bind UDP. If this upgrades a pre-socket, we must return
            // the new socket so the inode is swapped.
            let upgraded = socket.auto_bind_udp(tasks.deref(), net.deref()).await?;
            let upgraded_socket = upgraded.is_some();
            let mut socket = upgraded.unwrap_or(socket);

            let res = socket
                .connect(
                    tasks.deref(),
                    net.deref(),
                    addr,
                    None,
                    flags.contains(Fdflags::NONBLOCK),
                )
                .await;

            match res {
                Ok(Some(new_socket)) => Ok(Some(new_socket)),
                Ok(None) => {
                    if upgraded_socket {
                        Ok(Some(socket))
                    } else {
                        Ok(None)
                    }
                }
                Err(err) => Err(err),
            }
        }
    ));

    Ok(Ok(()))
}
