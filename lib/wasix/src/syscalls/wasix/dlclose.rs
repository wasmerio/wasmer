use state::ModuleHandle;

use super::*;
use crate::syscalls::*;

#[instrument(level = "trace", skip_all, fields(path = field::Empty), ret)]
pub fn dlclose<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    handle: DlHandle,
    err_buf: WasmPtr<u8, M>,
    err_buf_len: M::Offset,
) -> Result<Errno, WasiError> {
    let (env, mut store) = ctx.data_and_store_mut();
    let memory = unsafe { env.memory_view(&store) };

    let handle = if handle == 0 {
        wasi_dl_err!("Invalid handle: 0", memory, err_buf, err_buf_len);
    } else {
        ModuleHandle::from(handle)
    };

    let env_inner = unsafe { env.inner() };
    let Some(linker) = env_inner.linker() else {
        wasi_dl_err!(
            "The current instance is not a dynamically-linked instance",
            memory,
            err_buf,
            err_buf_len
        );
    };
    let linker = linker.clone();

    let result = linker.unload_module(handle, &mut ctx.as_mut());

    // Reborrow to keep rust happy
    let (env, mut store) = ctx.data_and_store_mut();
    let memory = unsafe { env.memory_view(&store) };

    let () = wasi_try_dl!(
        result,
        "failed to unload module: {}",
        memory,
        err_buf,
        err_buf_len
    );

    Ok(Errno::Success)
}
