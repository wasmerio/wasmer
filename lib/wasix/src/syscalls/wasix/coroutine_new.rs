use super::*;
use crate::syscalls::*;

/// ### `coroutine_new()`
#[instrument(level = "trace", skip_all, ret)]
pub fn coroutine_new<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    new_stack: WasmPtr<u32, M>,
    entrypoint: u32,
) -> Result<(), WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    Ok(())
}
