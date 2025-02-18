use std::sync::Arc;

use crate::{
    os::task::{
        thread::{RewindResultType, WasiThreadRunGuard},
        TaskJoinHandle,
    },
    runtime::{
        task_manager::{
            TaskWasm, TaskWasmRecycle, TaskWasmRecycleProperties, TaskWasmRunProperties,
        },
        TaintReason,
    },
    syscalls::rewind_ext,
    RewindState, SpawnError, WasiError, WasiRuntimeError,
};
use tracing::*;
use virtual_mio::InlineWaker;
use wasmer::{Function, Memory32, Memory64, Module, RuntimeError, Store, Value};
use wasmer_wasix_types::wasi::Errno;

use super::BinaryPackage;
use crate::{Runtime, WasiEnv, WasiFunctionEnv};

#[tracing::instrument(level = "trace", skip_all, fields(%name, package_id=%binary.id))]
pub async fn spawn_exec(
    binary: BinaryPackage,
    name: &str,
    env: WasiEnv,
    runtime: &Arc<dyn Runtime + Send + Sync + 'static>,
) -> Result<TaskJoinHandle, SpawnError> {
    spawn_union_fs(&env, &binary).await?;

    let wasm = spawn_load_wasm(&binary, name).await?;

    let module = spawn_load_module(name, wasm, runtime).await?;

    // Free the space used by the binary, since we don't need it
    // any longer
    drop(binary);

    spawn_exec_module(module, env, runtime)
}

#[tracing::instrument(level = "trace", skip_all, fields(%name))]
pub async fn spawn_exec_wasm(
    wasm: &[u8],
    name: &str,
    env: WasiEnv,
    runtime: &Arc<dyn Runtime + Send + Sync + 'static>,
) -> Result<TaskJoinHandle, SpawnError> {
    let module = spawn_load_module(name, wasm, runtime).await?;

    spawn_exec_module(module, env, runtime)
}

pub async fn spawn_load_wasm<'a>(
    binary: &'a BinaryPackage,
    name: &str,
) -> Result<&'a [u8], SpawnError> {
    let wasm = if let Some(cmd) = binary.get_command(name) {
        cmd.atom.as_ref()
    } else if let Some(cmd) = binary.get_entrypoint_command() {
        &cmd.atom
    } else {
        tracing::error!(
          command=name,
          pkg=%binary.id,
          "Unable to spawn a command because its package has no entrypoint",
        );
        return Err(SpawnError::MissingEntrypoint {
            package_id: binary.id.clone(),
        });
    };
    Ok(wasm)
}

pub async fn spawn_load_module(
    name: &str,
    wasm: &[u8],
    runtime: &Arc<dyn Runtime + Send + Sync + 'static>,
) -> Result<Module, SpawnError> {
    match runtime.load_module(wasm).await {
        Ok(module) => Ok(module),
        Err(err) => {
            tracing::error!(
                command = name,
                error = &err as &dyn std::error::Error,
                "Failed to compile the module",
            );
            Err(err)
        }
    }
}

pub async fn spawn_union_fs(env: &WasiEnv, binary: &BinaryPackage) -> Result<(), SpawnError> {
    // If the file system has not already been union'ed then do so
    env.state
        .fs
        .conditional_union(binary)
        .await
        .map_err(|err| {
            tracing::warn!("failed to union file system - {err}");
            SpawnError::FileSystemError(crate::ExtendedFsError::with_msg(
                err,
                "could not union filesystems",
            ))
        })?;
    tracing::debug!("{:?}", env.state.fs);
    Ok(())
}

pub fn spawn_exec_module(
    module: Module,
    env: WasiEnv,
    runtime: &Arc<dyn Runtime + Send + Sync + 'static>,
) -> Result<TaskJoinHandle, SpawnError> {
    // Create a new task manager
    let tasks = runtime.task_manager();

    // Create the signaler
    let pid = env.pid();

    let join_handle = env.thread.join_handle();
    {
        // Create a thread that will run this process
        let tasks_outer = tasks.clone();

        tasks_outer
            .task_wasm(
                TaskWasm::new(Box::new(run_exec), env, module, true).with_pre_run(Box::new(
                    |ctx, store| {
                        Box::pin(async move {
                            ctx.data(store).state.fs.close_cloexec_fds().await;
                        })
                    },
                )),
            )
            .map_err(|err| {
                error!("wasi[{}]::failed to launch module - {}", pid, err);
                SpawnError::UnknownError
            })?
    };

    Ok(join_handle)
}

/// # SAFETY
/// This must be executed from the same thread that owns the instance as
/// otherwise it will cause a panic
unsafe fn run_recycle(
    callback: Option<Box<TaskWasmRecycle>>,
    ctx: WasiFunctionEnv,
    mut store: Store,
) {
    if let Some(callback) = callback {
        let env = ctx.data_mut(&mut store);
        let memory = env.memory().clone();

        let props = TaskWasmRecycleProperties {
            env: env.clone(),
            memory,
            store,
        };
        callback(props);
    }
}

