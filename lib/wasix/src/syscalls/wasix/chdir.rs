use super::*;
use crate::syscalls::*;

/// ### `chdir()`
/// Sets the current working directory
#[instrument(level = "trace", skip_all, fields(name = field::Empty), ret)]
pub fn chdir<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let (memory, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let path = unsafe { get_input_str_ok!(&memory, path, path_len) };
    Span::current().record("path", path.as_str());

    wasi_try_ok!(chdir_internal(&mut ctx, &path));
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_chdir(&mut ctx, path).map_err(|err| {
            tracing::error!("failed to chdir event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub fn chdir_internal(ctx: &mut FunctionEnvMut<'_, WasiEnv>, path: &str) -> Result<(), Errno> {
    let env = ctx.data();
    let (memory, mut state) = unsafe { env.get_memory_and_wasi_state(ctx, 0) };

    // Check if the directory exists
    if state.fs.root_fs.read_dir(Path::new(path)).is_err() {
        return Err(Errno::Noent);
    }

    state.fs.set_current_dir(path);
    Ok(())
}
