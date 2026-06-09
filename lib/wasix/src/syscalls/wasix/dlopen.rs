use super::*;
use crate::{state::DlModuleSpec, syscalls::*};

// TODO: add journal events for dl-related syscalls
#[instrument(level = "trace", skip_all, fields(path = field::Empty, ld_library_path = field::Empty, flags), ret)]
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
    Span::current().record("path", path.as_str());

    let ld_library_path =
        unsafe { get_input_str_ok!(&memory, ld_library_path, ld_library_path_len) };
    Span::current().record("ld_library_path", ld_library_path.as_str());
    let ld_library_path = ld_library_path
        .split(':')
        .map(Path::new)
        .collect::<Vec<_>>();

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
    let err_buf_len = err_buf_len as usize;

    // The message is always written with a trailing NUL, so a zero-length
    // buffer leaves no room for anything.
    if err_buf_len == 0 {
        return Ok(());
    }

    // Reserve one byte for the trailing NUL.
    let max_err_len = err_buf_len - 1;
    let mut err_len = err.len();

    if err_len > max_err_len {
        err_len = max_err_len;
        err = &err[..err_len];
    }

    let Ok(err_len_offset) = M::Offset::try_from(err_len + 1) else {
        panic!("Failed to convert size to offset")
    };
    let mut err_buf = err_buf.slice(memory, err_len_offset)?.access()?;
    let dst = err_buf.as_mut();
    dst[..err_len].copy_from_slice(err.as_bytes());
    dst[err_len] = 0;

    Ok(())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::write_dl_error;
    use wasmer::{Memory, Memory32, MemoryType, Store, WasmPtr};

    #[test]
    fn write_dl_error_zero_len_buffer_writes_nothing() {
        let mut store = Store::default();
        let memory = Memory::new(&mut store, MemoryType::new(1, None, false)).unwrap();
        let view = memory.view(&store);

        // A guest-supplied err_buf_len of 0 leaves no room even for the NUL
        // terminator. The old accounting computed `0 - 1`, underflowing usize.
        let err_buf = WasmPtr::<u8, Memory32>::new(0);
        let res = write_dl_error::<Memory32>("failed to load module", &view, err_buf, 0);
        assert!(res.is_ok());
        assert_eq!(view.read_u8(0).unwrap(), 0);
    }

    #[test]
    fn write_dl_error_reserves_room_for_nul() {
        let mut store = Store::default();
        let memory = Memory::new(&mut store, MemoryType::new(1, None, false)).unwrap();
        let view = memory.view(&store);

        // err_buf_len equal to the message length must still keep the NUL
        // inside the buffer rather than spilling one byte past it.
        let msg = "abcd";
        let err_buf = WasmPtr::<u8, Memory32>::new(0);
        write_dl_error::<Memory32>(msg, &view, err_buf, msg.len() as u64).unwrap();

        let mut got = [1u8; 5];
        view.read(0, &mut got).unwrap();
        assert_eq!(&got[..4], b"abc\0");
        assert_eq!(got[4], 0);
    }
}
