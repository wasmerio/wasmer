use super::*;
use crate::syscalls::*;

/// ### `path_filestat_set_times()`
/// Update time metadata on a file or directory
/// Inputs:
/// - `Fd fd`
///     The directory relative to which the path is resolved
/// - `LookupFlags flags`
///     Flags to control how the path is understood
/// - `const char *path`
///     String containing the file path
/// - `u32 path_len`
///     The length of the `path` string
/// - `Timestamp st_atim`
///     The timestamp that the last accessed time attribute is set to
/// -  `Timestamp st_mtim`
///     The timestamp that the last modified time attribute is set to
/// - `Fstflags fst_flags`
///     A bitmask controlling which attributes are set
pub fn path_filestat_set_times<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    flags: LookupFlags,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
    st_atim: Timestamp,
    st_mtim: Timestamp,
    fst_flags: Fstflags,
) -> Errno {
    debug!(
        "wasi[{}:{}]::path_filestat_set_times",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let (memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));
    let fd_inode = fd_entry.inode;
    if !fd_entry.rights.contains(Rights::PATH_FILESTAT_SET_TIMES) {
        return Errno::Access;
    }
    if (fst_flags.contains(Fstflags::SET_ATIM) && fst_flags.contains(Fstflags::SET_ATIM_NOW))
        || (fst_flags.contains(Fstflags::SET_MTIM) && fst_flags.contains(Fstflags::SET_MTIM_NOW))
    {
        return Errno::Inval;
    }

    let mut path_string = unsafe { get_input_str!(&memory, path, path_len) };
    debug!("=> base_fd: {}, path: {}", fd, &path_string);

    // Convert relative paths into absolute paths
    if path_string.starts_with("./") {
        path_string = ctx.data().state.fs.relative_path_to_absolute(path_string);
        trace!(
            "wasi[{}:{}]::rel_to_abs (name={}))",
            ctx.data().pid(),
            ctx.data().tid(),
            path_string
        );
    }

    let file_inode = wasi_try!(state.fs.get_inode_at_path(
        inodes,
        fd,
        &path_string,
        flags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0,
    ));
    let stat = {
        let guard = file_inode.read();
        wasi_try!(state.fs.get_stat_for_kind(guard.deref()))
    };

    if fst_flags.contains(Fstflags::SET_ATIM) || fst_flags.contains(Fstflags::SET_ATIM_NOW) {
        let time_to_set = if fst_flags.contains(Fstflags::SET_ATIM) {
            st_atim
        } else {
            wasi_try!(get_current_time_in_nanos())
        };
        fd_inode.stat.write().unwrap().st_atim = time_to_set;
    }
    if fst_flags.contains(Fstflags::SET_MTIM) || fst_flags.contains(Fstflags::SET_MTIM_NOW) {
        let time_to_set = if fst_flags.contains(Fstflags::SET_MTIM) {
            st_mtim
        } else {
            wasi_try!(get_current_time_in_nanos())
        };
        fd_inode.stat.write().unwrap().st_mtim = time_to_set;
    }

    Errno::Success
}
