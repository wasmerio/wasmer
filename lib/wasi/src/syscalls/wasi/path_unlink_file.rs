use super::*;
use crate::syscalls::*;

/// ### `path_unlink_file()`
/// Unlink a file, deleting if the number of hardlinks is 1
/// Inputs:
/// - `Fd fd`
///     The base file descriptor from which the path is understood
/// - `const char *path`
///     Array of UTF-8 bytes representing the path
/// - `u32 path_len`
///     The number of bytes in the `path` array
pub fn path_unlink_file<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
) -> Errno {
    debug!(
        "wasi[{}:{}]::path_unlink_file",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let (memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);

    let base_dir = wasi_try!(state.fs.get_fd(fd));
    if !base_dir.rights.contains(Rights::PATH_UNLINK_FILE) {
        return Errno::Access;
    }
    let mut path_str = unsafe { get_input_str!(&memory, path, path_len) };
    debug!("Requested file: {}", path_str);

    // Convert relative paths into absolute paths
    if path_str.starts_with("./") {
        path_str = ctx.data().state.fs.relative_path_to_absolute(path_str);
        trace!(
            "wasi[{}:{}]::rel_to_abs (name={}))",
            ctx.data().pid(),
            ctx.data().tid(),
            path_str
        );
    }

    let inode = wasi_try!(state.fs.get_inode_at_path(inodes, fd, &path_str, false));
    let (parent_inode, childs_name) = wasi_try!(state.fs.get_parent_inode_at_path(
        inodes,
        fd,
        std::path::Path::new(&path_str),
        false
    ));

    let removed_inode = {
        let mut guard = parent_inode.write();
        match guard.deref_mut() {
            Kind::Dir {
                ref mut entries, ..
            } => {
                let removed_inode = wasi_try!(entries.remove(&childs_name).ok_or(Errno::Inval));
                // TODO: make this a debug assert in the future
                assert!(inode.ino() == removed_inode.ino());
                debug_assert!(inode.stat.read().unwrap().st_nlink > 0);
                removed_inode
            }
            Kind::Root { .. } => return Errno::Access,
            _ => unreachable!(
                "Internal logic error in wasi::path_unlink_file, parent is not a directory"
            ),
        }
    };

    let st_nlink = {
        let mut guard = removed_inode.stat.write().unwrap();
        guard.st_nlink -= 1;
        guard.st_nlink
    };
    if st_nlink == 0 {
        {
            let mut guard = removed_inode.read();
            match guard.deref() {
                Kind::File { handle, path, .. } => {
                    if let Some(h) = handle {
                        let mut h = h.write().unwrap();
                        wasi_try!(h.unlink().map_err(fs_error_into_wasi_err));
                    } else {
                        // File is closed
                        // problem with the abstraction, we can't call unlink because there's no handle
                        // drop mutable borrow on `path`
                        let path = path.clone();
                        drop(guard);
                        wasi_try!(state.fs_remove_file(path));
                    }
                }
                Kind::Dir { .. } | Kind::Root { .. } => return Errno::Isdir,
                Kind::Symlink { .. } => {
                    // TODO: actually delete real symlinks and do nothing for virtual symlinks
                }
                _ => unimplemented!("wasi::path_unlink_file for Buffer"),
            }
        }
    }

    Errno::Success
}
