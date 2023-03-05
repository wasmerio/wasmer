use super::*;
use crate::syscalls::*;

/// ### `chdir()`
/// Sets the current working directory
#[instrument(level = "debug", skip_all, fields(name = field::Empty), ret)]
pub fn chdir<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
) -> Errno {
    let env = ctx.data();
    let (memory, mut state) = env.get_memory_and_wasi_state(&ctx, 0);
    let path = unsafe { get_input_str!(&memory, path, path_len) };
    Span::current().record("path", path.as_str());

    // Check if the directory exists
    if state.fs.root_fs.read_dir(Path::new(path.as_str())).is_err() {
        return Errno::Noent;
    }

    state.fs.set_current_dir(path.as_str());
    Errno::Success
}
