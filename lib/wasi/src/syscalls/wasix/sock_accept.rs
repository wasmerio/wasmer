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
    mut fd_flags: Fdflags,
    ro_fd: WasmPtr<WasiFd, M>,
    ro_addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::sock_accept (fd={}, flags={:?})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock,
        fd_flags
    );

    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    let tasks = ctx.data().tasks().clone();
    let (child, addr, fd_flags) = wasi_try_ok!(__sock_asyncify(
        ctx.data(),
        sock,
        Rights::SOCK_ACCEPT,
        move |socket, fd| async move {
            if fd.flags.contains(Fdflags::NONBLOCK) {
                fd_flags.set(Fdflags::NONBLOCK, true);
            }
            socket
                .accept(tasks.deref(), fd_flags)
                .await
                .map(|a| (a.0, a.1, fd_flags))
        }
    ));

    let env = ctx.data();
    let (memory, state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);

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
    let fd = wasi_try_ok!(state.fs.create_fd(rights, rights, new_flags, 0, inode));

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
