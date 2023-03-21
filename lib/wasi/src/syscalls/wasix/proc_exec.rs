use super::*;
use crate::{
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
///
/// ## Return
///
/// Returns a bus process id that can be used to invoke calls
#[instrument(level = "debug", skip_all, fields(name = field::Empty, args_len), ret, err)]
pub fn proc_exec<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
    args: WasmPtr<u8, M>,
    args_len: M::Offset,
) -> Result<(), WasiError> {
    WasiEnv::process_signals_and_exit(&mut ctx)?;

    // If we were just restored the stack then we were woken after a deep sleep
    if handle_rewind::<M>(&mut ctx) {
        // We should never get here as the process will be termined
        // in the `WasiEnv::process_signals_and_exit()` call
        return Err(WasiError::Exit(Errno::Unknown.into()));
    }

    let memory = ctx.data().memory_view(&ctx);
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

    // Convert relative paths into absolute paths
    if name.starts_with("./") {
        name = ctx.data().state.fs.relative_path_to_absolute(name);
    }
    trace!(name);

    // Convert the preopen directories
    let preopen = ctx.data().state.preopen.clone();

    // Get the current working directory
    let (_, cur_dir) = {
        let (memory, state, inodes) = ctx.data().get_memory_and_wasi_state_and_inodes(&ctx, 0);
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
        // We will need the child pid later
        let child_process = ctx.data().process.clone();
        let child_pid = child_process.pid();
        let child_finished = child_process.finished;

        // Restore the WasiEnv to the point when we vforked
        std::mem::swap(&mut vfork.env.inner, &mut ctx.data_mut().inner);
        std::mem::swap(vfork.env.as_mut(), ctx.data_mut());
        let mut wasi_env = *vfork.env;
        wasi_env.owned_handles.push(vfork.handle);
        _prepare_wasi(&mut wasi_env, Some(args));

        // Recrod the stack offsets before we give up ownership of the wasi_env
        let stack_lower = wasi_env.layout.stack_lower;
        let stack_upper = wasi_env.layout.stack_upper;

        // Spawn a new process with this current execution environment
        let mut err_exit_code: ExitCode = Errno::Success.into();

        {
            let bin_factory = Box::new(ctx.data().bin_factory.clone());
            let tasks = wasi_env.tasks().clone();

            let mut new_store = Some(new_store);
            let mut config = Some(wasi_env);

            match bin_factory.try_built_in(name.clone(), Some(&ctx), &mut new_store, &mut config) {
                Ok(a) => {}
                Err(err) => {
                    if err != VirtualBusError::NotFound {
                        error!("builtin failed - {}", err);
                    }

                    let new_store = new_store.take().unwrap();
                    let env = config.take().unwrap();

                    tasks.block_on(async {
                        let name_inner = name.clone();
                        let ret = bin_factory.spawn(
                                name_inner,
                                new_store,
                                env,
                            )
                            .await;
                        match ret {
                            Ok(ret) => {
                                trace!(%child_pid, "spawned sub-process");
                            },
                            Err(err) => {
                                err_exit_code = conv_bus_err_to_exit_code(err);

                                debug!(%child_pid, "process failed with (err={})", err_exit_code);
                                child_finished.set_finished(Ok(err_exit_code));

                                warn!(
                                    "failed to execve as the process could not be spawned (vfork) - {}",
                                    err
                                );
                                let _ = stderr_write(
                                    &ctx,
                                    format!("wasm execute failed [{}] - {}\n", name.as_str(), err)
                                        .as_bytes(),
                                ).await;
                            }
                        }
                    })
                }
            }
        };

        let mut memory_stack = vfork.memory_stack;
        let rewind_stack = vfork.rewind_stack;
        let store_data = vfork.store_data;

        // If the return value offset is within the memory stack then we need
        // to update it here rather than in the real memory
        let val_bytes = child_pid.raw().to_ne_bytes();
        let pid_offset: u64 = vfork.pid_offset;
        if pid_offset >= stack_lower && (pid_offset + val_bytes.len() as u64) <= stack_upper {
            // Make sure its within the "active" part of the memory stack
            let offset = stack_upper - pid_offset;
            if (offset as usize + val_bytes.len()) > memory_stack.len() {
                warn!(
                    "vfork failed - the return value (pid) is outside of the active part of the memory stack ({} vs {})",
                    offset,
                    memory_stack.len()
                );
            } else {
                // Update the memory stack with the new PID
                let pstart = memory_stack.len() - offset as usize;
                let pend = pstart + val_bytes.len();
                let pbytes = &mut memory_stack[pstart..pend];
                pbytes.clone_from_slice(&val_bytes);
            }
        } else {
            warn!(
                "vfork failed - the return value (pid) is not being returned on the stack - which is not supported",
            );
        }

        // Jump back to the vfork point and current on execution
        unwind::<M, _>(ctx, move |mut ctx, _, _| {
            // Rewind the stack
            match rewind::<M>(
                ctx,
                memory_stack.freeze(),
                rewind_stack.freeze(),
                store_data,
            ) {
                Errno::Success => OnCalledAction::InvokeAgain,
                err => {
                    warn!("fork failed - could not rewind the stack - errno={}", err);
                    OnCalledAction::Trap(Box::new(WasiError::Exit(err.into())))
                }
            }
        })?;
        Ok(())
    }
    // Otherwise we need to unwind the stack to get out of the current executing
    // callstack, steal the memory/WasiEnv and switch it over to a new thread
    // on the new module
    else {
        // Prepare the environment
        let mut wasi_env = ctx.data_mut().duplicate();
        _prepare_wasi(&mut wasi_env, Some(args));

        // Get a reference to the runtime
        let bin_factory = ctx.data().bin_factory.clone();
        let tasks = wasi_env.tasks().clone();

        // Create the process and drop the context
        let bin_factory = Box::new(ctx.data().bin_factory.clone());

        let mut new_store = Some(new_store);
        let mut builder = Some(wasi_env);

        let process = match bin_factory.try_built_in(
            name.clone(),
            Some(&ctx),
            &mut new_store,
            &mut builder,
        ) {
            Ok(a) => Ok(Ok(a)),
            Err(err) => {
                if err != VirtualBusError::NotFound {
                    error!("builtin failed - {}", err);
                }

                let new_store = new_store.take().unwrap();
                let env = builder.take().unwrap();

                // Spawn a new process with this current execution environment
                //let pid = wasi_env.process.pid();
                let (tx, rx) = std::sync::mpsc::channel();
                tasks.block_on(Box::pin(async move {
                    let ret = bin_factory.spawn(name, new_store, env).await;
                    tx.send(ret);
                }));
                rx.recv()
            }
        };

        match process {
            Ok(Ok(mut process)) => {
                // If we support deep sleeping then we switch to deep sleep mode
                let env = ctx.data();

                // The poller will wait for the process to actually finish
                let res = __asyncify_with_deep_sleep_ext::<M, _, _, _>(
                    ctx,
                    None,
                    async move {
                        process
                            .wait_finished()
                            .await
                            .unwrap_or_else(|_| Errno::Child.into())
                    },
                    |exit_code, env, _| {
                        env.thread.set_status_finished(Ok(exit_code));
                    },
                )?;
                return match res {
                    AsyncifyAction::Finish(mut ctx) => {
                        // When we arrive here the process should already be terminated
                        WasiEnv::process_signals_and_exit(&mut ctx)?;
                        Err(WasiError::Exit(Errno::Unknown.into()))
                    }
                    AsyncifyAction::Unwind => Ok(()),
                };
            }
            Ok(Err(err)) => {
                warn!(
                    "failed to execve as the process could not be spawned (fork)[0] - {}",
                    err
                );
                Err(WasiError::Exit(Errno::Noexec.into()))
            }
            Err(err) => {
                warn!(
                    "failed to execve as the process could not be spawned (fork)[1] - {}",
                    err
                );
                Err(WasiError::Exit(Errno::Noexec.into()))
            }
        }
    }
}
