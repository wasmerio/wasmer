use std::{pin::Pin, sync::Arc};

use crate::{
    os::task::{thread::WasiThreadRunGuard, TaskJoinHandle},
    syscalls::rewind,
    RewindState, VirtualBusError, WasiError, WasiRuntimeError,
};
use futures::Future;
use tracing::*;
use wasmer::{Function, FunctionEnvMut, Instance, Memory, Memory32, Memory64, Module, Store};
use wasmer_wasix_types::wasi::Errno;

use super::{BinFactory, BinaryPackage, ModuleCache};
use crate::{
    import_object_for_all_wasi_versions, runtime::SpawnType, SpawnedMemory, WasiEnv,
    WasiFunctionEnv, WasiRuntime,
};

pub fn spawn_exec(
    binary: BinaryPackage,
    name: &str,
    store: Store,
    env: WasiEnv,
    runtime: &Arc<dyn WasiRuntime + Send + Sync + 'static>,
    compiled_modules: &ModuleCache,
) -> Result<TaskJoinHandle, VirtualBusError> {
    // The deterministic id for this engine
    let compiler = store.engine().deterministic_id();

    let module = compiled_modules.get_compiled_module(&store, binary.hash().as_str(), compiler);
    let module = match (module, binary.entry.as_ref()) {
        (Some(a), _) => a,
        (None, Some(entry)) => {
            let module = Module::new(&store, &entry[..]).map_err(|err| {
                error!(
                    "failed to compile module [{}, len={}] - {}",
                    name,
                    entry.len(),
                    err
                );
                VirtualBusError::CompileError
            });
            if module.is_err() {
                env.blocking_cleanup(Some(Errno::Noexec.into()));
            }
            let module = module?;
            compiled_modules.set_compiled_module(binary.hash().as_str(), compiler, &module);
            module
        }
        (None, None) => {
            error!("package has no entry [{}]", name,);
            env.blocking_cleanup(Some(Errno::Noexec.into()));
            return Err(VirtualBusError::CompileError);
        }
    };

    // If the file system has not already been union'ed then do so
    env.state.fs.conditional_union(&binary);
    tracing::debug!("{:?}", env.state.fs);

    // Now run the module
    spawn_exec_module(module, store, env, runtime)
}

pub fn spawn_exec_module(
    module: Module,
    store: Store,
    env: WasiEnv,
    runtime: &Arc<dyn WasiRuntime + Send + Sync + 'static>,
) -> Result<TaskJoinHandle, VirtualBusError> {
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
            Some(ty) => {
                #[cfg(feature = "sys")]
                let style = store.engine().tunables().memory_style(&ty);
                SpawnType::CreateWithType(SpawnedMemory {
                    ty,
                    #[cfg(feature = "sys")]
                    style,
                })
            }
            None => SpawnType::Create,
        };

        // Create a thread that will run this process
        let runtime = runtime.clone();
        let tasks_outer = tasks.clone();

        let task = {
            move |mut store: Store, module, memory| {
                // Create the WasiFunctionEnv
                let mut wasi_env = env;
                wasi_env.runtime = runtime;
                let thread = WasiThreadRunGuard::new(wasi_env.thread.clone());

                // Perform the initialization
                let (start, ctx) = {
                    let mut ctx = WasiFunctionEnv::new(&mut store, wasi_env);

                    // Let's instantiate the module with the imports.
                    let (mut import_object, init) =
                        import_object_for_all_wasi_versions(&module, &mut store, &ctx.env);
                    let imported_memory = if let Some(memory) = memory {
                        let imported_memory = Memory::new_from_existing(&mut store, memory);
                        import_object.define("env", "memory", imported_memory.clone());
                        Some(imported_memory)
                    } else {
                        None
                    };

                    let instance = match Instance::new(&mut store, &module, &import_object) {
                        Ok(a) => a,
                        Err(err) => {
                            error!("wasi[{}]::wasm instantiate error ({})", pid, err);
                            ctx.data(&store)
                                .blocking_cleanup(Some(Errno::Noexec.into()));
                            return;
                        }
                    };

                    init(&instance, &store).unwrap();

                    // Initialize the WASI environment
                    if let Err(err) =
                        ctx.initialize_with_memory(&mut store, instance.clone(), imported_memory)
                    {
                        error!("wasi[{}]::wasi initialize error ({})", pid, err);
                        ctx.data(&store)
                            .blocking_cleanup(Some(Errno::Noexec.into()));
                        return;
                    }

                    // Set the asynchronous threads flag
                    let capable_of_deep_sleep = ctx.data(&store).capable_of_deep_sleep();
                    ctx.data_mut(&mut store).enable_deep_sleep = capable_of_deep_sleep;

                    // If this module exports an _initialize function, run that first.
                    if let Ok(initialize) = instance.exports.get_function("_initialize") {
                        if let Err(err) = initialize.call(&mut store, &[]) {
                            thread.thread.set_status_finished(Err(err.into()));
                            ctx.data(&store)
                                .blocking_cleanup(Some(Errno::Noexec.into()));
                            return;
                        }
                    }

                    // Let's call the `_start` function, which is our `main` function in Rust.
                    let start = instance
                        .exports
                        .get_function("_start")
                        .map(|a| a.clone())
                        .ok();
                    let ctx = WasiFunctionEnv { env: ctx.env };

                    (start, ctx)
                };

                // If there is a start function
                debug!("wasi[{}]::called main()", pid);
                // TODO: rewrite to use crate::run_wasi_func

                // Call the module
                if let Some(start) = start {
                    call_module(ctx, store, module, start, None);
                } else {
                    debug!("wasi[{}]::exec-failed: missing _start function", pid);
                    ctx.data(&store)
                        .blocking_cleanup(Some(Errno::Noexec.into()));
                }
            }
        };

        tasks_outer
            .task_wasm(Box::new(task), store, module, memory_spawn)
            .map_err(|err| {
                error!("wasi[{}]::failed to launch module - {}", pid, err);
                VirtualBusError::UnknownError
            })?
    };

    Ok(join_handle)
}

