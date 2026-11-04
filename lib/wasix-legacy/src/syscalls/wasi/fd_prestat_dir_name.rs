use super::*;
use crate::syscalls::*;

#[instrument(level = "trace", skip_all, fields(%fd, path = field::Empty), ret)]
pub fn fd_prestat_dir_name<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
) -> Errno {
    let env = ctx.data();
    let (memory, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let path_chars = wasi_try_mem!(path.slice(&memory, path_len));

    let inode = wasi_try!(state.fs.get_fd_inode(fd));
    let name = inode.name.read().unwrap();
    Span::current().record("path", name.as_ref());

    // check inode-val.is_preopened?

    let guard = inode.read();
    match guard.deref() {
        Kind::Dir { .. } | Kind::Root { .. } => {
            let path_len: u64 = path_len.into();
            let name_len = name.len() as u64;
            if name_len <= path_len {
                wasi_try_mem!(
                    path_chars
                        .subslice(0..name_len)
                        .write_slice(name.as_bytes())
                );
                // Note: We don't write a null terminator since WASI spec doesn't require it
                // and pr_name_len already gives the exact length

                Errno::Success
            } else {
                Errno::Overflow
            }
        }
        Kind::Symlink { .. }
        | Kind::Buffer { .. }
        | Kind::File { .. }
        | Kind::Socket { .. }
        | Kind::PipeRx { .. }
        | Kind::PipeTx { .. }
        | Kind::DuplexPipe { .. }
        | Kind::EventNotifications { .. }
        | Kind::Epoll { .. } => Errno::Notdir,
    }
}
