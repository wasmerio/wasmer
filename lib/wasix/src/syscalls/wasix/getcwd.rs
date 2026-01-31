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
    let (memory, state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let cur_dir = state.fs.current_dir();
    Span::current().record("path", String::from_utf8_lossy(cur_dir.as_bytes()).as_ref());

    let max_path_len = wasi_try_mem!(path_len.read(&memory));
    let max_path_len64: u64 = max_path_len.into();
    let path_slice = match (path.is_null(), max_path_len64) {
        (true, _) => None,
        (_, 0) => None,
        (_, _) => Some(wasi_try_mem!(path.slice(&memory, max_path_len))),
    };
    Span::current().record("max_path_len", max_path_len64);

    let cur_dir_bytes = cur_dir.as_bytes();
    wasi_try_mem!(path_len.write(&memory, wasi_try!(to_offset::<M>(cur_dir_bytes.len()))));
    if cur_dir_bytes.len() as u64 > max_path_len64 {
        return Errno::Range;
    }

    if let Some(path_slice) = path_slice {
        let cur_dir = {
            let mut u8_buffer = vec![0; max_path_len64 as usize];
            let cur_dir_len = cur_dir_bytes.len();
            if (cur_dir_len as u64) <= max_path_len64 {
                u8_buffer[..cur_dir_len].clone_from_slice(cur_dir_bytes);
            } else {
                return Errno::Range;
            }
            u8_buffer
        };

        wasi_try_mem!(path_slice.write_slice(cur_dir.as_ref()));
        Errno::Success
    } else {
        Errno::Inval
    }
}
