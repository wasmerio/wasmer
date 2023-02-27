use super::*;
use crate::{os::task::OwnedTaskStatus, syscalls::*};

#[cfg(feature = "sys")]
use wasmer::vm::VMMemory;
#[cfg(feature = "js")]
use wasmer::VMMemory;

/// ### `proc_fork()`
/// Forks the current process into a new subprocess. If the function
/// returns a zero then its the new subprocess. If it returns a positive
/// number then its the current process and the $pid represents the child.
pub fn proc_fork<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    mut copy_memory: Bool,
    pid_ptr: WasmPtr<Pid, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    // If we were just restored then we need to return the value instead
    let fork_op = if copy_memory == Bool::True {
        "fork"
    } else {
        "vfork"
    };
    if handle_rewind::<M>(&mut ctx) {
        let env = ctx.data();
        let memory = env.memory_view(&ctx);
        let ret_pid = wasi_try_mem_ok!(pid_ptr.read(&memory));
        if ret_pid == 0 {
            trace!(
                "wasi[{}:{}]::proc_{} - entering child",
                ctx.data().pid(),
                ctx.data().tid(),
                fork_op
            );
        } else {
            trace!(
                "wasi[{}:{}]::proc_{} - entering parent(child={})",
                ctx.data().pid(),
                ctx.data().tid(),
                fork_op,
                ret_pid
            );
        }
        return Ok(Errno::Success);
    }
    trace!(
        "wasi[{}:{}]::proc_{} - capturing",
        ctx.data().pid(),
        ctx.data().tid(),
        fork_op
    );

    // Fork the environment which will copy all the open file handlers
    // and associate a new context but otherwise shares things like the
    // file system interface. The handle to the forked process is stored
    // in the parent process context
    let (mut child_env, mut child_handle) = match ctx.data().fork() {
        Ok(p) => p,
        Err(err) => {
            debug!(
                pid=%ctx.data().pid(),
                tid=%ctx.data().tid(),
                "could not fork process: {err}"
            );
            // TODO: evaluate the appropriate error code, document it in the spec.
            return Ok(Errno::Perm);
        }
    };
    let child_pid = child_env.process.pid();

    // We write a zero to the PID before we capture the stack
    // so that this is what will be returned to the child
    {
        let mut children = ctx.data().process.children.write().unwrap();
        children.push(child_pid);
    }
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    wasi_try_mem_ok!(pid_ptr.write(&memory, 0));

    // Pass some offsets to the unwind function
    let pid_offset = pid_ptr.offset();

    // If we are not copying the memory then we act like a `vfork`
    // instead which will pretend to be the new process for a period
    // of time until `proc_exec` is called at which point the fork
    // actually occurs
    if copy_memory == Bool::False {
        // Perform the unwind action
        let pid_offset: u64 = pid_offset.into();
        return unwind::<M, _>(ctx, move |mut ctx, mut memory_stack, rewind_stack| {
            // Grab all the globals and serialize them
            let store_data = crate::utils::store::capture_snapshot(&mut ctx.as_store_mut())
                .serialize()
                .unwrap();
            let store_data = Bytes::from(store_data);

            // We first fork the environment and replace the current environment
            // so that the process can continue to prepare for the real fork as
            // if it had actually forked
            std::mem::swap(&mut ctx.data_mut().inner, &mut child_env.inner);
            std::mem::swap(ctx.data_mut(), &mut child_env);
            ctx.data_mut().vfork.replace(WasiVFork {
                rewind_stack: rewind_stack.clone(),
                memory_stack: memory_stack.clone(),
                store_data: store_data.clone(),
                env: Box::new(child_env),
                handle: child_handle,
                pid_offset,
            });

            // Carry on as if the fork had taken place (which basically means
            // it prevents to be the new process with the old one suspended)
            // Rewind the stack and carry on
            match rewind::<M>(
                ctx,
                memory_stack.freeze(),
                rewind_stack.freeze(),
                store_data,
            ) {
                Errno::Success => OnCalledAction::InvokeAgain,
                err => {
                    warn!(
                        "{} failed - could not rewind the stack - errno={}",
                        fork_op, err
                    );
                    OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Fault as u32)))
                }
            }
        });
    }

    // Create the thread that will back this forked process
    let state = env.state.clone();
    let bin_factory = env.bin_factory.clone();

    // Perform the unwind action
    unwind::<M, _>(ctx, move |mut ctx, mut memory_stack, rewind_stack| {
        // Grab all the globals and serialize them
        let store_data = crate::utils::store::capture_snapshot(&mut ctx.as_store_mut())
            .serialize()
            .unwrap();
        let store_data = Bytes::from(store_data);

        // Fork the memory and copy the module (compiled code)
        let env = ctx.data();
        let fork_memory: VMMemory = match env
            .memory()
            .try_clone(&ctx)
            .ok_or_else(|| {
                error!(
                    "wasi[{}:{}]::{} failed - the memory could not be cloned",
                    ctx.data().pid(),
                    ctx.data().tid(),
                    fork_op
                );
                MemoryError::Generic("the memory could not be cloned".to_string())
            })
            .and_then(|mut memory| memory.duplicate())
        {
            Ok(memory) => memory.into(),
            Err(err) => {
                warn!(
                    "wasi[{}:{}]::{} failed - could not fork the memory - {}",
                    ctx.data().pid(),
                    ctx.data().tid(),
                    fork_op,
                    err
                );
                return OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Fault as u32)));
            }
        };
        let fork_module = env.inner().instance.module().clone();

        let mut fork_store = ctx.data().runtime.new_store();

        // Now we use the environment and memory references
        let runtime = child_env.runtime.clone();
        let tasks = child_env.tasks().clone();
        let child_memory_stack = memory_stack.clone();
        let child_rewind_stack = rewind_stack.clone();

        // Spawn a new process with this current execution environment
        let signaler = Box::new(child_env.process.clone());
        let (exit_code_tx, exit_code_rx) = tokio::sync::mpsc::unbounded_channel();
        {
            let store_data = store_data.clone();
            let runtime = runtime.clone();
            let tasks = tasks.clone();
            let tasks_outer = tasks.clone();

            let mut store = fork_store;
            let module = fork_module;

            let task = move || {
                // Create the WasiFunctionEnv
                let pid = child_env.pid();
                let tid = child_env.tid();
                child_env.runtime = runtime.clone();
                let mut ctx = WasiFunctionEnv::new(&mut store, child_env);
                // fork_store, fork_module,

                let spawn_type = SpawnType::NewThread(fork_memory);

                let memory = match tasks.build_memory(spawn_type) {
                    Ok(m) => m,
                    Err(err) => {
                        error!(
                            "wasi[{}:{}]::{} failed - could not build instance memory - {}",
                            pid, tid, fork_op, err
                        );
                        ctx.data(&store).cleanup(Some(Errno::Noexec as ExitCode));
                        return;
                    }
                };

                // Let's instantiate the module with the imports.
                let (mut import_object, init) =
                    import_object_for_all_wasi_versions(&module, &mut store, &ctx.env);
                let memory = if let Some(memory) = memory {
                    let memory = Memory::new_from_existing(&mut store, memory);
                    import_object.define("env", "memory", memory.clone());
                    memory
                } else {
                    error!(
                        "wasi[{}:{}]::wasm instantiate failed - no memory supplied",
                        pid, tid
                    );
                    return;
                };
                let instance = match Instance::new(&mut store, &module, &import_object) {
                    Ok(a) => a,
                    Err(err) => {
                        error!("wasi[{}:{}]::wasm instantiate error ({})", pid, tid, err);
                        return;
                    }
                };

                init(&instance, &store).unwrap();

                // Set the current thread ID
                ctx.data_mut(&mut store).inner =
                    Some(WasiInstanceHandles::new(memory, &store, instance));

                // Rewind the stack and carry on
                {
                    trace!(
                        "wasi[{}:{}]::{}: rewinding child",
                        ctx.data(&store).pid(),
                        ctx.data(&store).tid(),
                        fork_op
                    );
                    let ctx = ctx.env.clone().into_mut(&mut store);
                    match rewind::<M>(
                        ctx,
                        child_memory_stack.freeze(),
                        child_rewind_stack.freeze(),
                        store_data.clone(),
                    ) {
                        Errno::Success => OnCalledAction::InvokeAgain,
                        err => {
                            warn!("wasi[{}:{}]::wasm rewind failed - could not rewind the stack - errno={}", pid, tid, err);
                            return;
                        }
                    };
                }

                // Invoke the start function
                let mut ret = Errno::Success;
                if ctx.data(&store).thread.is_main() {
                    trace!(
                        "wasi[{}:{}]::{}: re-invoking main",
                        ctx.data(&store).pid(),
                        ctx.data(&store).tid(),
                        fork_op
                    );
                    let start = ctx.data(&store).inner().start.clone().unwrap();
                    start.call(&mut store);
                } else {
                    trace!(
                        "wasi[{}:{}]::{}: re-invoking thread_spawn",
                        ctx.data(&store).pid(),
                        ctx.data(&store).tid(),
                        fork_op
                    );
                    let start = ctx.data(&store).inner().thread_spawn.clone().unwrap();
                    start.call(&mut store, 0, 0);
                }

                // Clean up the environment
                ctx.cleanup((&mut store), Some(ret as ExitCode));

                // Send the result
                let _ = exit_code_tx.send(ret as u32);
                drop(exit_code_tx);
                drop(child_handle);
            };

            // TODO: handle this better - required because of Module not being Send.
            #[cfg(feature = "js")]
            let task = {
                struct UnsafeWrapper {
                    inner: Box<dyn FnOnce() + 'static>,
                }

                unsafe impl Send for UnsafeWrapper {}

                let inner = UnsafeWrapper {
                    inner: Box::new(task),
                };

                move || {
                    (inner.inner)();
                }
            };

            tasks_outer
                .task_wasm(Box::new(task))
                .map_err(|err| {
                    warn!(
                        "wasi[{}:{}]::failed to fork as the process could not be spawned - {}",
                        ctx.data().pid(),
                        ctx.data().tid(),
                        err
                    );
                    err
                })
                .ok()
        };

        // Add the process to the environment state

        let process = OwnedTaskStatus::default();

        {
            trace!(
                "wasi[{}:{}]::spawned sub-process (pid={})",
                ctx.data().pid(),
                ctx.data().tid(),
                child_pid.raw()
            );
            let mut inner = ctx.data().process.write();
            inner.bus_processes.insert(child_pid, process.handle());
        }

        // If the return value offset is within the memory stack then we need
        // to update it here rather than in the real memory
        let pid_offset: u64 = pid_offset.into();
        if pid_offset >= env.stack_start && pid_offset < env.stack_base {
            // Make sure its within the "active" part of the memory stack
            let offset = env.stack_base - pid_offset;
            if offset as usize > memory_stack.len() {
                warn!("{} failed - the return value (pid) is outside of the active part of the memory stack ({} vs {})", fork_op, offset, memory_stack.len());
                return OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Fault as u32)));
            }

            // Update the memory stack with the new PID
            let val_bytes = child_pid.raw().to_ne_bytes();
            let pstart = memory_stack.len() - offset as usize;
            let pend = pstart + val_bytes.len();
            let pbytes = &mut memory_stack[pstart..pend];
            pbytes.clone_from_slice(&val_bytes);
        } else {
            warn!("{} failed - the return value (pid) is not being returned on the stack - which is not supported", fork_op);
            return OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Fault as u32)));
        }

        // Rewind the stack and carry on
        match rewind::<M>(
            ctx,
            memory_stack.freeze(),
            rewind_stack.freeze(),
            store_data,
        ) {
            Errno::Success => OnCalledAction::InvokeAgain,
            err => {
                warn!(
                    "{} failed - could not rewind the stack - errno={}",
                    fork_op, err
                );
                OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Fault as u32)))
            }
        }
    })
}
