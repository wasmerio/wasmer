use std::task::Waker;

use super::*;
use crate::{net::socket::TimeType, syscalls::*};

/// ### `sock_accept()`
/// Accept a new incoming connection.
/// Note: This is similar to `accept` in POSIX.
///
/// ## Parameters
///
/// * `fd` - The listening socket.
/// * `flags` - The desired values of the file descriptor flags.
///
/// ## Return
///
/// New socket connection
#[instrument(level = "trace", skip_all, fields(%sock, fd = field::Empty), ret)]
pub fn sock_accept<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    fd_flags: Fdflags,
    ro_fd: WasmPtr<WasiFd, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    ctx = wasi_try_ok!(maybe_snapshot::<M>(ctx)?);

    let env = ctx.data();
    let (memory, state, _) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    let nonblocking = fd_flags.contains(Fdflags::NONBLOCK);

    let (fd, _, _) = wasi_try_ok!(sock_accept_internal(
        env,
        sock,
        fd_flags,
        nonblocking,
        None
    )?);

    wasi_try_mem_ok!(ro_fd.write(&memory, fd));

    Ok(Errno::Success)
}

/// ### `sock_accept_v2()`
/// Accept a new incoming connection.
/// Note: This is similar to `accept` in POSIX.
///
/// ## Parameters
///
/// * `fd` - The listening socket.
/// * `flags` - The desired values of the file descriptor flags.
/// * `ro_addr` - Returns the address and port of the client
///
/// ## Return
///
/// New socket connection
#[instrument(level = "trace", skip_all, fields(%sock, fd = field::Empty), ret)]
pub fn sock_accept_v2<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    fd_flags: Fdflags,
    ro_fd: WasmPtr<WasiFd, M>,
    ro_addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    let env = ctx.data();
    let (memory, state, _) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    let nonblocking = fd_flags.contains(Fdflags::NONBLOCK);

    let (fd, local_addr, peer_addr) = wasi_try_ok!(sock_accept_internal(
        env,
        sock,
        fd_flags,
        nonblocking,
        None
    )?);

    #[cfg(feature = "journal")]
    if ctx.data().enable_journal {
        JournalEffector::save_sock_accepted(
            &mut ctx,
            sock,
            fd,
            local_addr,
            peer_addr,
            fd_flags,
            nonblocking,
        )
        .map_err(|err| {
            tracing::error!("failed to save sock_accepted event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    let env = ctx.data();
    let (memory, state, _) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };
    wasi_try_mem_ok!(ro_fd.write(&memory, fd));
    wasi_try_ok!(crate::net::write_ip_port(
        &memory,
        ro_addr,
        peer_addr.ip(),
        peer_addr.port()
    ));

    Ok(Errno::Success)
}

pub(crate) fn sock_accept_internal(
    env: &WasiEnv,
    sock: WasiFd,
    mut fd_flags: Fdflags,
    mut nonblocking: bool,
    with_fd: Option<WasiFd>,
) -> Result<Result<(WasiFd, SocketAddr, SocketAddr), Errno>, WasiError> {
    let state = env.state();
    let inodes = &state.inodes;

    let tasks = env.tasks().clone();
    let (child, local_addr, peer_addr, fd_flags) = wasi_try_ok_ok!(__sock_asyncify(
        env,
        sock,
        Rights::SOCK_ACCEPT,
        move |socket, fd| async move {
            if fd.inner.flags.contains(Fdflags::NONBLOCK) {
                fd_flags.set(Fdflags::NONBLOCK, true);
                nonblocking = true;
            }
            let timeout = socket
                .opt_time(TimeType::AcceptTimeout)
                .ok()
                .flatten()
                .unwrap_or(Duration::from_secs(30));
            let local_addr = socket.addr_local()?;
            socket
                .accept(tasks.deref(), nonblocking, Some(timeout))
                .await
                .map(|a| (a.0, local_addr, a.1, fd_flags))
        },
    ));

    let kind = Kind::Socket {
        socket: InodeSocket::new(InodeSocketKind::TcpStream {
            socket: child,
            write_timeout: None,
            read_timeout: None,
        }),
    };
    let inode = state
        .fs
        .create_inode_with_default_stat(inodes, kind, false, "socket".into());

    let mut new_flags = Fdflags::empty();
    if fd_flags.contains(Fdflags::NONBLOCK) {
        new_flags.set(Fdflags::NONBLOCK, true);
    }

    let mut new_flags = Fdflags::empty();
    if fd_flags.contains(Fdflags::NONBLOCK) {
        new_flags.set(Fdflags::NONBLOCK, true);
    }

    let rights = Rights::all_socket();
    let fd = wasi_try_ok_ok!(if let Some(fd) = with_fd {
        state
            .fs
            .with_fd(rights, rights, new_flags, Fdflagsext::empty(), 0, inode, fd)
            .map(|_| fd)
    } else {
        state
            .fs
            .create_fd(rights, rights, new_flags, Fdflagsext::empty(), 0, inode)
    });
    Span::current().record("fd", fd);

    Ok(Ok((fd, local_addr, peer_addr)))
}
