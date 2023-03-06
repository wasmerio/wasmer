use super::*;
use crate::syscalls::*;

/// ### `getcwd()`
/// Returns the current working directory
/// If the path exceeds the size of the buffer then this function
/// will fill the path_len with the needed size and return EOVERFLOW
pub fn getcwd<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    path: WasmPtr<u8, M>,
    path_len: WasmPtr<M::Offset, M>,
) -> Errno {
    debug!("wasi[{}:{}]::getcwd", ctx.data().pid(), ctx.data().tid());
    let env = ctx.data();
    let (memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);

    let (_, cur_dir) = wasi_try!(state.fs.get_current_dir(inodes, crate::VIRTUAL_ROOT_FD,));
    trace!(
        "wasi[{}:{}]::getcwd(current_dir={})",
        ctx.data().pid(),
        ctx.data().tid(),
        cur_dir
    );

    let max_path_len = wasi_try_mem!(path_len.read(&memory));
    let path_slice = wasi_try_mem!(path.slice(&memory, max_path_len));
    let max_path_len: u64 = max_path_len.into();

    let cur_dir = cur_dir.as_bytes();
    wasi_try_mem!(path_len.write(&memory, wasi_try!(to_offset::<M>(cur_dir.len()))));
    if cur_dir.len() as u64 >= max_path_len {
        return Errno::Overflow;
    }

    let cur_dir = {
        let mut u8_buffer = vec![0; max_path_len as usize];
        let cur_dir_len = cur_dir.len();
        if (cur_dir_len as u64) < max_path_len {
            u8_buffer[..cur_dir_len].clone_from_slice(cur_dir);
            u8_buffer[cur_dir_len] = 0;
        } else {
            return Errno::Overflow;
        }
        u8_buffer
    };

    wasi_try_mem!(path_slice.write_slice(&cur_dir[..]));
    Errno::Success
}