pub fn run_exec(props: TaskWasmRunProperties) {
    let ctx = props.ctx;
    let mut store = props.store;

    // Create the WasiFunctionEnv
    let thread = WasiThreadRunGuard::new(ctx.data(&store).thread.clone());
    let recycle = props.recycle;

    // Perform the initialization
    let ctx = {
        // If this module exports an _initialize function, run that first.
        if let Ok(initialize) = unsafe { ctx.data(&store).inner() }
            .instance
            .exports
            .get_function("_initialize")
        {
            let initialize = initialize.clone();
            if let Err(err) = initialize.call(&mut store, &[]) {
                thread.thread.set_status_finished(Err(err.into()));
                ctx.data(&store)
                    .blocking_on_exit(Some(Errno::Noexec.into()));
                unsafe { run_recycle(recycle, ctx, store) };
                return;
            }
        }

        WasiFunctionEnv { env: ctx.env }
    };

    // Bootstrap the process
    // Unsafe: The bootstrap must be executed in the same thread that runs the
    //         actual WASM code
    let rewind_state = match unsafe { ctx.bootstrap(&mut store) } {
        Ok(r) => r,
        Err(err) => {
            tracing::warn!("failed to bootstrap - {}", err);
            thread.thread.set_status_finished(Err(err));
            ctx.data(&store)
                .blocking_on_exit(Some(Errno::Noexec.into()));
            unsafe { run_recycle(recycle, ctx, store) };
            return;
        }
    };

    // If there is a start function
    debug!("wasi[{}]::called main()", ctx.data(&store).pid());
    // TODO: rewrite to use crate::run_wasi_func

    // Call the module
    call_module(ctx, store, thread, rewind_state, recycle);
}

fn get_start(ctx: &WasiFunctionEnv, store: &Store) -> Option<Function> {
    unsafe { ctx.data(store).inner() }
        .instance
        .exports
        .get_function("_start")
        .cloned()
        .ok()
}

/// Calls the module
fn call_module(
    ctx: WasiFunctionEnv,
    mut store: Store,
    handle: WasiThreadRunGuard,
    rewind_state: Option<(RewindState, RewindResultType)>,
    recycle: Option<Box<TaskWasmRecycle>>,
) {
    let env = ctx.data(&store);
    let pid = env.pid();
    let tasks = env.tasks().clone();
    handle.thread.set_status_running();
    let runtime = env.runtime.clone();

    // If we need to rewind then do so
    if let Some((rewind_state, rewind_result)) = rewind_state {
        let mut ctx = ctx.env.clone().into_mut(&mut store);
        if rewind_state.is_64bit {
            let res = rewind_ext::<Memory64>(
                &mut ctx,
                Some(rewind_state.memory_stack),
                rewind_state.rewind_stack,
                rewind_state.store_data,
                rewind_result,
            );
            if res != Errno::Success {
                ctx.data().blocking_on_exit(Some(res.into()));
                unsafe { run_recycle(recycle, WasiFunctionEnv { env: ctx.as_ref() }, store) };
                return;
            }
        } else {
            let res = rewind_ext::<Memory32>(
                &mut ctx,
                Some(rewind_state.memory_stack),
                rewind_state.rewind_stack,
                rewind_state.store_data,
                rewind_result,
            );
            if res != Errno::Success {
                ctx.data().blocking_on_exit(Some(res.into()));
                unsafe { run_recycle(recycle, WasiFunctionEnv { env: ctx.as_ref() }, store) };
                return;
            }
        };
    }

    // Invoke the start function
    let ret = {
        // Call the module
        let Some(start) = get_start(&ctx, &store) else {
            debug!("wasi[{}]::exec-failed: missing _start function", pid);
            ctx.data(&store)
                .blocking_on_exit(Some(Errno::Noexec.into()));
            unsafe { run_recycle(recycle, ctx, store) };
            return;
        };

        let mut call_ret = start.call(&mut store, &[]);

        loop {
            // Technically, it's an error for a vfork to return from main, but anyway...
            match resume_vfork(&ctx, &mut store, &start, &call_ret) {
                // A vfork was resumed, there may be another, so loop back
                Ok(Some(ret)) => call_ret = ret,

                // An error was encountered when restoring from the vfork, report it
                Err(e) => {
                    call_ret = Err(RuntimeError::user(Box::new(WasiError::Exit(e.into()))));
                    break;
                }

                // No vfork, keep the call_ret value
                Ok(None) => break,
            }
        }

        if let Err(err) = call_ret {
            match err.downcast::<WasiError>() {
                Ok(WasiError::Exit(code)) if code.is_success() => Ok(Errno::Success),
                Ok(WasiError::ThreadExit) => Ok(Errno::Success),
                Ok(WasiError::Exit(code)) => {
                    runtime.on_taint(TaintReason::NonZeroExitCode(code));
                    Err(WasiError::Exit(code).into())
                }
                Ok(WasiError::DeepSleep(deep)) => {
                    // Create the callback that will be invoked when the thread respawns after a deep sleep
                    let rewind = deep.rewind;
                    let respawn = {
                        move |ctx, store, rewind_result| {
                            // Call the thread
                            call_module(
                                ctx,
                                store,
                                handle,
                                Some((rewind, RewindResultType::RewindWithResult(rewind_result))),
                                recycle,
                            );
                        }
                    };

                    // Spawns the WASM process after a trigger
                    if let Err(err) = unsafe {
                        tasks.resume_wasm_after_poller(Box::new(respawn), ctx, store, deep.trigger)
                    } {
                        debug!("failed to go into deep sleep - {}", err);
                    }
                    return;
                }
                Ok(WasiError::UnknownWasiVersion) => {
                    debug!("failed as wasi version is unknown");
                    runtime.on_taint(TaintReason::UnknownWasiVersion);
                    Ok(Errno::Noexec)
                }
                Err(err) => {
                    runtime.on_taint(TaintReason::RuntimeError(err.clone()));
                    Err(WasiRuntimeError::from(err))
                }
            }
        } else {
            Ok(Errno::Success)
        }
    };

    let code = if let Err(err) = &ret {
        match err.as_exit_code() {
            Some(s) => s,
            None => {
                error!("{err}");
                eprintln!("{err}");
                Errno::Noexec.into()
            }
        }
    } else {
        Errno::Success.into()
    };

    // Cleanup the environment
    ctx.data(&store).blocking_on_exit(Some(code));
    unsafe { run_recycle(recycle, ctx, store) };

    debug!("wasi[{pid}]::main() has exited with {code}");
    handle.thread.set_status_finished(ret.map(|a| a.into()));
}

