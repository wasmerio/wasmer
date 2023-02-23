use super::*;
use crate::syscalls::*;

/// ### `path_create_directory()`
/// Create directory at a path
/// Inputs:
/// - `Fd fd`
///     The directory that the path is relative to
/// - `const char *path`
///     String containing path data
/// - `u32 path_len`
///     The length of `path`
/// Errors:
/// Required Rights:
/// - Rights::PATH_CREATE_DIRECTORY
///     This right must be set on the directory that the file is created in (TODO: verify that this is true)
pub fn path_create_directory<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
) -> Errno {
    debug!(
        "wasi[{}:{}]::path_create_directory",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let (memory, state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);

    let working_dir = wasi_try!(state.fs.get_fd(fd));
    {
        let guard = working_dir.inode.read();
        if let Kind::Root { .. } = guard.deref() {
            return Errno::Access;
        }
    }
    if !working_dir.rights.contains(Rights::PATH_CREATE_DIRECTORY) {
        return Errno::Access;
    }
    let mut path_string = unsafe { get_input_str!(&memory, path, path_len) };
    debug!("=> fd: {}, path: {}", fd, &path_string);

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

    let path = std::path::PathBuf::from(&path_string);
    let path_vec = wasi_try!(path
        .components()
        .map(|comp| {
            comp.as_os_str()
                .to_str()
                .map(|inner_str| inner_str.to_string())
                .ok_or(Errno::Inval)
        })
        .collect::<Result<Vec<String>, Errno>>());
    if path_vec.is_empty() {
        return Errno::Inval;
    }

    debug!("Looking at components {:?}", &path_vec);

    let mut cur_dir_inode = working_dir.inode;
    for comp in &path_vec {
        debug!("Creating dir {}", comp);

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
                            return Errno::Notdir;
                        }
                    } else {
                        wasi_try!(state.fs_create_dir(&adjusted_path));
                    }
                    let kind = Kind::Dir {
                        parent: cur_dir_inode.downgrade(),
                        path: adjusted_path,
                        entries: Default::default(),
                    };
                    let new_inode =
                        wasi_try!(state.fs.create_inode(inodes, kind, false, comp.to_string()));

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
            Kind::Root { .. } => return Errno::Access,
            _ => return Errno::Notdir,
        }
    }

    Errno::Success
}
