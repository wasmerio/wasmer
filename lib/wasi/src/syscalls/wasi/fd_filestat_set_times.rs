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
pub fn fd_filestat_set_times(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    st_atim: Timestamp,
    st_mtim: Timestamp,
    fst_flags: Fstflags,
) -> Errno {
    debug!(
        "wasi[{}:{}]::fd_filestat_set_times",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let (_, mut state) = env.get_memory_and_wasi_state(&ctx, 0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));

    if !fd_entry.rights.contains(Rights::FD_FILESTAT_SET_TIMES) {
        return Errno::Access;
    }

    if (fst_flags.contains(Fstflags::SET_ATIM) && fst_flags.contains(Fstflags::SET_ATIM_NOW))
        || (fst_flags.contains(Fstflags::SET_MTIM) && fst_flags.contains(Fstflags::SET_MTIM_NOW))
    {
        return Errno::Inval;
    }

    let inode = fd_entry.inode;

    if fst_flags.contains(Fstflags::SET_ATIM) || fst_flags.contains(Fstflags::SET_ATIM_NOW) {
        let time_to_set = if fst_flags.contains(Fstflags::SET_ATIM) {
            st_atim
        } else {
            wasi_try!(get_current_time_in_nanos())
        };
        inode.stat.write().unwrap().st_atim = time_to_set;
    }

    if fst_flags.contains(Fstflags::SET_MTIM) || fst_flags.contains(Fstflags::SET_MTIM_NOW) {
        let time_to_set = if fst_flags.contains(Fstflags::SET_MTIM) {
            st_mtim
        } else {
            wasi_try!(get_current_time_in_nanos())
        };
        inode.stat.write().unwrap().st_mtim = time_to_set;
    }

    Errno::Success
}
