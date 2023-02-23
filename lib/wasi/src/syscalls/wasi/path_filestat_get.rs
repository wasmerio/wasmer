use super::*;
use crate::syscalls::*;

/// ### `path_filestat_get()`
/// Access metadata about a file or directory
/// Inputs:
/// - `Fd fd`
///     The directory that `path` is relative to
/// - `LookupFlags flags`
///     Flags to control how `path` is understood
/// - `const char *path`
///     String containing the file path
/// - `u32 path_len`
///     The length of the `path` string
/// Output:
/// - `__wasi_file_stat_t *buf`
///     The location where the metadata will be stored
pub fn path_filestat_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    flags: LookupFlags,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
    buf: WasmPtr<Filestat, M>,
) -> Errno {
    let env = ctx.data();
    let (memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);

    let mut path_string = unsafe { get_input_str!(&memory, path, path_len) };
    trace!(
        "wasi[{}:{}]::path_filestat_get (fd={}, path={})",
        ctx.data().pid(),
        ctx.data().tid(),
        fd,
        path_string
    );

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

    let stat = wasi_try!(path_filestat_get_internal(
        &memory,
        state,
        inodes,
        fd,
        flags,
        &path_string
    ));

    wasi_try_mem!(buf.deref(&memory).write(stat));

    Errno::Success
}

/// ### `path_filestat_get()`
/// Access metadata about a file or directory
/// Inputs:
/// - `Fd fd`
///     The directory that `path` is relative to
/// - `LookupFlags flags`
///     Flags to control how `path` is understood
/// - `const char *path`
///     String containing the file path
/// - `u32 path_len`
///     The length of the `path` string
/// Output:
/// - `__wasi_file_stat_t *buf`
///     The location where the metadata will be stored
pub(crate) fn path_filestat_get_internal(
    memory: &MemoryView,
    state: &WasiState,
    inodes: &crate::WasiInodes,
    fd: WasiFd,
    flags: LookupFlags,
    path_string: &str,
) -> Result<Filestat, Errno> {
    let root_dir = state.fs.get_fd(fd)?;

    if !root_dir.rights.contains(Rights::PATH_FILESTAT_GET) {
        return Err(Errno::Access);
    }
    debug!("=> base_fd: {}, path: {}", fd, path_string);

    let file_inode = state.fs.get_inode_at_path(
        inodes,
        fd,
        path_string,
        flags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0,
    )?;
    if file_inode.is_preopened {
        Ok(*file_inode.stat.read().unwrap().deref())
    } else {
        let guard = file_inode.read();
        state.fs.get_stat_for_kind(guard.deref())
    }
}
