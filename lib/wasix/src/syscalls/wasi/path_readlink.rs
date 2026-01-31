use vfs_core::{ReadlinkOptions, ResolveFlags, VfsBaseDirAsync, VfsPath};
use vfs_unix::errno::vfs_error_to_wasi_errno;

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
    let (memory, state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let base_dir = wasi_try_ok!(state.fs.get_fd(dir_fd));
    if !base_dir.inner.rights.contains(Rights::PATH_READLINK) {
        return Ok(Errno::Access);
    }
    let mut path_str = unsafe { get_input_str_ok!(&memory, path, path_len) };
    Span::current().record("path", path_str.as_str());

    let dir_handle = match base_dir.kind {
        Kind::VfsDir { handle } => handle,
        _ => return Ok(Errno::Badf),
    };

    let ctx = state.fs.ctx.read().unwrap().clone();
    let path_bytes = path_str.as_bytes().to_vec();
    let res = __asyncify_light(env, None, async move {
        state
            .fs
            .vfs
            .readlinkat_async(
                &ctx,
                VfsBaseDirAsync::Handle(&dir_handle),
                VfsPath::new(&path_bytes),
                ReadlinkOptions {
                    resolve: ResolveFlags::empty(),
                },
            )
            .await
            .map_err(|err| vfs_error_to_wasi_errno(&err))
    })?;

    let path_buf = match res {
        Ok(path_buf) => path_buf,
        Err(err) => return Ok(err),
    };

    let buf_len: u64 = buf_len.into();
    let bytes = path_buf.as_bytes();
    if bytes.len() as u64 >= buf_len {
        return Ok(Errno::Overflow);
    }

    let out = wasi_try_mem_ok!(buf.slice(&memory, wasi_try_ok!(to_offset::<M>(bytes.len()))));
    wasi_try_mem_ok!(out.write_slice(bytes));

    let bytes_len: M::Offset = wasi_try_ok!(bytes.len().try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(buf_used.deref(&memory).write(bytes_len));

    Ok(Errno::Success)
}
