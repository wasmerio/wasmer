#![allow(clippy::result_large_err)]
use super::{BinaryPackage, BinaryPackageCommand};
use crate::{
    RewindState, SpawnError, WasiError, WasiRuntimeError,
    os::task::{
        TaskJoinHandle,
        thread::{RewindResultType, WasiThreadRunGuard},
    },
    runtime::{
        ModuleInput, TaintReason,
        module_cache::HashedModuleData,
        task_manager::{
            TaskWasm, TaskWasmRecycle, TaskWasmRecycleProperties, TaskWasmRunProperties,
        },
    },
    state::context_switching::ContextSwitchingEnvironment,
    syscalls::rewind_ext,
};
use crate::{Runtime, WasiEnv, WasiFunctionEnv};
use std::{borrow::Cow, sync::Arc};
use tracing::*;
use virtual_mio::block_on;
use wasmer::{Function, Memory32, Memory64, Module, Store};
use wasmer_wasix_types::wasi::Errno;

#[tracing::instrument(level = "trace", skip_all, fields(%name, package_id=%binary.id))]
pub async fn spawn_exec(
    binary: BinaryPackage,
    name: &str,
    env: WasiEnv,
    runtime: &Arc<dyn Runtime + Send + Sync + 'static>,
) -> Result<TaskJoinHandle, SpawnError> {
    spawn_union_fs(&env, &binary).await?;

    let cmd = package_command_by_name(&binary, name)?;
    let input = ModuleInput::Command(Cow::Borrowed(cmd));
    let module = runtime.resolve_module(input, None, None).await?;

    // Free the space used by the binary, since we don't need it
    // any longer
    drop(binary);

    spawn_exec_module(module, env, runtime)
}

#[tracing::instrument(level = "trace", skip_all, fields(%name))]
pub async fn spawn_exec_wasm(
    wasm: HashedModuleData,
    name: &str,
    env: WasiEnv,
    runtime: &Arc<dyn Runtime + Send + Sync + 'static>,
) -> Result<TaskJoinHandle, SpawnError> {
    let module = spawn_load_module(name, wasm, runtime).await?;

    spawn_exec_module(module, env, runtime)
}

pub fn package_command_by_name<'a>(
    pkg: &'a BinaryPackage,
    name: &str,
) -> Result<&'a BinaryPackageCommand, SpawnError> {
    // If an explicit command is provided, use it.
    // Otherwise, use the entrypoint.
    // If no entrypoint exists, and the package has a single
    // command, then use it. This is done for backwards
    // compatibility.
    let cmd = if let Some(cmd) = pkg.get_command(name) {
        cmd
    } else if let Some(cmd) = pkg.get_entrypoint_command() {
        cmd
    } else {
        match pkg.commands.as_slice() {
            // Package only has a single command, so use it.
            [first] => first,
            // Package either has no command, or has multiple commands, which
            // would make the choice ambiguous, so fail.
            _ => {
                return Err(SpawnError::MissingEntrypoint {
                    package_id: pkg.id.clone(),
                });
            }
        }
    };

    Ok(cmd)
}

pub async fn spawn_load_module(
    name: &str,
    wasm: HashedModuleData,
    runtime: &Arc<dyn Runtime + Send + Sync + 'static>,
) -> Result<Module, SpawnError> {
    match runtime.load_hashed_module(wasm, None).await {
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
                TaskWasm::new(Box::new(run_exec), env, module, true, true).with_pre_run(Box::new(
                    |ctx, store| {
                        Box::pin(async move {
                            ctx.data(store).state.fs.close_cloexec_fds().await;
                        })
                    },
                )),
            )
            .map_err(|err| {
                error!("wasi[{}]::failed to launch module - {}", pid, err);
                SpawnError::Other(Box::new(err))
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
        let memory = unsafe { env.memory() }.clone();

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
    // If this module exports an _initialize function, run that first.
    if let Ok(initialize) = ctx
        .data(&store)
        .inner()
        .main_module_instance_handles()
        .instance
        .exports
        .get_function("_initialize")
        .cloned()
    {
        // This does not need a context switching environment as the documentation
        // states that that is only available after the first call to main
        let result = initialize.call(&mut store, &[]);

        if let Err(err) = result {
            thread.thread.set_status_finished(Err(err.into()));
            ctx.data(&store)
                .blocking_on_exit(Some(Errno::Noexec.into()));
            unsafe { run_recycle(recycle, ctx, store) };
            return;
        }
    }

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
    ctx.data(store)
        .inner()
        .main_module_instance_handles()
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
    // Call the module
    let Some(start) = get_start(&ctx, &store) else {
        debug!("wasi[{}]::exec-failed: missing _start function", pid);
        ctx.data(&store)
            .blocking_on_exit(Some(Errno::Noexec.into()));
        unsafe { run_recycle(recycle, ctx, store) };
        return;
    };

    let (mut store, call_ret) =
        ContextSwitchingEnvironment::run_main_context(&ctx, store, start.clone(), vec![]);

    let ret = if let Err(err) = call_ret {
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
            Ok(WasiError::DlSymbolResolutionFailed(symbol)) => {
                debug!("failed as a needed DL symbol could not be resolved");
                runtime.on_taint(TaintReason::DlSymbolResolutionFailed(symbol.clone()));
                Err(WasiError::DlSymbolResolutionFailed(symbol).into())
            }
            Err(err) => {
                runtime.on_taint(TaintReason::RuntimeError(err.clone()));
                Err(WasiRuntimeError::from(err))
            }
        }
    } else {
        Ok(Errno::Success)
    };

    let code = if let Err(err) = &ret {
        match err.as_exit_code() {
            Some(s) => s,
            None => {
                let err_display = err.display(&mut store);
                error!("{err_display}");
                eprintln!("{err_display}");
                Errno::Noexec.into()
            }
        }
    } else {
        Errno::Success.into()
    };

    // If we're in a vfork that didn't exec or exit, we need to clean up the vfork resources
    // This handles the undefined behavior case where a vforked child returns from main
    if let Some(mut vfork) = ctx.data_mut(&mut store).vfork.take() {
        tracing::warn!(
            "vforked process returned from main without calling exec or exit - cleaning up resources"
        );
        
        // Close all file descriptors in the child environment before restoring parent
        block_on(
            unsafe { ctx.data(&store).get_memory_and_wasi_state(&store, 0) }
                .1
                .fs
                .close_all()
        );
        
        // Restore the WasiEnv to the point when we vforked (following proc_exit2 pattern)
        let mut parent_env = vfork.env;
        let ctx_data = ctx.data_mut(&mut store);
        ctx_data.swap_inner(parent_env.as_mut());
        let mut child_env = std::mem::replace(ctx_data, *parent_env);
        
        // Transfer thread handle ownership to child so it's properly cleaned up
        child_env.owned_handles.push(vfork.handle);
        
        // Terminate the child process
        child_env.process.terminate(code);
        
        // The parent environment is now restored. The subsequent blocking_on_exit will
        // clean up the parent's resources, not the child's (which were already closed above).
    }

    // Cleanup the environment
    ctx.data(&store).blocking_on_exit(Some(code));
    unsafe { run_recycle(recycle, ctx, store) };

    debug!("wasi[{pid}]::main() has exited with {code}");
    handle.thread.set_status_finished(ret.map(|a| a.into()));
}
