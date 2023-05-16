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
#[instrument(level = "debug", skip_all, fields(%dir_fd, path = field::Empty), ret)]
pub fn path_readlink<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    dir_fd: WasiFd,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
    buf: WasmPtr<u8, M>,
    buf_len: M::Offset,
    buf_used: WasmPtr<M::Offset, M>,
) -> Errno {
    let env = ctx.data();
    let (memory, mut state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    let base_dir = wasi_try!(state.fs.get_fd(dir_fd));
    if !base_dir.rights.contains(Rights::PATH_READLINK) {
        return Errno::Access;
    }
    let mut path_str = unsafe { get_input_str!(&memory, path, path_len) };
    Span::current().record("path", path_str.as_str());

    // Convert relative paths into absolute paths
    if path_str.starts_with("./") {
        path_str = ctx.data().state.fs.relative_path_to_absolute(path_str);
        trace!(
            %path_str
        );
    }

    let inode = wasi_try!(state.fs.get_inode_at_path(inodes, dir_fd, &path_str, false));

    {
        let guard = inode.read();
        if let Kind::Symlink { relative_path, .. } = guard.deref() {
            let rel_path_str = relative_path.to_string_lossy();
            let buf_len: u64 = buf_len.into();
            let bytes = rel_path_str.bytes();
            if bytes.len() as u64 >= buf_len {
                return Errno::Overflow;
            }
            let bytes: Vec<_> = bytes.collect();

            let out = wasi_try_mem!(buf.slice(&memory, wasi_try!(to_offset::<M>(bytes.len()))));
            wasi_try_mem!(out.write_slice(&bytes));
            // should we null terminate this?

            let bytes_len: M::Offset =
                wasi_try!(bytes.len().try_into().map_err(|_| Errno::Overflow));
            wasi_try_mem!(buf_used.deref(&memory).write(bytes_len));
        } else {
            return Errno::Inval;
        }
    }

    Errno::Success
}
