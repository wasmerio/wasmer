use wasmer::FromToNativeWasmType;

use super::*;
use crate::{
    VIRTUAL_ROOT_FD, WasiFs,
    os::task::{OwnedTaskStatus, TaskStatus},
    syscalls::*,
};

/// Replaces the current process with a new process, with proper `WasmPtr<WasmPtr<u8>>` string lists.
///
/// Successor to `proc_exec3`. `args` and `envs` are pointer arrays of null-terminated
/// strings with `args_len` / `envs_len` as element counts. A null `envs` pointer inherits
/// the current environment.
///
/// ## Return
///
/// If the execution fails, returns an error code. Does not return otherwise.
#[instrument(level = "trace", skip_all, fields(name = field::Empty, %args_len), ret)]
pub fn proc_exec4<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
    args: WasmPtr<WasmPtr<u8, M>, M>,
    args_len: M::Offset,
    envs: WasmPtr<WasmPtr<u8, M>, M>,
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

    let mut args = read_string_array(&memory, args, args_len).map_err(|err| {
        warn!("failed to execve as the args could not be read - {}", err);
        WasiError::Exit(err.into())
    })?;
    if args.is_empty() {
        // POSIX expects argv[0] to be present even if caller passed empty argv.
        args.push(name.clone());
    }

    let envs = if !envs.is_null() {
        let envs = read_string_array(&memory, envs, envs_len).map_err(|err| {
            warn!("failed to execve as the envs could not be read - {}", err);
            WasiError::Exit(err.into())
        })?;
        Some(parse_env_entries(envs).map_err(|err| WasiError::Exit(err.into()))?)
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

pub(crate) fn proc_exec4_impl<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: &mut String,
    args: Vec<String>,
    envs: Option<Vec<(String, String)>>,
    search_path: Bool,
    path: Option<&str>,
) -> Result<Errno, WasiError> {
    // Convert relative paths into absolute paths
    if search_path == Bool::True && !name.contains('/') {
        let path = if let Some(path) = path {
            path.split(':').collect::<Vec<_>>()
        } else {
            vec!["/usr/local/bin", "/bin", "/usr/bin"]
        };
        let (_, state, inodes) =
            unsafe { ctx.data().get_memory_and_wasi_state_and_inodes(&ctx, 0) };
        match find_executable_in_path(&state.fs, inodes, path.iter().map(AsRef::as_ref), name) {
            FindExecutableResult::Found(p) => *name = p,
            FindExecutableResult::AccessError => return Ok(Errno::Access),
            FindExecutableResult::NotFound => return Ok(Errno::Noent),
        }
    } else if name.starts_with("./") {
        *name = ctx.data().state.fs.relative_path_to_absolute(name.clone());
    }

    if search_path == Bool::False && !name.starts_with('/') {
        *name = ctx.data().state.fs.relative_path_to_absolute(name.clone());
    }

    trace!(%name);

    // ENAMETOOLONG: any path component > 255 bytes
    if name.split('/').any(|seg| seg.len() > 255) {
        return Ok(Errno::Nametoolong);
    }

    if name.contains('/') {
        let (_, state, inodes) =
            unsafe { ctx.data().get_memory_and_wasi_state_and_inodes(&ctx, 0) };
        match state
            .fs
            .get_inode_at_path(inodes, VIRTUAL_ROOT_FD, name, true)
        {
            Ok(_) => (),
            Err(Errno::Notdir) => return Ok(Errno::Notdir),
            Err(Errno::Noent) => return Ok(Errno::Noent),
            Err(Errno::Access) => return Ok(Errno::Access),
            Err(_) => (),
        }
    }

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

        // Record the stack offsets before we give up ownership of the wasi_env
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
                // We failed to spawn a new process - put the child env back
                child_env.swap_inner(ctx.data_mut());
                std::mem::swap(child_env.as_mut(), ctx.data_mut());

                // Put back the vfork we previously took from here
                ctx.data_mut().vfork = Some(vfork);
                Ok(e)
            }
            Ok(()) => {
                // We spawned a new process - put the parent env back
                ctx.data_mut().swap_inner(&mut vfork.env);
                std::mem::swap(ctx.data_mut(), &mut vfork.env);

                let Some(asyncify_info) = vfork.asyncify else {
                    // vfork without asyncify only forks the WasiEnv, which we have restored
                    // above. Restoring the control flow is done on the guest side.
                    // See `proc_fork_env()` for information about this.

                    return Ok(Errno::Success);
                };

                // Jump back to the vfork point and continue execution
                // note: fork does not return any values hence passing `()`
                let rewind_stack = asyncify_info.rewind_stack.freeze();
                let store_data = asyncify_info.store_data;
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
                block_on(bin_factory.spawn(name.clone(), env))
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
