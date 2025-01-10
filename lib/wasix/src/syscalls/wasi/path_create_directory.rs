use std::{
    path::{Component, PathBuf},
    str::FromStr,
};

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
#[instrument(level = "trace", skip_all, fields(%fd, path = field::Empty), ret)]
pub fn path_create_directory<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let (memory, state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    let mut path_string = unsafe { get_input_str_ok!(&memory, path, path_len) };
    Span::current().record("path", path_string.as_str());

    wasi_try_ok!(path_create_directory_internal(&mut ctx, fd, &path_string));
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_path_create_directory(&mut ctx, fd, path_string).map_err(|err| {
            tracing::error!("failed to save create directory event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub(crate) fn path_create_directory_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    path: &str,
) -> Result<(), Errno> {
    let env = ctx.data();
    let (memory, state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };
    let working_dir = state.fs.get_fd(fd)?;

    if !working_dir
        .inner
        .rights
        .contains(Rights::PATH_CREATE_DIRECTORY)
    {
        trace!("working directory (fd={fd}) has no rights to create a directory");
        return Err(Errno::Access);
    }

    let (parent_inode, dir_name) =
        state
            .fs
            .get_parent_inode_at_path(inodes, fd, Path::new(path), true)?;

    let mut guard = parent_inode.write();
    match guard.deref_mut() {
        Kind::Dir {
            ref entries,
            ref path,
            ..
        } => {
            if let Some(child) = entries.get(&dir_name) {
                return Err(Errno::Exist);
            }

            let mut new_dir_path = path.clone();
            new_dir_path.push(&dir_name);

            drop(guard);

            // TODO: This condition should already have been checked by the entries.get check
            // above, but it was in the code before my refactor and I'm keeping it just in case.
            if path_filestat_get_internal(
                &memory,
                state,
                inodes,
                fd,
                0,
                &new_dir_path.to_string_lossy(),
            )
            .is_ok()
            {
                return Err(Errno::Exist);
            }

            state.fs_create_dir(&new_dir_path)?;

            let kind = Kind::Dir {
                parent: parent_inode.downgrade(),
                path: new_dir_path,
                entries: Default::default(),
            };
            let new_inode = state
                .fs
                .create_inode(inodes, kind, false, dir_name.clone())?;

            // reborrow to insert
            {
                let mut guard = parent_inode.write();
                let Kind::Dir {
                    ref mut entries, ..
                } = guard.deref_mut()
                else {
                    unreachable!();
                };

                entries.insert(dir_name, new_inode.clone());
            }
        }
        Kind::Root { .. } => {
            trace!("the root node can only contain pre-opened directories");
            return Err(Errno::Access);
        }
        _ => {
            trace!("path is not a directory");
            return Err(Errno::Notdir);
        }
    }

    Ok(())
}
