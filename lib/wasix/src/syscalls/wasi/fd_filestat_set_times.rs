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
#[instrument(level = "debug", skip_all, fields(%fd, %st_atim, %st_mtim), ret)]
pub fn fd_filestat_set_times(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    st_atim: Timestamp,
    st_mtim: Timestamp,
    fst_flags: Fstflags,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(fd_filestat_set_times_internal(
        &mut ctx, fd, st_atim, st_mtim, fst_flags
    ));
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_fd_set_times(&mut ctx, fd, st_atim, st_mtim, fst_flags).map_err(
            |err| {
                tracing::error!("failed to save file set times event - {}", err);
                WasiError::Exit(ExitCode::Errno(Errno::Fault))
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
    let env = ctx.data();
    let (_, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let fd_entry = state.fs.get_fd(fd)?;

    if !fd_entry.rights.contains(Rights::FD_FILESTAT_SET_TIMES) {
        return Err(Errno::Access);
    }

    if (fst_flags.contains(Fstflags::SET_ATIM) && fst_flags.contains(Fstflags::SET_ATIM_NOW))
        || (fst_flags.contains(Fstflags::SET_MTIM) && fst_flags.contains(Fstflags::SET_MTIM_NOW))
    {
        return Err(Errno::Inval);
    }

    let inode = fd_entry.inode;

    if fst_flags.contains(Fstflags::SET_ATIM) || fst_flags.contains(Fstflags::SET_ATIM_NOW) {
        let time_to_set = if fst_flags.contains(Fstflags::SET_ATIM) {
            st_atim
        } else {
            get_current_time_in_nanos()?
        };
        inode.stat.write().unwrap().st_atim = time_to_set;
    }

    if fst_flags.contains(Fstflags::SET_MTIM) || fst_flags.contains(Fstflags::SET_MTIM_NOW) {
        let time_to_set = if fst_flags.contains(Fstflags::SET_MTIM) {
            st_mtim
        } else {
            get_current_time_in_nanos()?
        };
        inode.stat.write().unwrap().st_mtim = time_to_set;
    }

    Ok(())
}
