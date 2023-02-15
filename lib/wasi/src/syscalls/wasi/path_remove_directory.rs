use super::*;
use crate::syscalls::*;

/// Returns Errno::Notemtpy if directory is not empty
pub fn path_remove_directory<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
) -> Errno {
    // TODO check if fd is a dir, ensure it's within sandbox, etc.
    debug!(
        "wasi[{}:{}]::path_remove_directory",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let (memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);

    let base_dir = wasi_try!(state.fs.get_fd(fd));
    let mut path_str = unsafe { get_input_str!(&memory, path, path_len) };

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

    let host_path_to_remove = {
        let guard = inode.read();
        match guard.deref() {
            Kind::Dir { entries, path, .. } => {
                if !entries.is_empty() || wasi_try!(state.fs_read_dir(path)).count() != 0 {
                    return Errno::Notempty;
                }
                path.clone()
            }
            Kind::Root { .. } => return Errno::Access,
            _ => return Errno::Notdir,
        }
    };

    {
        let mut guard = parent_inode.write();
        match guard.deref_mut() {
            Kind::Dir {
                ref mut entries, ..
            } => {
                let removed_inode = wasi_try!(entries.remove(&childs_name).ok_or(Errno::Inval));
                // TODO: make this a debug assert in the future
                assert!(inode.ino() == removed_inode.ino());
            }
            Kind::Root { .. } => return Errno::Access,
            _ => unreachable!(
                "Internal logic error in wasi::path_remove_directory, parent is not a directory"
            ),
        }
    }

    if let Err(err) = state.fs_remove_dir(host_path_to_remove) {
        // reinsert to prevent FS from being in bad state
        let mut guard = parent_inode.write();
        if let Kind::Dir {
            ref mut entries, ..
        } = guard.deref_mut()
        {
            entries.insert(childs_name, inode);
        }
        return err;
    }

    Errno::Success
}
