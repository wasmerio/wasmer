use wasmer_vfs::Pipe;

use super::*;
use crate::syscalls::*;

/// ### `fd_pipe()`
/// Creates ta pipe that feeds data between two file handles
/// Output:
/// - `Fd`
///     First file handle that represents one end of the pipe
/// - `Fd`
///     Second file handle that represents the other end of the pipe
pub fn fd_pipe<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    ro_fd1: WasmPtr<WasiFd, M>,
    ro_fd2: WasmPtr<WasiFd, M>,
) -> Errno {
    trace!("wasi[{}:{}]::fd_pipe", ctx.data().pid(), ctx.data().tid());

    let env = ctx.data();
    let (memory, state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);

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
        | Rights::FD_FDSTAT_SET_FLAGS;
    let fd1 = wasi_try!(state
        .fs
        .create_fd(rights, rights, Fdflags::empty(), 0, inode1));
    let fd2 = wasi_try!(state
        .fs
        .create_fd(rights, rights, Fdflags::empty(), 0, inode2));
    trace!(
        "wasi[{}:{}]::fd_pipe (fd1={}, fd2={})",
        ctx.data().pid(),
        ctx.data().tid(),
        fd1,
        fd2
    );

    wasi_try_mem!(ro_fd1.write(&memory, fd1));
    wasi_try_mem!(ro_fd2.write(&memory, fd2));

    Errno::Success
}
