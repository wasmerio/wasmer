use wasmer::FromToNativeWasmType;

use super::*;
use crate::syscalls::*;

/// Replaces the current process with a new process.
///
/// Legacy delimiter-based API: `args` and `envs` are single strings with entries
/// separated by line feeds. Prefer `proc_exec4` for proper string lists.
///
/// ## Parameters
///
/// * `name` - Name of the process to be spawned
/// * `args` - List of the arguments to pass the process
///   (entries are separated by line feeds)
/// * `envs` - List of the environment variables to pass process
///
/// ## Return
///
/// If the execution fails, returns an error code. Does not return otherwise.
#[instrument(level = "trace", skip_all, fields(name = field::Empty, %args_len), ret)]
pub fn proc_exec3<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
    args: WasmPtr<u8, M>,
    args_len: M::Offset,
    envs: WasmPtr<u8, M>,
    envs_len: M::Offset,
    search_path: Bool,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    // If we were just restored the stack then we were woken after a deep sleep
    if let Some(exit_code) = unsafe { handle_rewind::<M, i32>(&mut ctx) } {
        // We should never get here as the process will be terminated
        // in the `WasiEnv::do_pending_operations()` call
        let exit_code = ExitCode::from_native(exit_code);
        ctx.data().process.terminate(exit_code);
        return Err(WasiError::Exit(exit_code));
    }

    let memory = unsafe { ctx.data().memory_view(&ctx) };
    let mut name = name.read_utf8_string(&memory, name_len).map_err(|err| {
        warn!("failed to execve as the name could not be read - {}", err);
        WasiError::Exit(Errno::Inval.into())
    })?;
    Span::current().record("name", name.as_str());
    let args = args.read_utf8_string(&memory, args_len).map_err(|err| {
        warn!("failed to execve as the args could not be read - {}", err);
        WasiError::Exit(Errno::Inval.into())
    })?;
    let mut args = parse_delimited_exec_args(&args);
    if args.is_empty() {
        // POSIX expects argv[0] to be present even if caller passed empty argv.
        args.push(name.clone());
    }

    let envs = if !envs.is_null() {
        let envs = envs.read_utf8_string(&memory, envs_len).map_err(|err| {
            warn!("failed to execve as the envs could not be read - {}", err);
            WasiError::Exit(Errno::Inval.into())
        })?;
        Some(parse_delimited_env_list(&envs).map_err(|err| WasiError::Exit(err.into()))?)
    } else {
        None
    };

    let path = if path.is_null() {
        None
    } else {
        Some(path.read_utf8_string(&memory, path_len).map_err(|err| {
            warn!("failed to execve as the path could not be read - {}", err);
            WasiError::Exit(Errno::Inval.into())
        })?)
    };

    proc_exec4_impl::<M>(ctx, &mut name, args, envs, search_path, path.as_deref())
}
