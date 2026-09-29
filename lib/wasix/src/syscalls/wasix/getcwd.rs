use super::*;
use crate::syscalls::*;

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

    let max_path_len64: u64 = wasi_try_mem!(path_len.read(&memory)).into();
    Span::current().record("max_path_len", max_path_len64);

    let (_, cur_dir) = wasi_try!(state.fs.get_current_dir(inodes, crate::VIRTUAL_ROOT_FD));
    Span::current().record("path", cur_dir.as_str());

    let cur_dir_len = cur_dir.len();
    wasi_try_mem!(path_len.write(&memory, wasi_try!(to_offset::<M>(cur_dir_len))));
    if cur_dir_len as u64 > max_path_len64 {
        return Errno::Range;
    }

    if path.is_null() || max_path_len64 == 0 {
        return Errno::Inval;
    }

    let path_slice = wasi_try_mem!(path.slice(&memory, wasi_try!(to_offset::<M>(cur_dir_len))));
    wasi_try_mem!(path_slice.write_slice(cur_dir.as_bytes()));
    // null-terminate the path if it's shorter than max_path_len64
    if (cur_dir_len as u64) < max_path_len64 {
        let path_nul = wasi_try_mem!(path.add_offset(wasi_try!(to_offset::<M>(cur_dir_len))));
        // null-terminate the path
        wasi_try_mem!(path_nul.write(&memory, 0));
    }
    Errno::Success
}
