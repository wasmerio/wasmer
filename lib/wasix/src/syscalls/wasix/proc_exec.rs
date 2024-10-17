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
///
/// ## Return
///
/// Returns a bus process id that can be used to invoke calls
#[instrument(level = "trace", skip_all, fields(name = field::Empty, %args_len), ret)]
pub fn proc_exec<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
    args: WasmPtr<u8, M>,
    args_len: M::Offset,
) -> Result<(), WasiError> {
    proc_exec2(
        ctx,
        name,
        name_len,
        args,
        args_len,
        WasmPtr::null(),
        M::ZERO,
    )
}
