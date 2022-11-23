use super::*;
use crate::syscalls::*;

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
pub fn sock_accept<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    fd_flags: Fdflags,
    ro_fd: WasmPtr<WasiFd, M>,
    ro_addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::sock_accept (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    wasi_try_ok!(ctx.data().clone().process_signals_and_exit(&mut ctx)?);

    let (child, addr) = wasi_try_ok!(__sock_actor(
        &mut ctx,
        sock,
        Rights::SOCK_ACCEPT,
        move |socket| async move { socket.accept(fd_flags).await }
    ));

    let env = ctx.data();
    let (memory, state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let kind = Kind::Socket {
        socket: InodeSocket::new(InodeSocketKind::TcpStream(child)),
    };
    let inode =
        state
            .fs
            .create_inode_with_default_stat(inodes.deref_mut(), kind, false, "socket".into());

    let rights = Rights::all_socket();
    let fd = wasi_try_ok!(state
        .fs
        .create_fd(rights, rights, Fdflags::empty(), 0, inode));

    debug!(
        "wasi[{}:{}]::sock_accept (ret=ESUCCESS, peer={})",
        ctx.data().pid(),
        ctx.data().tid(),
        fd
    );

    wasi_try_mem_ok!(ro_fd.write(&memory, fd));
    wasi_try_ok!(crate::net::write_ip_port(
        &memory,
        ro_addr,
        addr.ip(),
        addr.port()
    ));

    Ok(Errno::Success)
}
