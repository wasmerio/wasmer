use std::{pin::Pin, sync::Arc};

use crate::{
    os::task::{thread::WasiThreadRunGuard, TaskJoinHandle},
    runtime::task_manager::{TaskWasm, TaskWasmRunProperties},
    syscalls::rewind_ext,
    RewindState, SpawnError, WasiError, WasiRuntimeError,
};
use bytes::Bytes;
use futures::Future;
use tracing::*;
use wasmer::{Function, FunctionEnvMut, Memory32, Memory64, Module, Store};
use wasmer_wasix_types::wasi::Errno;

use super::{BinFactory, BinaryPackage};
use crate::{runtime::SpawnMemoryType, Runtime, WasiEnv, WasiFunctionEnv};

#[tracing::instrument(level = "trace", skip_all, fields(%name, %binary.package_name))]
pub async fn spawn_exec(
    binary: BinaryPackage,
    name: &str,
    _store: Store,
    env: WasiEnv,
    runtime: &Arc<dyn Runtime + Send + Sync + 'static>,
) -> Result<TaskJoinHandle, SpawnError> {
    let wasm = if let Some(cmd) = binary.get_command(name) {
        cmd.atom.as_ref()
    } else if let Some(wasm) = binary.entrypoint_bytes() {
        wasm
    } else {
        tracing::error!(
          command=name,
          pkg.name=%binary.package_name,
          pkg.version=%binary.version,
          "Unable to spawn a command because its package has no entrypoint",
        );
        env.cleanup(Some(Errno::Noexec.into())).await;
        return Err(SpawnError::CompileError);
    };

    let module = match runtime.load_module(wasm).await {
        Ok(module) => module,
        Err(err) => {
            tracing::error!(
                command = name,
                error = &*err,
                "Failed to compile the module",
            );
            env.cleanup(Some(Errno::Noexec.into())).await;
            return Err(SpawnError::CompileError);
        }
    };

    // If the file system has not already been union'ed then do so
    // env.state
    //     .fs
    //     .conditional_union(&binary)
    //     .await
    //     .map_err(|err| {
    //         tracing::warn!("failed to union file system - {err}");
    //         SpawnError::FileSystemError
    //     })?;
    tracing::debug!("{:?}", env.state.fs);

    // Now run the module
    spawn_exec_module(module, env, runtime)
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
        // Determine if shared memory needs to be created and imported
        let shared_memory = module.imports().memories().next().map(|a| *a.ty());

        // Determine if we are going to create memory and import it or just rely on self creation of memory
        let memory_spawn = match shared_memory {
            Some(ty) => SpawnMemoryType::CreateMemoryOfType(ty),
            None => SpawnMemoryType::CreateMemory,
        };

        // Create a thread that will run this process
        let tasks_outer = tasks.clone();

        let run = {
            move |props: TaskWasmRunProperties| {
                let ctx = props.ctx;
                let mut store = props.store;

                // Create the WasiFunctionEnv
                let thread = WasiThreadRunGuard::new(ctx.data(&store).thread.clone());

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
                                .blocking_cleanup(Some(Errno::Noexec.into()));
                            return;
                        }
                    }

                    WasiFunctionEnv { env: ctx.env }
                };

                // If there is a start function
                debug!("wasi[{}]::called main()", pid);
                // TODO: rewrite to use crate::run_wasi_func

                // Call the module
                call_module(ctx, store, thread, None);
            }
        };

        tasks_outer
            .task_wasm(TaskWasm::new(Box::new(run), env, module, true).with_memory(memory_spawn))
            .map_err(|err| {
                error!("wasi[{}]::failed to launch module - {}", pid, err);
                SpawnError::UnknownError
            })?
    };

    Ok(join_handle)
}

fn get_start(ctx: &WasiFunctionEnv, store: &Store) -> Option<Function> {
    unsafe { ctx.data(store).inner() }
        .instance
        .exports
        .get_function("_start")
        .map(|a| a.clone())
        .ok()
}

