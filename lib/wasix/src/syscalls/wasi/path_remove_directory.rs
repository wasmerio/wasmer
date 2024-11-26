use super::*;
use crate::syscalls::*;

/// Returns Errno::Notemtpy if directory is not empty
#[instrument(level = "trace", skip_all, fields(%fd, path = field::Empty), ret)]
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
    let (memory, state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };
    let working_dir = state.fs.get_fd(fd)?;

    let path = std::path::PathBuf::from(path);
    let path_vec = path
        .components()
        .map(|comp| {
            comp.as_os_str()
                .to_str()
                .map(|inner_str| inner_str.to_string())
                .ok_or(Errno::Inval)
        })
        .collect::<Result<Vec<String>, Errno>>()?;
    if path_vec.is_empty() {
        trace!("path vector is invalid (its empty)");
        return Err(Errno::Inval);
    }

    let (child, parent) = path_vec.split_last().unwrap();

    // if path only contains one component (the root), operation is not permitted
    if child.is_empty() {
        return Err(Errno::Access);
    }

    let mut cur_dir_inode = working_dir.inode;
    for comp in path_vec.iter() {
        let processing_cur_dir_inode = cur_dir_inode.clone();
        let mut guard = processing_cur_dir_inode.write();
        match guard.deref_mut() {
            Kind::Dir {
                ref mut entries,
                path,
                parent,
            } => {
                match comp.borrow() {
                    ".." => {
                        if let Some(p) = parent.upgrade() {
                            cur_dir_inode = p;
                            continue;
                        }
                    }
                    "." => continue,
                    _ => (),
                }
                if let Some(child) = entries.get(comp) {
                    cur_dir_inode = child.clone();
                } else {
                    let parent_path = path.clone();
                    let mut adjusted_path = path.clone();
                    drop(guard);

                    // TODO: double check this doesn't risk breaking the sandbox
                    adjusted_path.push(comp);
                    if let Ok(adjusted_path_stat) = path_filestat_get_internal(
                        &memory,
                        state,
                        inodes,
                        fd,
                        0,
                        &adjusted_path.to_string_lossy(),
                    ) {
                        if adjusted_path_stat.st_filetype != Filetype::Directory {
                            trace!("path is not a directory");
                            return Err(Errno::Notdir);
                        }
                    } else {
                        return Err(Errno::Noent);
                    }
                    let kind = Kind::Dir {
                        parent: cur_dir_inode.downgrade(),
                        path: adjusted_path,
                        entries: Default::default(),
                    };
                    let new_inode = state
                        .fs
                        .create_inode(inodes, kind, false, comp.to_string())?;

                    // reborrow to insert
                    {
                        let mut guard = cur_dir_inode.write();
                        if let Kind::Dir {
                            ref mut entries, ..
                        } = guard.deref_mut()
                        {
                            entries.insert(comp.to_string(), new_inode.clone());
                        }
                    }
                    cur_dir_inode = new_inode;
                }
            }
            Kind::Root { .. } => {
                trace!("the root node can no create a directory");
                return Err(Errno::Access);
            }
            _ => {
                trace!("path is not a directory");
                return Err(Errno::Notdir);
            }
        }
    }

    if let Kind::Dir {
        parent,
        path: child_path,
        entries,
    } = cur_dir_inode.write().deref_mut()
    {
        let parent = parent.upgrade().ok_or(Errno::Noent)?;

        if let Kind::Dir { entries, .. } = parent.write().deref_mut() {
            let child_inode = entries.remove(child).ok_or(Errno::Noent)?;

            if let Err(e) = state.fs_remove_dir(&child_path) {
                tracing::warn!(path = ?child_path, error = ?e, "failed to remove directory");
            }
        }

        drop(parent)
    }

    Ok(())
}
