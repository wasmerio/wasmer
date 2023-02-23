use super::*;
use crate::syscalls::*;

/// ### `path_rename()`
/// Rename a file or directory
/// Inputs:
/// - `Fd old_fd`
///     The base directory for `old_path`
/// - `const char* old_path`
///     Pointer to UTF8 bytes, the file to be renamed
/// - `u32 old_path_len`
///     The number of bytes to read from `old_path`
/// - `Fd new_fd`
///     The base directory for `new_path`
/// - `const char* new_path`
///     Pointer to UTF8 bytes, the new file name
/// - `u32 new_path_len`
///     The number of bytes to read from `new_path`
pub fn path_rename<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    old_fd: WasiFd,
    old_path: WasmPtr<u8, M>,
    old_path_len: M::Offset,
    new_fd: WasiFd,
    new_path: WasmPtr<u8, M>,
    new_path_len: M::Offset,
) -> Errno {
    debug!(
        "wasi::path_rename: old_fd = {}, new_fd = {}",
        old_fd, new_fd
    );
    let env = ctx.data();
    let (memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let mut source_str = unsafe { get_input_str!(&memory, old_path, old_path_len) };
    source_str = ctx.data().state.fs.relative_path_to_absolute(source_str);
    let source_path = std::path::Path::new(&source_str);
    let mut target_str = unsafe { get_input_str!(&memory, new_path, new_path_len) };
    target_str = ctx.data().state.fs.relative_path_to_absolute(target_str);
    let target_path = std::path::Path::new(&target_str);
    debug!("=> rename from {} to {}", &source_str, &target_str);

    {
        let source_fd = wasi_try!(state.fs.get_fd(old_fd));
        if !source_fd.rights.contains(Rights::PATH_RENAME_SOURCE) {
            return Errno::Access;
        }
        let target_fd = wasi_try!(state.fs.get_fd(new_fd));
        if !target_fd.rights.contains(Rights::PATH_RENAME_TARGET) {
            return Errno::Access;
        }
    }

    // this is to be sure the source file is fetch from filesystem if needed
    wasi_try!(state.fs.get_inode_at_path(
        inodes,
        old_fd,
        source_path.to_str().as_ref().unwrap(),
        true
    ));
    // Create the destination inode if the file exists.
    let _ =
        state
            .fs
            .get_inode_at_path(inodes, new_fd, target_path.to_str().as_ref().unwrap(), true);
    let (source_parent_inode, source_entry_name) =
        wasi_try!(state
            .fs
            .get_parent_inode_at_path(inodes, old_fd, source_path, true));
    let (target_parent_inode, target_entry_name) =
        wasi_try!(state
            .fs
            .get_parent_inode_at_path(inodes, new_fd, target_path, true));
    let mut need_create = true;
    let host_adjusted_target_path = {
        let guard = target_parent_inode.read();
        match guard.deref() {
            Kind::Dir { entries, path, .. } => {
                if entries.contains_key(&target_entry_name) {
                    need_create = false;
                }
                let mut out_path = path.clone();
                out_path.push(std::path::Path::new(&target_entry_name));
                out_path
            }
            Kind::Root { .. } => return Errno::Notcapable,
            Kind::Socket { .. } | Kind::Pipe { .. } | Kind::EventNotifications { .. } => {
                return Errno::Inval
            }
            Kind::Symlink { .. } | Kind::File { .. } | Kind::Buffer { .. } => {
                error!("Fatal internal logic error: parent of inode is not a directory");
                return Errno::Inval;
            }
        }
    };

    let source_entry = {
        let mut guard = source_parent_inode.write();
        match guard.deref_mut() {
            Kind::Dir { entries, .. } => {
                wasi_try!(entries.remove(&source_entry_name).ok_or(Errno::Noent))
            }
            Kind::Root { .. } => return Errno::Notcapable,
            Kind::Socket { .. } | Kind::Pipe { .. } | Kind::EventNotifications { .. } => {
                return Errno::Inval
            }
            Kind::Symlink { .. } | Kind::File { .. } | Kind::Buffer { .. } => {
                error!("Fatal internal logic error: parent of inode is not a directory");
                return Errno::Inval;
            }
        }
    };

    {
        let mut guard = source_entry.write();
        match guard.deref_mut() {
            Kind::File {
                handle, ref path, ..
            } => {
                // TODO: investigate why handle is not always there, it probably should be.
                // My best guess is the fact that a handle means currently open and a path
                // just means reference to host file on disk. But ideally those concepts
                // could just be unified even if there's a `Box<dyn VirtualFile>` which just
                // implements the logic of "I'm not actually a file, I'll try to be as needed".
                let result = if let Some(h) = handle {
                    drop(guard);
                    state.fs_rename(&source_path, &host_adjusted_target_path)
                } else {
                    let path_clone = path.clone();
                    drop(guard);
                    let out = state.fs_rename(&path_clone, &host_adjusted_target_path);
                    {
                        let mut guard = source_entry.write();
                        if let Kind::File { ref mut path, .. } = guard.deref_mut() {
                            *path = host_adjusted_target_path;
                        } else {
                            unreachable!()
                        }
                    }
                    out
                };
                // if the above operation failed we have to revert the previous change and then fail
                if let Err(e) = result {
                    let mut guard = source_parent_inode.write();
                    if let Kind::Dir { entries, .. } = guard.deref_mut() {
                        entries.insert(source_entry_name, source_entry);
                        return e;
                    }
                }
            }
            Kind::Dir { ref path, .. } => {
                let cloned_path = path.clone();
                if let Err(e) = state.fs_rename(cloned_path, &host_adjusted_target_path) {
                    return e;
                }
                {
                    drop(guard);
                    let mut guard = source_entry.write();
                    if let Kind::Dir { path, .. } = guard.deref_mut() {
                        *path = host_adjusted_target_path;
                    }
                }
            }
            Kind::Buffer { .. } => {}
            Kind::Symlink { .. } => {}
            Kind::Socket { .. } => {}
            Kind::Pipe { .. } => {}
            Kind::EventNotifications { .. } => {}
            Kind::Root { .. } => unreachable!("The root can not be moved"),
        }
    }

    if need_create {
        let mut guard = target_parent_inode.write();
        if let Kind::Dir { entries, .. } = guard.deref_mut() {
            let result = entries.insert(target_entry_name, source_entry);
            assert!(
                result.is_none(),
                "Fatal error: race condition on filesystem detected or internal logic error"
            );
        }
    }

    Errno::Success
}
