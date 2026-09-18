use state::ModuleHandle;

use super::*;
use crate::syscalls::*;

#[instrument(level = "trace", skip_all, fields(path = field::Empty), ret)]
pub fn dl_invalid_handle(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    handle: DlHandle,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let (env, mut store) = ctx.data_and_store_mut();
    let memory = unsafe { env.memory_view(&store) };

    let handle = if handle == 0 {
        // Handle zero is the main module, and never a valid side module handle
        return Ok(Errno::Noexec);
    } else {
        ModuleHandle::from(handle)
    };

    let env_inner = unsafe { env.inner() };
    let Some(linker) = env_inner.linker() else {
        // No linker means no handles
        return Ok(Errno::Noexec);
    };

    let result = linker.clone().is_handle_valid(handle, &mut ctx);
    if let Err(error) = &result
        && let Some(code) = error.termination_code()
    {
        return Err(WasiError::Exit(code));
    }
    let is_valid = result.unwrap_or(false);

    Ok(if is_valid {
        Errno::Success
    } else {
        Errno::Noexec
    })
}
