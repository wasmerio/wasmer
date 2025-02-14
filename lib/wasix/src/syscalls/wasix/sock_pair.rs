use virtual_fs::Pipe;

use super::*;
use crate::{
    net::socket::{self, SocketProperties},
    syscalls::*,
};

// FIXME
/// ### `sock_pair()`
/// Create an interconnected socket pair; or at least it's supposed to.
///
/// Currently, this creates a pipe rather than a pair of sockets. Before this
/// syscall was added, wasix-libc would just do pipe2 in its socketpair
/// implementation. Since we fixed pipe2 to return a simplex pipe, that was no
/// longer an option; hence this syscall was added, but the implementation
/// still uses a pipe as the underlying communication mechanism. This is not
/// the correct implementation and needs to be fixed. We hope that the pipe
/// is sufficient for anything that doesn't do socket-specific stuff, such
/// as sending out-of-band packets.
///
/// Note: This is (supposed to be) similar to `socketpair` in POSIX using PF_INET
///
/// ## Parameters
///
/// * `af` - Address family
/// * `socktype` - Socket type, either datagram or stream
/// * `sock_proto` - Socket protocol
///
/// ## Return
///
/// The file descriptor of the socket that has been opened.
#[instrument(level = "trace", skip_all, fields(?af, ?ty, ?pt, sock1 = field::Empty, sock2 = field::Empty), ret)]
pub fn sock_pair<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    af: Addressfamily,
    ty: Socktype,
    pt: SockProto,
    ro_sock1: WasmPtr<WasiFd, M>,
    ro_sock2: WasmPtr<WasiFd, M>,
) -> Result<Errno, WasiError> {
    // only certain combinations are supported
    match pt {
        SockProto::Tcp => {
            if ty != Socktype::Stream {
                return Ok(Errno::Notsup);
            }
        }
        SockProto::Udp => {
            if ty != Socktype::Dgram {
                return Ok(Errno::Notsup);
            }
        }
        _ => {}
    }

    // FIXME: currently, socket properties are ignored outright, since they
    // make no sense for the underlying pipe
    let (fd1, fd2) = wasi_try_ok!(sock_pair_internal(&mut ctx, None, None));

    #[cfg(feature = "journal")]
    if ctx.data().enable_journal {
        JournalEffector::save_sock_pair(&mut ctx, fd1, fd2).map_err(|err| {
            tracing::error!("failed to save sock_pair event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    let env = ctx.data();
    let (memory, state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };
    wasi_try_mem_ok!(ro_sock1.write(&memory, fd1));
    wasi_try_mem_ok!(ro_sock2.write(&memory, fd2));

    Ok(Errno::Success)
}

pub(crate) fn sock_pair_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    with_fd1: Option<WasiFd>,
    with_fd2: Option<WasiFd>,
) -> Result<(WasiFd, WasiFd), Errno> {
    let env = ctx.data();
    let (memory, state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };
    let (end1, end2) = Pipe::channel();

    let inode1 = state.fs.create_inode_with_default_stat(
        inodes,
        Kind::DuplexPipe { pipe: end1 },
        false,
        "socketpair".into(),
    );
    let inode2 = state.fs.create_inode_with_default_stat(
        inodes,
        Kind::DuplexPipe { pipe: end2 },
        false,
        "socketpair".into(),
    );

    let rights = Rights::all_socket();
    let fd1 = if let Some(fd) = with_fd1 {
        state
            .fs
            .with_fd(
                rights,
                rights,
                Fdflags::empty(),
                Fdflagsext::empty(),
                0,
                inode1,
                fd,
            )
            .map(|_| fd)?
    } else {
        state.fs.create_fd(
            rights,
            rights,
            Fdflags::empty(),
            Fdflagsext::empty(),
            0,
            inode1,
        )?
    };
    let fd2 = if let Some(fd) = with_fd2 {
        state
            .fs
            .with_fd(
                rights,
                rights,
                Fdflags::empty(),
                Fdflagsext::empty(),
                0,
                inode2,
                fd,
            )
            .map(|_| fd)?
    } else {
        state.fs.create_fd(
            rights,
            rights,
            Fdflags::empty(),
            Fdflagsext::empty(),
            0,
            inode2,
        )?
    };
    Span::current().record("end1", fd1);
    Span::current().record("end2", fd2);

    Ok((fd1, fd2))
}
