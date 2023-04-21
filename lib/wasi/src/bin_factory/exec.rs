use std::{pin::Pin, sync::Arc};

use crate::{
    os::task::{thread::WasiThreadRunGuard, TaskJoinHandle},
    VirtualBusError, WasiRuntimeError,
};
use futures::Future;
use tracing::*;
use wasmer::{FunctionEnvMut, Instance, Memory, Module, Store};
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
                env.blocking_cleanup(&store, Some(Errno::Noexec.into()));
            }
            let module = module?;
            compiled_modules.set_compiled_module(binary.hash().as_str(), compiler, &module);
            module
        }
        (None, None) => {
            error!("package has no entry [{}]", name,);
            env.blocking_cleanup(&store, Some(Errno::Noexec.into()));
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
            Some(ty) => SpawnType::CreateWithType(SpawnedMemory { ty }),
            None => SpawnType::Create,
        };

        // Create a thread that will run this process
        let runtime = runtime.clone();
        let tasks_outer = tasks.clone();

        let task = {
            move |mut store, module, memory: Option<Memory>| {
                // Create the WasiFunctionEnv
                let mut wasi_env = env;
                wasi_env.runtime = runtime;
                let thread = WasiThreadRunGuard::new(wasi_env.thread.clone());

                let mut wasi_env = WasiFunctionEnv::new(&mut store, wasi_env);

                // Let's instantiate the module with the imports.
                let (mut import_object, init) =
                    import_object_for_all_wasi_versions(&module, &mut store, &wasi_env.env);
                let imported_memory = if let Some(memory) = memory {
                    import_object.define("env", "memory", memory.clone());
                    Some(memory)
                } else {
                    None
                };

                let instance = match Instance::new(&mut store, &module, &import_object) {
                    Ok(a) => a,
                    Err(err) => {
                        error!("wasi[{}]::wasm instantiate error ({})", pid, err);
                        wasi_env
                            .data(&store)
                            .blocking_cleanup(&store, Some(Errno::Noexec.into()));
                        return;
                    }
                };

                init(&instance, &store).unwrap();

                // Initialize the WASI environment
                if let Err(err) =
                    wasi_env.initialize_with_memory(&mut store, instance.clone(), imported_memory)
                {
                    error!("wasi[{}]::wasi initialize error ({})", pid, err);
                    wasi_env
                        .data(&store)
                        .blocking_cleanup(&store, Some(Errno::Noexec.into()));
                    return;
                }

                // If this module exports an _initialize function, run that first.
                if let Ok(initialize) = instance.exports.get_function("_initialize") {
                    if let Err(err) = initialize.call(&mut store, &[]) {
                        thread.thread.set_status_finished(Err(err.into()));
                        wasi_env
                            .data(&store)
                            .blocking_cleanup(&store, Some(Errno::Noexec.into()));
                        return;
                    }
                }

                // Let's call the `_start` function, which is our `main` function in Rust.
                let start = instance.exports.get_function("_start").ok();

                // If there is a start function
                debug!("wasi[{}]::called main()", pid);
                // TODO: rewrite to use crate::run_wasi_func

                thread.thread.set_status_running();

                let ret = if let Some(start) = start {
                    start
                        .call(&mut store, &[])
                        .map_err(WasiRuntimeError::from)
                        .map(|_| Errno::Success)
                } else {
                    debug!("wasi[{}]::exec-failed: missing _start function", pid);
                    Ok(Errno::Noexec)
                };

                let code = if let Err(err) = &ret {
                    err.as_exit_code().unwrap_or_else(|| Errno::Noexec.into())
                } else {
                    Errno::Success.into()
                };

                // Cleanup the environment
                wasi_env.data(&store).blocking_cleanup(&store, Some(code));

                debug!("wasi[{pid}]::main() has exited with {code}");
                thread.thread.set_status_finished(ret.map(|a| a.into()));
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
                env.cleanup(&store, Some(Errno::Noent.into())).await;
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
