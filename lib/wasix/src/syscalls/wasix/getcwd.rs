use super::*;
use crate::syscalls::*;
use std::path::Path;
use virtual_fs::host_fs::normalize_path;

/// ### `getcwd()`
/// Returns the current working directory
/// If the path exceeds the size of the buffer then this function
/// will return ERANGE
#[instrument(level = "trace", skip_all, fields(path = field::Empty, max_path_len = field::Empty), ret)]
pub fn getcwd<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    path: WasmPtr<u8, M>,
    path_len: WasmPtr<M::Offset, M>,
) -> Errno {
    let env = ctx.data();
    let (memory, mut state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    let (_, cur_dir) = wasi_try!(state.fs.get_current_dir(inodes, crate::VIRTUAL_ROOT_FD));
    let mut cur_dir = cur_dir;
    let cur_dir_path = Path::new(cur_dir.as_str());
    if let Ok((parent_inode, entry_name)) =
        state
            .fs
            .get_parent_inode_at_path(inodes, crate::VIRTUAL_ROOT_FD, cur_dir_path, true)
    {
        let guard = parent_inode.read();
        if let Kind::Dir { entries, .. } = guard.deref() {
            if let Some(entry) = entries.get(entry_name.as_str()) {
                let entry_guard = entry.read();
                if let Kind::Symlink {
                    base_po_dir,
                    path_to_symlink,
                    relative_path,
                } = entry_guard.deref()
                {
                    let mut base = path_to_symlink.clone();
                    base.pop();
                    base.push(relative_path);
                    let base_inode = match state.fs.get_fd_inode(*base_po_dir) {
                        Ok(inode) => inode,
                        Err(err) => return err,
                    };
                    let base_name = base_inode.name.read().unwrap();
                    let mut resolved = Path::new(base_name.as_ref()).to_path_buf();
                    resolved.push(base);
                    cur_dir = normalize_path(resolved.as_path())
                        .to_string_lossy()
                        .into_owned();
                }
            }
        }
    }
    Span::current().record("path", cur_dir.as_str());

    let max_path_len = wasi_try_mem!(path_len.read(&memory));
    let max_path_len64: u64 = max_path_len.into();
    Span::current().record("max_path_len", max_path_len64);

    let cur_dir = cur_dir.as_bytes();
    let required_len = cur_dir.len() as u64 + 1;
    wasi_try_mem!(path_len.write(
        &memory,
        wasi_try!(to_offset::<M>(required_len as usize))
    ));
    if max_path_len64 < required_len {
        return Errno::Range;
    }

    if path.is_null() {
        return Errno::Fault;
    }

    let required_len_offset = wasi_try!(to_offset::<M>(required_len as usize));
    let path_slice = match path.slice(&memory, required_len_offset) {
        Ok(slice) => slice,
        Err(_) => return Errno::Fault,
    };
    let mut buffer = Vec::with_capacity(required_len as usize);
    buffer.extend_from_slice(cur_dir);
    buffer.push(0);
    if path_slice.write_slice(buffer.as_ref()).is_err() {
        return Errno::Fault;
    }

    Errno::Success
}