/// Calls the module
fn call_module(
    ctx: WasiFunctionEnv,
    mut store: Store,
    handle: WasiThreadRunGuard,
    rewind_state: Option<(RewindState, Bytes)>,
) {
    let env = ctx.data(&store);
    let pid = env.pid();
    let tasks = env.tasks().clone();
    handle.thread.set_status_running();

    // If we need to rewind then do so
    if let Some((rewind_state, rewind_result)) = rewind_state {
        if rewind_state.is_64bit {
            let res = rewind_ext::<Memory64>(
                ctx.env.clone().into_mut(&mut store),
                rewind_state.memory_stack,
                rewind_state.rewind_stack,
                rewind_state.store_data,
                rewind_result,
            );
            if res != Errno::Success {
                ctx.data(&store).blocking_cleanup(Some(res.into()));
                return;
            }
        } else {
            let res = rewind_ext::<Memory32>(
                ctx.env.clone().into_mut(&mut store),
                rewind_state.memory_stack,
                rewind_state.rewind_stack,
                rewind_state.store_data,
                rewind_result,
            );
            if res != Errno::Success {
                ctx.data(&store).blocking_cleanup(Some(res.into()));
                return;
            }
        };
    }

    // Invoke the start function
    let ret = {
        // Call the module
        let call_ret = if let Some(start) = get_start(&ctx, &store) {
            start.call(&mut store, &[])
        } else {
            debug!("wasi[{}]::exec-failed: missing _start function", pid);
            ctx.data(&store)
                .blocking_cleanup(Some(Errno::Noexec.into()));
            return;
        };

        if let Err(err) = call_ret {
            match err.downcast::<WasiError>() {
                Ok(WasiError::Exit(code)) if code.is_success() => Ok(Errno::Success),
                Ok(err @ WasiError::Exit(_)) => Err(err.into()),
                Ok(WasiError::DeepSleep(deep)) => {
                    // Create the callback that will be invoked when the thread respawns after a deep sleep
                    let rewind = deep.rewind;
                    let respawn = {
                        move |ctx, store, rewind_result| {
                            // Call the thread
                            call_module(ctx, store, handle, Some((rewind, rewind_result)));
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
                    debug!("failed as wasi version is unknown",);
                    Ok(Errno::Noexec)
                }
                Err(err) => Err(WasiRuntimeError::from(err)),
            }
        } else {
            Ok(Errno::Success)
        }
    };

    let code = if let Err(err) = &ret {
        err.as_exit_code().unwrap_or_else(|| Errno::Noexec.into())
    } else {
        Errno::Success.into()
    };

    // Cleanup the environment
    ctx.data(&store).blocking_cleanup(Some(code));

    debug!("wasi[{pid}]::main() has exited with {code}");
    handle.thread.set_status_finished(ret.map(|a| a.into()));
}

impl BinFactory {
    pub fn spawn<'a>(
        &'a self,
        name: String,
        store: Store,
        env: WasiEnv,
    ) -> Pin<Box<dyn Future<Output = Result<TaskJoinHandle, SpawnError>> + 'a>> {
        Box::pin(async move {
            // Find the binary (or die trying) and make the spawn type
            let binary = self
                .get_binary(name.as_str(), Some(env.fs_root()))
                .await
                .ok_or(SpawnError::NotFound);
            if binary.is_err() {
                env.cleanup(Some(Errno::Noent.into())).await;
            }
            let binary = binary?;

            // Execute
            spawn_exec(binary, name.as_str(), store, env, &self.runtime).await
        })
    }

    pub fn try_built_in(
        &self,
        name: String,
        parent_ctx: Option<&FunctionEnvMut<'_, WasiEnv>>,
        store: &mut Option<Store>,
        builder: &mut Option<WasiEnv>,
    ) -> Result<TaskJoinHandle, SpawnError> {
        // We check for built in commands
        if let Some(parent_ctx) = parent_ctx {
            if self.commands.exists(name.as_str()) {
                return self
                    .commands
                    .exec(parent_ctx, name.as_str(), store, builder);
            }
        } else if self.commands.exists(name.as_str()) {
            tracing::warn!("builtin command without a parent ctx - {}", name);
        }
        Err(SpawnError::NotFound)
    }
}
