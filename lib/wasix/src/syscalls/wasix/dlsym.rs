use super::*;
use crate::{
    linker::{ModuleHandle, ResolvedExport},
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
    let (env, mut store) = ctx.data_and_store_mut();
    let memory = unsafe { env.memory_view(&store) };
    let symbol = unsafe { get_input_str_ok!(&memory, symbol, symbol_len) };
    Span::current().record("symbol", symbol.as_str());

    let Some(linker) = env.linker.as_ref() else {
        wasi_dl_err!(
            "no dl modules have been loaded",
            memory,
            err_buf,
            err_buf_len
        );
    };

    let handle = ModuleHandle::from(handle);
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
            // TODO: this does not work if called from a side module since we're using the
            // WasiInstanceHandles from the calling module, need proper storage of all instances
            // and instance handles in the WasiEnv

            let Some(table) = unsafe { env.inner() }.indirect_function_table.as_ref() else {
                wasi_dl_err!(
                    "The module does not export its indirect function table",
                    memory,
                    err_buf,
                    err_buf_len
                );
            };

            let func_index = table.grow(&mut store, 1, func.into());

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
