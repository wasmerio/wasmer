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
pub fn proc_exec<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
    args: WasmPtr<u8, M>,
    args_len: M::Offset,
) -> Result<(), WasiError> {
    let memory = ctx.data().memory_view(&ctx);
    let mut name = name.read_utf8_string(&memory, name_len).map_err(|err| {
        warn!("failed to execve as the name could not be read - {}", err);
        WasiError::Exit(Errno::Fault as ExitCode)
    })?;
    trace!(
        "wasi[{}:{}]::proc_exec (name={})",
        ctx.data().pid(),
        ctx.data().tid(),
        name
    );

    let args = args.read_utf8_string(&memory, args_len).map_err(|err| {
        warn!("failed to execve as the args could not be read - {}", err);
        WasiError::Exit(Errno::Fault as ExitCode)
    })?;
    let args: Vec<_> = args
        .split(&['\n', '\r'])
        .map(|a| a.to_string())
        .filter(|a| !a.is_empty())
        .collect();

    // Convert relative paths into absolute paths
    if name.starts_with("./") {
        name = ctx.data().state.fs.relative_path_to_absolute(name);
        trace!(
            "wasi[{}:{}]::rel_to_abs (name={}))",
            ctx.data().pid(),
            ctx.data().tid(),
            name
        );
    }

    // Convert the preopen directories
    let preopen = ctx.data().state.preopen.clone();

    // Get the current working directory
    let (_, cur_dir) = {
        let (memory, state, inodes) = ctx.data().get_memory_and_wasi_state_and_inodes(&ctx, 0);
        match state.fs.get_current_dir(inodes, crate::VIRTUAL_ROOT_FD) {
            Ok(a) => a,
            Err(err) => {
                warn!("failed to create subprocess for fork - {}", err);
                return Err(WasiError::Exit(Errno::Fault as ExitCode));
            }
        }
    };

    let new_store = ctx.data().runtime.new_store();

    // If we are in a vfork we need to first spawn a subprocess of this type
    // with the forked WasiEnv, then do a longjmp back to the vfork point.
    if let Some(mut vfork) = ctx.data_mut().vfork.take() {
        // We will need the child pid later
        let child_pid = ctx.data().process.pid();

        // Restore the WasiEnv to the point when we vforked
        std::mem::swap(&mut vfork.env.inner, &mut ctx.data_mut().inner);
        std::mem::swap(vfork.env.as_mut(), ctx.data_mut());
        let mut wasi_env = *vfork.env;
        wasi_env.owned_handles.push(vfork.handle);
        _prepare_wasi(&mut wasi_env, Some(args));

        // Recrod the stack offsets before we give up ownership of the wasi_env
        let stack_base = wasi_env.stack_base;
        let stack_start = wasi_env.stack_start;

        // Spawn a new process with this current execution environment
        let mut err_exit_code = -2i32 as u32;

        let mut process = {
            let bin_factory = Box::new(ctx.data().bin_factory.clone());
            let tasks = wasi_env.tasks().clone();

            let mut new_store = Some(new_store);
            let mut config = Some(wasi_env);

            match bin_factory.try_built_in(name.clone(), Some(&ctx), &mut new_store, &mut config) {
                Ok(a) => Some(a),
                Err(err) => {
                    if err != VirtualBusError::NotFound {
                        error!(
                            "wasi[{}:{}]::proc_exec - builtin failed - {}",
                            ctx.data().pid(),
                            ctx.data().tid(),
                            err
                        );
                    }

                    let new_store = new_store.take().unwrap();
                    let env = config.take().unwrap();

                    let (process, c) = tasks.block_on(async move {
                        let name_inner = name.clone();
                        let ret = bin_factory.spawn(
                                name_inner,
                                new_store,
                                env,
                            )
                            .await;
                        match ret {
                            Ok(ret) => (Some(ret), ctx),
                            Err(err) => {
                                err_exit_code = conv_bus_err_to_exit_code(err);
                                warn!(
                                    "failed to execve as the process could not be spawned (vfork) - {}",
                                    err
                                );
                                let _ = stderr_write(
                                    &ctx,
                                    format!("wasm execute failed [{}] - {}\n", name.as_str(), err)
                                        .as_bytes(),
                                ).await;
                                (None, ctx)
                            }
                        }
                    });
                    ctx = c;
                    process
                }
            }
        };

        // If no process was created then we create a dummy one so that an
        // exit code can be processed
        let process = match process {
            Some(a) => {
                trace!(
                    "wasi[{}:{}]::spawned sub-process (pid={})",
                    ctx.data().pid(),
                    ctx.data().tid(),
                    child_pid.raw()
                );
                a
            }
            None => {
                debug!(
                    "wasi[{}:{}]::process failed with (err={})",
                    ctx.data().pid(),
                    ctx.data().tid(),
                    err_exit_code
                );
                OwnedTaskStatus::new(TaskStatus::Finished(Ok(err_exit_code))).handle()
            }
        };

        // Add the process to the environment state
        {
            let mut inner = ctx.data().process.write();
            inner.bus_processes.insert(child_pid, process);
        }

        let mut memory_stack = vfork.memory_stack;
        let rewind_stack = vfork.rewind_stack;
        let store_data = vfork.store_data;

        // If the return value offset is within the memory stack then we need
        // to update it here rather than in the real memory
        let pid_offset: u64 = vfork.pid_offset;
        if pid_offset >= stack_start && pid_offset < stack_base {
            // Make sure its within the "active" part of the memory stack
            let offset = stack_base - pid_offset;
            if offset as usize > memory_stack.len() {
                warn!("vfork failed - the return value (pid) is outside of the active part of the memory stack ({} vs {})", offset, memory_stack.len());
            } else {
                // Update the memory stack with the new PID
                let val_bytes = child_pid.raw().to_ne_bytes();
                let pstart = memory_stack.len() - offset as usize;
                let pend = pstart + val_bytes.len();
                let pbytes = &mut memory_stack[pstart..pend];
                pbytes.clone_from_slice(&val_bytes);
            }
        } else {
            warn!("vfork failed - the return value (pid) is not being returned on the stack - which is not supported");
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
                    OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Fault as u32)))
                }
            }
        })?;
        return Ok(());
    }
    // Otherwise we need to unwind the stack to get out of the current executing
    // callstack, steal the memory/WasiEnv and switch it over to a new thread
    // on the new module
    else {
        // We need to unwind out of this process and launch a new process in its place
        unwind::<M, _>(ctx, move |mut ctx, _, _| {
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
                        error!(
                            "wasi[{}:{}]::proc_exec - builtin failed - {}",
                            ctx.data().pid(),
                            ctx.data().tid(),
                            err
                        );
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
                    // Wait for the sub-process to exit itself - then we will exit
                    let (tx, rx) = std::sync::mpsc::channel();
                    let tasks_inner = tasks.clone();
                    tasks.block_on(Box::pin(async move {
                        let code = process.wait_finished().await.unwrap_or(Errno::Child as u32);
                        tx.send(code);
                    }));
                    let exit_code = rx.recv().unwrap();
                    OnCalledAction::Trap(Box::new(WasiError::Exit(exit_code as ExitCode)))
                }
                Ok(Err(err)) => {
                    warn!(
                        "failed to execve as the process could not be spawned (fork)[0] - {}",
                        err
                    );
                    OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Noexec as ExitCode)))
                }
                Err(err) => {
                    warn!(
                        "failed to execve as the process could not be spawned (fork)[1] - {}",
                        err
                    );
                    OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Noexec as ExitCode)))
                }
            }
        })?;
    }

    // Success
    Ok(())
}
