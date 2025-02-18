use wasmer::FromToNativeWasmType;

use super::*;
use crate::{
    os::task::{OwnedTaskStatus, TaskStatus},
    syscalls::*,
};

/// Replaces the current process with a new process
///
/// ## Parameters
///
/// * `name` - Name of the process to be spawned
/// * `args` - List of the arguments to pass the process
///   (entries are separated by line feeds)
/// * `envs` - List of the environment variables to pass process
///
/// ## Return
///
/// Returns a bus process id that can be used to invoke calls
#[instrument(level = "trace", skip_all, fields(name = field::Empty, %args_len), ret)]
pub fn proc_exec2<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
    args: WasmPtr<u8, M>,
    args_len: M::Offset,
    envs: WasmPtr<u8, M>,
    envs_len: M::Offset,
) -> Result<(), WasiError> {
    match proc_exec3(ctx, name, name_len, args, args_len, envs, envs_len) {
        Ok(e) => Err(WasiError::Exit(e.into())),
        Err(e) => Err(e),
    }
}
