use super::*;
use crate::syscalls::*;

/// ### `fd_filestat_set_times()`
/// Set timestamp metadata on a file
/// Inputs:
/// - `Timestamp st_atim`
///     Last accessed time
/// - `Timestamp st_mtim`
///     Last modified time
/// - `Fstflags fst_flags`
///     Bit-vector for controlling which times get set
#[instrument(level = "trace", skip_all, fields(%fd, %st_atim, %st_mtim), ret)]
pub fn fd_filestat_set_times(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    st_atim: Timestamp,
    st_mtim: Timestamp,
    fst_flags: Fstflags,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    wasi_try_ok!(fd_filestat_set_times_internal(
        &mut ctx, fd, st_atim, st_mtim, fst_flags
    ));
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_fd_set_times(&mut ctx, fd, st_atim, st_mtim, fst_flags).map_err(
            |err| {
                tracing::error!("failed to save file set times event - {}", err);
                WasiError::Exit(ExitCode::from(Errno::Fault))
            },
        )?;
    }

    Ok(Errno::Success)
}

pub(crate) fn fd_filestat_set_times_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    st_atim: Timestamp,
    st_mtim: Timestamp,
    fst_flags: Fstflags,
) -> Result<(), Errno> {
    let state = ctx.data().get_wasi_state();
    let fd_entry = state.fs.get_fd(fd)?;
    if !fd_entry
        .inner
        .rights
        .contains(Rights::FD_FILESTAT_SET_TIMES)
    {
        return Err(Errno::Access);
    }
    let (atime, mtime) = timestamp_updates(st_atim, st_mtim, fst_flags)?;
    state.fs.set_times_for_inode(&fd_entry.inode, atime, mtime)
}