/// Calls the module
fn call_module(
    ctx: WasiFunctionEnv,
    mut store: Store,
    module: Module,
    start: Function,
    rewind_state: Option<(RewindState, Result<(), Errno>)>,
) {
    let env = ctx.data(&store);
    let pid = env.pid();
    let thread = env.thread.clone();
    let tasks = env.tasks().clone();
    thread.set_status_running();

    // If we need to rewind then do so
    if let Some((mut rewind_state, trigger_res)) = rewind_state {
        if rewind_state.is_64bit {
            if let Err(exit_code) = rewind_state
                .rewinding_finish::<Memory64>(ctx.env.clone().into_mut(&mut store), trigger_res)
            {
                ctx.data(&store).blocking_cleanup(Some(exit_code));
                return;
            }
            let res = rewind::<Memory64>(
                ctx.env.clone().into_mut(&mut store),
                rewind_state.memory_stack,
                rewind_state.rewind_stack,
                rewind_state.store_data,
            );
            if res != Errno::Success {
                ctx.data(&store).blocking_cleanup(Some(res.into()));
                return;
            }
        } else {
            if let Err(exit_code) = rewind_state
                .rewinding_finish::<Memory32>(ctx.env.clone().into_mut(&mut store), trigger_res)
            {
                ctx.data(&store).blocking_cleanup(Some(exit_code));
                return;
            }
            let res = rewind::<Memory32>(
                ctx.env.clone().into_mut(&mut store),
                rewind_state.memory_stack,
                rewind_state.rewind_stack,
                rewind_state.store_data,
            );
            if res != Errno::Success {
                ctx.data(&store).blocking_cleanup(Some(res.into()));
                return;
            }
        };
    }

    // Invoke the start function
    let ret = {
        let call_ret = start.call(&mut store, &[]);

        if let Err(err) = call_ret {
            match err.downcast::<WasiError>() {
                Ok(WasiError::Exit(code)) => {
                    if code.is_success() {
                        Ok(Errno::Success)
                    } else {
                        Ok(Errno::Noexec)
                    }
                }
                Ok(WasiError::DeepSleep(deep)) => {
                    // Create the callback that will be invoked when the thread respawns after a deep sleep
                    let rewind = deep.rewind;
                    let respawn = {
                        let mut ctx = ctx.clone();
                        move |store, module, trigger_res| {
                            // Reinitialize and then call the thread
                            let store = ctx.may_reinitialize(store, &module)?;
                            call_module(ctx, store, module, start, Some((rewind, trigger_res)));
                            Ok(())
                        }
                    };

                    // Spawns the WASM process after a trigger
                    if let Err(err) = tasks.resume_wasm_after_poller(
                        Box::new(respawn),
                        store,
                        module,
                        ctx,
                        deep.work,
                    ) {
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
    thread.set_status_finished(ret.map(|a| a.into()));
}

impl BinFactory {
    pub fn spawn<'a>(
        &'a self,
        name: String,
        store: Store,
        env: WasiEnv,
    ) -> Pin<Box<dyn Future<Output = Result<TaskJoinHandle, VirtualBusError>> + 'a>> {
        Box::pin(async move {
            // Find the binary (or die trying) and make the spawn type
            let binary = self
                .get_binary(name.as_str(), Some(env.fs_root()))
                .await
                .ok_or(VirtualBusError::NotFound);
            if binary.is_err() {
                env.cleanup(Some(Errno::Noent.into())).await;
            }
            let binary = binary?;

            // Execute
            spawn_exec(
                binary,
                name.as_str(),
                store,
                env,
                &self.runtime,
                &self.cache,
            )
        })
    }

    pub fn try_built_in(
        &self,
        name: String,
        parent_ctx: Option<&FunctionEnvMut<'_, WasiEnv>>,
        store: &mut Option<Store>,
        builder: &mut Option<WasiEnv>,
    ) -> Result<TaskJoinHandle, VirtualBusError> {
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
        Err(VirtualBusError::NotFound)
    }
}
