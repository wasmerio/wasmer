use super::*;
use crate::syscalls::*;

/// Returns Errno::Notemtpy if directory is not empty
#[instrument(level = "trace", skip_all, fields(%fd, path = field::Empty), ret)]
pub fn path_remove_directory<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    // TODO check if fd is a dir, ensure it's within sandbox, etc.
    let env = ctx.data();
    let (memory, mut state, _inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    let base_dir = wasi_try_ok!(state.fs.get_fd(fd));
    let path_str = unsafe { get_input_str_ok!(&memory, path, path_len) };
    Span::current().record("path", path_str.as_str());

    wasi_try_ok!(path_remove_directory_internal(
        &mut ctx, fd, base_dir, &path_str
    ));
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        wasi_try_ok!(
            JournalEffector::save_path_remove_directory(&mut ctx, fd, path_str).map_err(|err| {
                tracing::error!("failed to save remove directory event - {}", err);
                Errno::Fault
            })
        )
    }

    Ok(Errno::Success)
}

pub(crate) fn path_remove_directory_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    _base_dir: Fd,
    path: &str,
) -> Result<(), Errno> {
    let env = ctx.data();
    let (state, inodes) = env.get_wasi_state_and_inodes();
    state.fs.remove_directory(inodes, fd, path)
}