#[allow(clippy::type_complexity)]
fn resume_vfork(
    ctx: &WasiFunctionEnv,
    store: &mut Store,
    start: &Function,
    call_ret: &Result<Box<[Value]>, RuntimeError>,
) -> Result<Option<Result<Box<[Value]>, RuntimeError>>, Errno> {
    let (err, code) = match call_ret {
        Ok(_) => (None, wasmer_wasix_types::wasi::ExitCode::from(0u16)),
        Err(err) => match err.downcast_ref::<WasiError>() {
            // If the child process is just deep sleeping, we don't restore the vfork
            Some(WasiError::DeepSleep(..)) => return Ok(None),

            Some(WasiError::Exit(code)) => (None, *code),
            Some(WasiError::ThreadExit) => (None, wasmer_wasix_types::wasi::ExitCode::from(0u16)),
            Some(WasiError::UnknownWasiVersion) => (None, Errno::Noexec.into()),
            None => (
                Some(WasiRuntimeError::from(err.clone())),
                Errno::Unknown.into(),
            ),
        },
    };

    if let Some(mut vfork) = ctx.data_mut(store).vfork.take() {
        if let Some(err) = err {
            error!(%err, "Error from child process");
            eprintln!("{err}");
        }

        InlineWaker::block_on(
            unsafe { ctx.data(store).get_memory_and_wasi_state(store, 0) }
                .1
                .fs
                .close_all(),
        );

        tracing::debug!(
            pid = %ctx.data_mut(store).process.pid(),
            vfork_pid = %vfork.env.process.pid(),
            "Resuming from vfork after child process was terminated"
        );

        // Restore the WasiEnv to the point when we vforked
        vfork.env.swap_inner(ctx.data_mut(store));
        std::mem::swap(vfork.env.as_mut(), ctx.data_mut(store));
        let mut child_env = *vfork.env;
        child_env.owned_handles.push(vfork.handle);

        // Terminate the child process
        child_env.process.terminate(code);

        // Jump back to the vfork point and current on execution
        let child_pid = child_env.process.pid();
        let rewind_stack = vfork.rewind_stack.freeze();
        let store_data = vfork.store_data;

        let ctx = ctx.env.clone().into_mut(store);
        // Now rewind the previous stack and carry on from where we did the vfork
        let rewind_result = if vfork.is_64bit {
            crate::syscalls::rewind::<Memory64, _>(
                ctx,
                None,
                rewind_stack,
                store_data,
                crate::syscalls::ForkResult {
                    pid: child_pid.raw() as wasmer_wasix_types::wasi::Pid,
                    ret: Errno::Success,
                },
            )
        } else {
            crate::syscalls::rewind::<Memory32, _>(
                ctx,
                None,
                rewind_stack,
                store_data,
                crate::syscalls::ForkResult {
                    pid: child_pid.raw() as wasmer_wasix_types::wasi::Pid,
                    ret: Errno::Success,
                },
            )
        };

        match rewind_result {
            Errno::Success => Ok(Some(start.call(store, &[]))),
            err => {
                warn!("fork failed - could not rewind the stack - errno={}", err);
                Err(err)
            }
        }
    } else {
        Ok(None)
    }
}
