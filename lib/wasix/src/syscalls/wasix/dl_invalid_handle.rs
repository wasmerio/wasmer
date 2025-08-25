use state::ModuleHandleWithFlags;

use super::*;
use crate::{state::ModuleHandle, syscalls::*};

// TODO: dl invalid handle
#[instrument(level = "trace", skip_all, fields(path = field::Empty), ret)]
pub fn dl_invalid_handle(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    handle: DlHandle,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let (env, mut store) = ctx.data_and_store_mut();
    let memory = unsafe { env.memory_view(&store) };

    let Ok(handle) = ModuleHandle::try_from(handle) else {
        return Ok(Errno::Noexec);
    };

    let env_inner = unsafe { env.inner() };
    let Some(linker) = env_inner.linker() else {
        // No linker means no handles
        return Ok(Errno::Noexec);
    };

    let result = linker
        .clone()
        .is_handle_valid(handle, &mut ctx)
        .map(|v| if v { Errno::Success } else { Errno::Noexec })
        .unwrap_or(Errno::Noexec);

    Ok(result)
}
