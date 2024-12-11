use std::fs;

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
    let path_str = unsafe { get_input_str!(&memory, path, path_len) };
    Span::current().record("path", path_str.as_str());

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

    let (parent_inode, dir_name) =
        state
            .fs
            .get_parent_inode_at_path(inodes, fd, Path::new(path), true)?;

    let mut guard = parent_inode.write();
    match guard.deref_mut() {
        Kind::Dir {
            entries: ref mut parent_entries,
            ..
        } => {
            let Some(child_inode) = parent_entries.get(&dir_name) else {
                return Err(Errno::Noent);
            };

            {
                let Kind::Dir {
                    entries: ref child_entries,
                    path: ref child_path,
                    ..
                } = *child_inode.read()
                else {
                    return Err(Errno::Notdir);
                };

                if !child_entries.is_empty() {
                    return Err(Errno::Notempty);
                }

                if let Err(e) = state.fs_remove_dir(child_path) {
                    tracing::warn!(path = ?child_path, error = ?e, "failed to remove directory");
                    return Err(e);
                }
            }

            parent_entries.remove(&dir_name).expect(
                "Entry should exist since we checked before and have an exclusive write lock",
            );

            Ok(())
        }
        Kind::Root { .. } => {
            trace!("directories directly in the root node can not be removed");
            Err(Errno::Access)
        }
        _ => {
            trace!("path is not a directory");
            Err(Errno::Notdir)
        }
    }
}
