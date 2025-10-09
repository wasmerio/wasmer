use wasmer::FromToNativeWasmType;

use super::*;
use crate::{
    VIRTUAL_ROOT_FD, WasiFs,
    os::task::{OwnedTaskStatus, TaskStatus},
    syscalls::*,
};

/// Replaces the current process with a new process
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
        // We should never get here as the process will be termined
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
    let args: Vec<_> = args
        .split(&['\n', '\r'])
        .map(|a| a.to_string())
        .filter(|a| !a.is_empty())
        .collect();

    let envs = if !envs.is_null() {
        let envs = envs.read_utf8_string(&memory, envs_len).map_err(|err| {
            warn!("failed to execve as the envs could not be read - {}", err);
            WasiError::Exit(Errno::Inval.into())
        })?;

        let envs = envs
            .split(&['\n', '\r'])
            .map(|a| a.to_string())
            .filter(|a| !a.is_empty());

        let mut vec = vec![];
        for env in envs {
            let (key, value) = wasi_try_ok!(env.split_once('=').ok_or(Errno::Inval));

            vec.push((key.to_string(), value.to_string()));
        }

        Some(vec)
    } else {
        None
    };

    // Convert relative paths into absolute paths
    if search_path == Bool::True && !name.contains('/') {
        let path_str;

        let path = if path.is_null() {
            vec!["/usr/local/bin", "/bin", "/usr/bin"]
        } else {
            path_str = path.read_utf8_string(&memory, path_len).map_err(|err| {
                warn!("failed to execve as the path could not be read - {}", err);
                WasiError::Exit(Errno::Inval.into())
            })?;
            path_str.split(':').collect()
        };
        let (_, state, inodes) =
            unsafe { ctx.data().get_memory_and_wasi_state_and_inodes(&ctx, 0) };
        match find_executable_in_path(&state.fs, inodes, path.iter().map(AsRef::as_ref), &name) {
            FindExecutableResult::Found(p) => name = p,
            FindExecutableResult::AccessError => return Ok(Errno::Access),
            FindExecutableResult::NotFound => return Ok(Errno::Noexec),
        }
    } else if name.starts_with("./") {
        name = ctx.data().state.fs.relative_path_to_absolute(name);
    }

    trace!(name);

    // Convert the preopen directories
    let preopen = ctx.data().state.preopen.clone();

    // Get the current working directory
    let (_, cur_dir) = {
        let (memory, state, inodes) =
            unsafe { ctx.data().get_memory_and_wasi_state_and_inodes(&ctx, 0) };
        match state.fs.get_current_dir(inodes, crate::VIRTUAL_ROOT_FD) {
            Ok(a) => a,
            Err(err) => {
                warn!("failed to create subprocess for fork - {}", err);
                return Err(WasiError::Exit(err.into()));
            }
        }
    };

    let new_store = ctx.data().runtime.new_store();

    // If we are in a vfork we need to first spawn a subprocess of this type
    // with the forked WasiEnv, then do a longjmp back to the vfork point.
    if let Some(mut vfork) = ctx.data_mut().vfork.take() {
        // Needed in case an error happens and we need to get back into the child process
        let mut child_env = Box::new(ctx.data().clone());

        // We will need the child pid later
        let child_pid = ctx.data().process.pid();

        tracing::debug!(
            %child_pid,
            vfork_pid = %vfork.env.process.pid(),
            "proc_exec in presence of vfork"
        );

        // Restore the WasiEnv to the point when we vforked
        let mut vfork_env = vfork.env.clone();
        vfork_env.swap_inner(ctx.data_mut());
        std::mem::swap(vfork_env.as_mut(), ctx.data_mut());
        let mut wasi_env = *vfork_env;
        wasi_env.owned_handles.push(vfork.handle.clone());
        _prepare_wasi(&mut wasi_env, Some(args), envs, None);

        // Recrod the stack offsets before we give up ownership of the wasi_env
        let stack_lower = wasi_env.layout.stack_lower;
        let stack_upper = wasi_env.layout.stack_upper;

        // Spawn a new process with this current execution environment
        let mut err_exit_code: ExitCode = Errno::Success.into();

        let spawn_result = {
            let bin_factory = Box::new(ctx.data().bin_factory.clone());
            let tasks = wasi_env.tasks().clone();

            let mut config = Some(wasi_env);

            match bin_factory.try_built_in(name.clone(), Some(&ctx), &mut config) {
                Ok(a) => Ok(()),
                Err(err) => {
                    if !err.is_not_found() {
                        error!("builtin failed - {}", err);
                    }

                    let env = config.take().unwrap();

                    let name_inner = name.clone();
                    __asyncify_light(ctx.data(), None, async {
                        let ret = bin_factory.spawn(name_inner, env).await;
                        match ret {
                            Ok(ret) => {
                                trace!(%child_pid, "spawned sub-process");
                                Ok(())
                            }
                            Err(err) => {
                                err_exit_code = conv_spawn_err_to_exit_code(&err);

                                debug!(%child_pid, "process failed with (err={})", err_exit_code);

                                Err(Errno::Noexec)
                            }
                        }
                    })
                    .unwrap()
                }
            }
        };

        match spawn_result {
            Err(e) => {
                // We failed to spawn a new process - put the vfork back
                child_env.swap_inner(ctx.data_mut());
                std::mem::swap(child_env.as_mut(), ctx.data_mut());

                ctx.data_mut().vfork = Some(vfork);
                return Ok(e);
            }
            Ok(()) => {
                // Jump back to the vfork point and current on execution
                // note: fork does not return any values hence passing `()`
                let rewind_stack = vfork.rewind_stack.freeze();
                let store_data = vfork.store_data;
                unwind::<M, _>(ctx, move |mut ctx, _, _| {
                    // Rewind the stack
                    match rewind::<M, _>(
                        ctx,
                        None,
                        rewind_stack,
                        store_data,
                        ForkResult {
                            pid: child_pid.raw() as Pid,
                            ret: Errno::Success,
                        },
                    ) {
                        Errno::Success => OnCalledAction::InvokeAgain,
                        err => {
                            warn!("fork failed - could not rewind the stack - errno={}", err);
                            OnCalledAction::Trap(Box::new(WasiError::Exit(err.into())))
                        }
                    }
                })?;
                Ok(Errno::Success)
            }
        }
    }
    // Otherwise we need to unwind the stack to get out of the current executing
    // callstack, steal the memory/WasiEnv and switch it over to a new thread
    // on the new module
    else {
        // Prepare the environment
        let mut wasi_env = ctx.data().clone();
        _prepare_wasi(&mut wasi_env, Some(args), envs, None);

        // Get a reference to the runtime
        let bin_factory = ctx.data().bin_factory.clone();
        let tasks = wasi_env.tasks().clone();

        // Create the process and drop the context
        let bin_factory = Box::new(ctx.data().bin_factory.clone());

        let mut builder = Some(wasi_env);

        let process = match bin_factory.try_built_in(name.clone(), Some(&ctx), &mut builder) {
            Ok(a) => Ok(a),
            Err(err) => {
                if !err.is_not_found() {
                    error!("builtin failed - {}", err);
                }

                let env = builder.take().unwrap();

                // Spawn a new process with this current execution environment
                InlineWaker::block_on(bin_factory.spawn(name, env))
            }
        };

        match process {
            Ok(mut process) => {
                // If we support deep sleeping then we switch to deep sleep mode
                let env = ctx.data();

                let thread = env.thread.clone();

                // The poller will wait for the process to actually finish
                let res = __asyncify_with_deep_sleep::<M, _, _>(ctx, async move {
                    process
                        .wait_finished()
                        .await
                        .unwrap_or_else(|_| Errno::Child.into())
                        .to_native()
                })?;
                match res {
                    AsyncifyAction::Finish(mut ctx, result) => {
                        // When we arrive here the process should already be terminated
                        let exit_code = ExitCode::from_native(result);
                        ctx.data().process.terminate(exit_code);
                        WasiEnv::process_signals_and_exit(&mut ctx)?;
                        Err(WasiError::Exit(Errno::Unknown.into()))
                    }
                    AsyncifyAction::Unwind => Ok(Errno::Success),
                }
            }
            Err(err) => {
                warn!(
                    "failed to execve as the process could not be spawned (fork)[0] - {}",
                    err
                );
                Ok(Errno::Noexec)
            }
        }
    }
}

pub(crate) enum FindExecutableResult {
    Found(String),
    AccessError,
    NotFound,
}

pub(crate) fn find_executable_in_path<'a>(
    fs: &WasiFs,
    inodes: &WasiInodes,
    path: impl IntoIterator<Item = &'a str>,
    file_name: &str,
) -> FindExecutableResult {
    let mut encountered_eaccess = false;
    for p in path {
        let full_path = format!("{}/{}", p.trim_end_matches('/'), file_name);
        match fs.get_inode_at_path(inodes, VIRTUAL_ROOT_FD, &full_path, true) {
            Ok(_) => return FindExecutableResult::Found(full_path),
            Err(Errno::Access) => encountered_eaccess = true,
            Err(_) => (),
        }
    }

    if encountered_eaccess {
        FindExecutableResult::AccessError
    } else {
        FindExecutableResult::NotFound
    }
}
