use std::sync::atomic::AtomicUsize;

use virtual_fs::Pipe;

use super::*;
use crate::syscalls::*;

// Used to make pipe end names unique. This is necessary since we use
// a hash of the name to calculate inode numbers. The actual number
// has no other meaning.
static PIPE_NUMBER: AtomicUsize = AtomicUsize::new(0);

/// ### `fd_pipe()`
/// Creates ta pipe that feeds data between two file handles
/// Output:
/// - `Fd`
///     First file handle that represents the read end of the pipe
/// - `Fd`
///     Second file handle that represents the write end of the pipe
#[instrument(level = "trace", skip_all, fields(read_fd = field::Empty, write_fd = field::Empty), ret)]
pub fn fd_pipe<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    ro_read_fd: WasmPtr<WasiFd, M>,
    ro_write_fd: WasmPtr<WasiFd, M>,
) -> Result<Errno, WasiError> {
    let (read_fd, write_fd) = wasi_try_ok!(fd_pipe_internal(&mut ctx, None, None));
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_fd_pipe(&mut ctx, read_fd, write_fd).map_err(|err| {
            tracing::error!("failed to save create pipe event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    let env = ctx.data();
    let (memory, state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    Span::current()
        .record("read_fd", read_fd)
        .record("write_fd", write_fd);

    wasi_try_mem_ok!(ro_read_fd.write(&memory, read_fd));
    wasi_try_mem_ok!(ro_write_fd.write(&memory, write_fd));

    Ok(Errno::Success)
}

pub fn fd_pipe_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    with_read_fd: Option<WasiFd>,
    with_write_fd: Option<WasiFd>,
) -> Result<(WasiFd, WasiFd), Errno> {
    let env = ctx.data();
    let (memory, state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };
    let (tx, rx) = Pipe::new().split();

    // FIXME: since a hash of the inode name is used to calculate the inode number, this may
    // or may not break journals that include pipes and are compacted.
    let pipe_no = PIPE_NUMBER.fetch_add(1, Ordering::SeqCst);

    let rx_inode = state.fs.create_inode_with_default_stat(
        inodes,
        Kind::PipeRx { rx },
        false,
        format!("pipe{pipe_no}-rx").into(),
    );
    let tx_inode = state.fs.create_inode_with_default_stat(
        inodes,
        Kind::PipeTx { tx },
        false,
        format!("pipe{pipe_no}-tx").into(),
    );

    let rights = Rights::FD_SYNC
        | Rights::FD_DATASYNC
        | Rights::POLL_FD_READWRITE
        | Rights::SOCK_SEND
        | Rights::FD_FDSTAT_SET_FLAGS
        | Rights::FD_FILESTAT_GET;

    let read_rights = rights | Rights::FD_READ;
    let write_rights = rights | Rights::FD_WRITE;

    let read_fd = if let Some(fd) = with_read_fd {
        state
            .fs
            .with_fd(
                read_rights,
                read_rights,
                Fdflags::empty(),
                Fdflagsext::empty(),
                0,
                rx_inode,
                fd,
            )
            .map(|()| fd)?
    } else {
        state.fs.create_fd(
            read_rights,
            read_rights,
            Fdflags::empty(),
            Fdflagsext::empty(),
            0,
            rx_inode,
        )?
    };

    let write_fd = if let Some(fd) = with_write_fd {
        state
            .fs
            .with_fd(
                write_rights,
                write_rights,
                Fdflags::empty(),
                Fdflagsext::empty(),
                0,
                tx_inode,
                fd,
            )
            .map(|()| fd)?
    } else {
        state.fs.create_fd(
            write_rights,
            write_rights,
            Fdflags::empty(),
            Fdflagsext::empty(),
            0,
            tx_inode,
        )?
    };

    Ok((read_fd, write_fd))
}
