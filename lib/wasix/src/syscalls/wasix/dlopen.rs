use super::*;
use crate::{state::DlModuleSpec, syscalls::*};

// TODO: add journal events for dl-related syscalls
#[instrument(level = "trace", skip_all, fields(path = field::Empty, flags), ret)]
pub fn dlopen<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
    flags: DlFlags,
    err_buf: WasmPtr<u8, M>,
    err_buf_len: M::Offset,
    ld_library_path: WasmPtr<u8, M>,
    ld_library_path_len: M::Offset,
    out_handle: WasmPtr<DlHandle, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let (env, mut store) = ctx.data_and_store_mut();
    let memory = unsafe { env.memory_view(&store) };

    let env_inner = unsafe { env.inner() };
    let Some(linker) = env_inner.linker() else {
        wasi_dl_err!(
            "The current instance is not a dynamically-linked instance",
            memory,
            err_buf,
            err_buf_len
        );
    };

    if path.is_null() {
        // A null file name symbolizes the main module, which has a static handle
        wasi_try_mem_ok!(out_handle.write(&memory, crate::state::MAIN_MODULE_HANDLE.into()));
        return Ok(Errno::Success);
    }

    let path = unsafe { get_input_str_ok!(&memory, path, path_len) };
    let ld_library_path =
        unsafe { get_input_str_ok!(&memory, ld_library_path, ld_library_path_len) };
    let ld_library_path = ld_library_path
        .split(':')
        .map(Path::new)
        .collect::<Vec<_>>();
    Span::current().record("path", path.as_str());

    let linker = linker.clone();

    let location = DlModuleSpec::FileSystem {
        module_spec: Path::new(&path),
        ld_library_path: ld_library_path.as_slice(),
    };
    let module_handle = linker.load_module(location, &mut ctx);

    // Reborrow to keep rust happy
    let (env, mut store) = ctx.data_and_store_mut();
    let memory = unsafe { env.memory_view(&store) };

    let module_handle = wasi_try_dl!(
        module_handle,
        "failed to load module: {}",
        memory,
        err_buf,
        err_buf_len
    );

    wasi_try_mem_ok!(out_handle.write(&memory, module_handle.into()));

    Ok(Errno::Success)
}

pub(crate) fn write_dl_error<M: MemorySize>(
    mut err: &str,
    memory: &MemoryView,
    err_buf: WasmPtr<u8, M>,
    err_buf_len: u64,
) -> Result<(), MemoryAccessError> {
    let mut err_len = err.len();

    if err_len > err_buf_len as usize {
        err_len = err_buf_len as usize - 1;
        err = &err[..err_len];
    }

    let mut buf = vec![0; err_len + 1];
    buf[0..err_len].copy_from_slice(err.as_bytes());

    let Ok(err_len_offset) = M::Offset::try_from(err_len + 1) else {
        panic!("Failed to convert size to offset")
    };
    let mut err_buf = err_buf.slice(memory, err_len_offset)?.access()?;
    err_buf.copy_from_slice(&buf[..]);

    Ok(())
}
