use super::*;
use crate::syscalls::*;

/// ### `path_link()`
/// Create a hard link
/// Inputs:
/// - `Fd old_fd`
///     The directory relative to which the `old_path` is
/// - `LookupFlags old_flags`
///     Flags to control how `old_path` is understood
/// - `const char *old_path`
///     String containing the old file path
/// - `u32 old_path_len`
///     Length of the `old_path` string
/// - `Fd new_fd`
///     The directory relative to which the `new_path` is
/// - `const char *new_path`
///     String containing the new file path
/// - `u32 old_path_len`
///     Length of the `new_path` string
pub fn path_link<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    old_fd: WasiFd,
    old_flags: LookupFlags,
    old_path: WasmPtr<u8, M>,
    old_path_len: M::Offset,
    new_fd: WasiFd,
    new_path: WasmPtr<u8, M>,
    new_path_len: M::Offset,
) -> Errno {
    debug!("wasi[{}:{}]::path_link", ctx.data().pid(), ctx.data().tid());
    if old_flags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0 {
        debug!("  - will follow symlinks when opening path");
    }
    let env = ctx.data();
    let (memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let mut old_path_str = unsafe { get_input_str!(&memory, old_path, old_path_len) };
    let mut new_path_str = unsafe { get_input_str!(&memory, new_path, new_path_len) };
    let source_fd = wasi_try!(state.fs.get_fd(old_fd));
    let target_fd = wasi_try!(state.fs.get_fd(new_fd));
    debug!(
        "=> source_fd: {}, source_path: {}, target_fd: {}, target_path: {}",
        old_fd, &old_path_str, new_fd, new_path_str
    );

    if !source_fd.rights.contains(Rights::PATH_LINK_SOURCE)
        || !target_fd.rights.contains(Rights::PATH_LINK_TARGET)
    {
        return Errno::Access;
    }

    // Convert relative paths into absolute paths
    old_path_str = ctx.data().state.fs.relative_path_to_absolute(old_path_str);
    new_path_str = ctx.data().state.fs.relative_path_to_absolute(new_path_str);

    let source_inode = wasi_try!(state.fs.get_inode_at_path(
        inodes,
        old_fd,
        &old_path_str,
        old_flags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0,
    ));
    let target_path_arg = std::path::PathBuf::from(&new_path_str);
    let (target_parent_inode, new_entry_name) =
        wasi_try!(state
            .fs
            .get_parent_inode_at_path(inodes, new_fd, &target_path_arg, false));

    if source_inode.stat.write().unwrap().st_nlink == Linkcount::max_value() {
        return Errno::Mlink;
    }
    {
        let mut guard = target_parent_inode.write();
        match guard.deref_mut() {
            Kind::Dir { entries, .. } => {
                if entries.contains_key(&new_entry_name) {
                    return Errno::Exist;
                }
                entries.insert(new_entry_name, source_inode.clone());
            }
            Kind::Root { .. } => return Errno::Inval,
            Kind::File { .. }
            | Kind::Symlink { .. }
            | Kind::Buffer { .. }
            | Kind::Socket { .. }
            | Kind::Pipe { .. }
            | Kind::EventNotifications { .. } => return Errno::Notdir,
        }
    }
    source_inode.stat.write().unwrap().st_nlink += 1;

    Errno::Success
}
