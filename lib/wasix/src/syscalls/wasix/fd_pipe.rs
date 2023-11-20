use virtual_fs::Pipe;

use super::*;
use crate::syscalls::*;

/// ### `fd_pipe()`
/// Creates ta pipe that feeds data between two file handles
/// Output:
/// - `Fd`
///     First file handle that represents one end of the pipe
/// - `Fd`
///     Second file handle that represents the other end of the pipe
#[instrument(level = "trace", skip_all, fields(fd1 = field::Empty, fd2 = field::Empty), ret)]
pub fn fd_pipe<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    ro_fd1: WasmPtr<WasiFd, M>,
    ro_fd2: WasmPtr<WasiFd, M>,
) -> Result<Errno, WasiError> {
    let (fd1, fd2) = wasi_try_ok!(fd_pipe_internal(&mut ctx));
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_fd_pipe(&mut ctx, fd1, fd2).map_err(|err| {
            tracing::error!("failed to save create pipe event - {}", err);
            WasiError::Exit(ExitCode::Errno(Errno::Fault))
        })?;
    }

    let env = ctx.data();
    let (memory, state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    Span::current().record("fd1", fd1).record("fd2", fd2);

    wasi_try_mem_ok!(ro_fd1.write(&memory, fd1));
    wasi_try_mem_ok!(ro_fd2.write(&memory, fd2));

    Ok(Errno::Success)
}

pub fn fd_pipe_internal(ctx: &mut FunctionEnvMut<'_, WasiEnv>) -> Result<(WasiFd, WasiFd), Errno> {
    let env = ctx.data();
    let (memory, state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };
    let (pipe1, pipe2) = Pipe::channel();

    let inode1 = state.fs.create_inode_with_default_stat(
        inodes,
        Kind::Pipe { pipe: pipe1 },
        false,
        "pipe".to_string().into(),
    );
    let inode2 = state.fs.create_inode_with_default_stat(
        inodes,
        Kind::Pipe { pipe: pipe2 },
        false,
        "pipe".to_string().into(),
    );

    let rights = Rights::FD_READ
        | Rights::FD_WRITE
        | Rights::FD_SYNC
        | Rights::FD_DATASYNC
        | Rights::POLL_FD_READWRITE
        | Rights::SOCK_SEND
        | Rights::FD_FDSTAT_SET_FLAGS;
    let fd1 = state
        .fs
        .create_fd(rights, rights, Fdflags::empty(), 0, inode1)?;
    let fd2 = state
        .fs
        .create_fd(rights, rights, Fdflags::empty(), 0, inode2)?;

    Ok((fd1, fd2))
}
