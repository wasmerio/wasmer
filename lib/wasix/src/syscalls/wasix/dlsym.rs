use state::MAIN_MODULE_HANDLE;

use super::*;
use crate::{
    state::{ModuleHandle, ResolvedExport},
    syscalls::*,
};

#[instrument(level = "trace", skip_all, fields(symbol = field::Empty), ret)]
pub fn dlsym<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    handle: DlHandle,
    symbol: WasmPtr<u8, M>,
    symbol_len: M::Offset,
    err_buf: WasmPtr<u8, M>,
    err_buf_len: M::Offset,
    out_symbol: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let (env, mut store) = ctx.data_and_store_mut();
    let memory = unsafe { env.memory_view(&store) };
    let symbol = unsafe { get_input_str_ok!(&memory, symbol, symbol_len) };
    Span::current().record("symbol", symbol.as_str());

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

    // handle = 0 is RTLD_DEFAULT, so search everywhere
    let handle = if handle == 0 {
        None
    } else {
        Some(ModuleHandle::from(handle))
    };
    let symbol = linker.resolve_export(&mut store, handle, &symbol);

    let (env, mut store) = ctx.data_and_store_mut();
    let memory = unsafe { env.memory_view(&store) };

    let symbol = wasi_try_dl!(
        symbol,
        "failed to resolve symbol: {}",
        memory,
        err_buf,
        err_buf_len
    );

    match symbol {
        ResolvedExport::Function(func) => {
            let func_index = linker.append_to_function_table(&mut store, func);

            let (env, mut store) = ctx.data_and_store_mut();
            let memory = unsafe { env.memory_view(&store) };

            let func_index = wasi_try_dl!(
                func_index,
                "failed to grow indirect function table: {}",
                memory,
                err_buf,
                err_buf_len
            );

            wasi_try_mem_ok!(out_symbol.write(&memory, func_index.into()));
        }
        ResolvedExport::Global(address) => {
            let Ok(address) = address.try_into() else {
                panic!("Failed to convert address to u64");
            };
            wasi_try_mem_ok!(out_symbol.write(&memory, address));
        }
    }

    Ok(Errno::Success)
}
