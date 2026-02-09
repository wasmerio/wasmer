use super::*;
use crate::syscalls::*;

/// ### `path_readlink()`
/// Read the value of a symlink
/// Inputs:
/// - `Fd dir_fd`
///     The base directory from which `path` is understood
/// - `const char *path`
///     Pointer to UTF-8 bytes that make up the path to the symlink
/// - `u32 path_len`
///     The number of bytes to read from `path`
/// - `u32 buf_len`
///     Space available pointed to by `buf`
/// Outputs:
/// - `char *buf`
///     Pointer to characters containing the path that the symlink points to
/// - `u32 buf_used`
///     The number of bytes written to `buf`
#[instrument(level = "trace", skip_all, fields(%dir_fd, path = field::Empty), ret)]
pub fn path_readlink<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    dir_fd: WasiFd,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
    buf: WasmPtr<u8, M>,
    buf_len: M::Offset,
    buf_used: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let (memory, mut state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    let base_dir = wasi_try_ok!(state.fs.get_fd(dir_fd));
    if !base_dir.inner.rights.contains(Rights::PATH_READLINK) {
        return Ok(Errno::Access);
    }
    let mut path_str = unsafe { get_input_str_ok!(&memory, path, path_len) };
    Span::current().record("path", path_str.as_str());

    let inode = wasi_try_ok!(state.fs.get_inode_at_path(inodes, dir_fd, &path_str, false));

    {
        let guard = inode.read();
        if let Kind::Symlink { relative_path, .. } = guard.deref() {
            let rel_path_str = relative_path.to_string_lossy();
            let buf_len: u64 = buf_len.into();
            if buf_len == 0 {
                return Ok(Errno::Inval);
            }
            let bytes = rel_path_str.as_bytes();
            let write_len = std::cmp::min(bytes.len(), buf_len as usize);

            let out = wasi_try_mem_ok!(
                buf.slice(&memory, wasi_try_ok!(to_offset::<M>(write_len)))
            );
            if write_len > 0 {
                wasi_try_mem_ok!(out.write_slice(&bytes[..write_len]));
            }
            // should we null terminate this?

            let bytes_len: M::Offset =
                wasi_try_ok!(write_len.try_into().map_err(|_| Errno::Overflow));
            wasi_try_mem_ok!(buf_used.deref(&memory).write(bytes_len));
        } else {
            return Ok(Errno::Inval);
        }
    }

    Ok(Errno::Success)
}
