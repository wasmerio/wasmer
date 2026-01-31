use super::*;
use crate::syscalls::*;

/// ### `fd_fdstat_get()`
/// Get metadata of a file descriptor
/// Input:
/// - `Fd fd`
///     The file descriptor whose metadata will be accessed
/// Output:
/// - `Fdstat *buf`
///     The location where the metadata will be written
#[instrument(level = "trace", skip_all, fields(%fd), ret)]
pub fn fd_fdstat_get<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    buf_ptr: WasmPtr<Fdstat, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let (memory, state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
    let filetype = match fd_entry.kind {
        Kind::VfsFile { .. } => Filetype::RegularFile,
        Kind::VfsDir { .. } => Filetype::Directory,
        Kind::Stdin { .. } | Kind::Stdout { .. } | Kind::Stderr { .. } => Filetype::CharacterDevice,
        Kind::Socket { .. } => Filetype::SocketStream,
        Kind::PipeRx { .. } | Kind::PipeTx { .. } | Kind::DuplexPipe { .. } => Filetype::Unknown,
        Kind::EventNotifications { .. } | Kind::Epoll { .. } => Filetype::Unknown,
        Kind::Buffer { .. } => Filetype::RegularFile,
    };
    let stat = Fdstat {
        fs_filetype: filetype,
        fs_flags: fd_entry.inner.flags,
        fs_rights_base: fd_entry.inner.rights,
        fs_rights_inheriting: fd_entry.inner.rights_inheriting,
    };

    let buf = buf_ptr.deref(&memory);

    wasi_try_mem_ok!(buf.write(stat));

    Ok(Errno::Success)
}
