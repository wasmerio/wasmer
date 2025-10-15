use super::*;
use crate::syscalls::*;

/// ### `stackswitching_switch()`
#[instrument(level = "trace", skip_all, ret)]
pub fn stackswitching_switch<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    stack: u32,
) -> Result<(), WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    Ok(())
}
