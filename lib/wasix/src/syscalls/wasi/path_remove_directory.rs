use super::*;
use crate::syscalls::*;

/// Returns Errno::Notemtpy if directory is not empty
#[instrument(level = "debug", skip_all, fields(%fd, path = field::Empty), ret)]
pub fn path_remove_directory<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
) -> Errno {
    // TODO check if fd is a dir, ensure it's within sandbox, etc.
    let env = ctx.data();
    let (memory, mut state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    let base_dir = wasi_try!(state.fs.get_fd(fd));
    let mut path_str = unsafe { get_input_str!(&memory, path, path_len) };
    Span::current().record("path", path_str.as_str());

    // Convert relative paths into absolute paths
    if path_str.starts_with("./") {
        path_str = ctx.data().state.fs.relative_path_to_absolute(path_str);
        trace!(
            %path_str
        );
    }

    wasi_try!(path_remove_directory_internal(&mut ctx, fd, &path_str));
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        wasi_try!(
            JournalEffector::save_path_remove_directory(&mut ctx, fd, path_str).map_err(|err| {
                tracing::error!("failed to save remove directory event - {}", err);
                Errno::Fault
            })
        )
    }

    Errno::Success
}

pub(crate) fn path_remove_directory_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    path: &str,
) -> Result<(), Errno> {
    let env = ctx.data();
    let (memory, mut state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    let inode = state.fs.get_inode_at_path(inodes, fd, path, false)?;
    let (parent_inode, childs_name) =
        state
            .fs
            .get_parent_inode_at_path(inodes, fd, std::path::Path::new(&path), false)?;

    let host_path_to_remove = {
        let guard = inode.read();
        match guard.deref() {
            Kind::Dir { entries, path, .. } => {
                if !entries.is_empty() || state.fs_read_dir(path)?.count() != 0 {
                    return Err(Errno::Notempty);
                }
                path.clone()
            }
            Kind::Root { .. } => return Err(Errno::Access),
            _ => return Err(Errno::Notdir),
        }
    };

    {
        let mut guard = parent_inode.write();
        match guard.deref_mut() {
            Kind::Dir {
                ref mut entries, ..
            } => {
                let removed_inode = entries.remove(&childs_name).ok_or(Errno::Inval)?;

                // TODO: make this a debug assert in the future
                assert!(inode.ino() == removed_inode.ino());
            }
            Kind::Root { .. } => return Err(Errno::Access),
            _ => unreachable!(
                "Internal logic error in wasi::path_remove_directory, parent is not a directory"
            ),
        }
    }

    let env = ctx.data();
    let (memory, mut state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };
    if let Err(err) = state.fs_remove_dir(host_path_to_remove) {
        // reinsert to prevent FS from being in bad state
        let mut guard = parent_inode.write();
        if let Kind::Dir {
            ref mut entries, ..
        } = guard.deref_mut()
        {
            entries.insert(childs_name, inode);
        }
        return Err(err);
    }

    Ok(())
}
