use std::sync::Arc;

use crate::{
    os::task::control_plane::ControlPlaneError, vbus::BusSpawnedProcess, WasiRuntimeError,
};
use wasmer::{Instance, Memory, Module, Store};
use wasmer_wasi_types::wasi::{Errno, ExitCode};

use super::{BinaryPackage, SpawnResultSender, SpawnedInstance};
use crate::{
    import_object_for_all_wasi_versions, runtime::SpawnType, SpawnedMemory, WasiEnv, WasiError,
    WasiFunctionEnv,
};

pub async fn spawn_exec_command(
    name: &str,
    store: Store,
    env: WasiEnv,
) -> Result<BusSpawnedProcess, WasiRuntimeError> {
    let pkg = env.bin_factory.must_resolve_binary(name, None).await?;
    spawn_exec(pkg, store, env)
}

pub fn spawn_exec(
    binary: Arc<BinaryPackage>,
    store: Store,
    env: WasiEnv,
) -> Result<BusSpawnedProcess, WasiRuntimeError> {
    // If the file system has not already been union'ed then do so
    env.state.fs.conditional_union(&binary);

    // Now run the module
    let mut spawned = spawn_exec_module(binary.module.clone(), store, env)?;

    spawned.module_memory_footprint = binary.module_size_bytes;
    spawned.file_system_memory_footprint = binary.module_memory_footprint;

    Ok(spawned)
}

pub fn spawn_exec_module(
    module: Module,
    store: Store,
    env: WasiEnv,
) -> Result<BusSpawnedProcess, WasiRuntimeError> {
    // Create a new task manager
    let tasks = env.runtime.task_manager();

    // Create the signaler
    let pid = env.pid();
    let signaler = Box::new(env.process.clone());

    // Now run the binary

    let (result_sender, result_receiver) = SpawnedInstance::new();
    {
        // Create a thread that will run this process
        let runtime = env.runtime.clone();
        let tasks_outer = tasks.clone();

        let task = {
            move || {
                spawn_exec_module_inner(result_sender, env, &mut store, module);
                env.cleanup(None);
            }
        };

        tasks_outer.task_wasm(Box::new(task)).map_err(|err| {
            tracing::error!("wasi[{}]::failed to launch module - {}", pid, err);
            WasiRuntimeError::ControlPlane(ControlPlaneError::TaskAborted)
        })?
    };

    Ok(BusSpawnedProcess {
        inst: result_receiver,
        stdin: None,
        stdout: None,
        stderr: None,
        signaler: Some(signaler),
        module_memory_footprint: 0,
        file_system_memory_footprint: 0,
    })
}

fn spawn_exec_module_inner(
    sender: SpawnResultSender,
    mut env: WasiEnv,
    store: &mut Store,
    module: Module,
) {
    // Determine if shared memory needs to be created and imported
    let shared_memory = module.imports().memories().next().map(|a| *a.ty());

    // Determine if we are going to create memory and import it or just rely on self creation of memory
    let spawn_type = match shared_memory {
        Some(ty) => {
            #[cfg(feature = "sys")]
            let style = store.tunables().memory_style(&ty);
            SpawnType::CreateWithType(SpawnedMemory {
                ty,
                #[cfg(feature = "sys")]
                style,
            })
        }
        None => SpawnType::Create,
    };

    let pid = env.process.pid();
    let memory = match env.tasks().build_memory(spawn_type) {
        Ok(m) => m,
        Err(err) => {
            tracing::error!("wasi[{}]::wasm could not build memory error ({})", pid, err);
            env.cleanup(Some(Errno::Noexec as ExitCode));
            return;
        }
    };

    let mut wasi_env = WasiFunctionEnv::new(&mut store, env);

    // Let's instantiate the module with the imports.
    let (mut import_object, init) =
        import_object_for_all_wasi_versions(&module, &mut store, &wasi_env.env);
    if let Some(memory) = memory {
        import_object.define(
            "env",
            "memory",
            Memory::new_from_existing(&mut store, memory),
        );
    }
    let instance = match Instance::new(&mut store, &module, &import_object) {
        Ok(a) => a,
        Err(err) => {
            tracing::error!("wasi[{}]::wasm instantiate error ({})", pid, err);
            wasi_env
                .data(&store)
                .cleanup(Some(Errno::Noexec as ExitCode));
            return;
        }
    };

    init(&instance, &store).unwrap();

    // Initialize the WASI environment
    if let Err(err) = wasi_env.initialize(&mut store, instance.clone()) {
        tracing::error!("wasi[{}]::wasi initialize error ({})", pid, err);
        wasi_env
            .data(&store)
            .cleanup(Some(Errno::Noexec as ExitCode));
        return;
    }

    // If this module exports an _initialize function, run that first.
    if let Ok(initialize) = instance.exports.get_function("_initialize") {
        if let Err(e) = initialize.call(&mut store, &[]) {
            let code = match e.downcast::<WasiError>() {
                Ok(WasiError::Exit(code)) => code as ExitCode,
                Ok(WasiError::UnknownWasiVersion) => {
                    tracing::debug!("wasi[{}]::exec-failed: unknown wasi version", pid);
                    Errno::Noexec as ExitCode
                }
                Err(err) => {
                    tracing::debug!("wasi[{}]::exec-failed: runtime error - {}", pid, err);
                    Errno::Noexec as ExitCode
                }
            };

            sender.on_exit(code);
            wasi_env
                .data(&store)
                .cleanup(Some(Errno::Noexec as ExitCode));
            return;
        }
    }

    // Let's call the `_start` function, which is our `main` function in Rust.
    let start = instance.exports.get_function("_start").ok();

    // If there is a start function
    tracing::debug!("wasi[{}]::called main()", pid);
    // TODO: rewrite to use crate::run_wasi_func
    let exit_code = if let Some(start) = start {
        match start.call(&mut store, &[]) {
            Ok(_) => 0,
            Err(e) => match e.downcast::<WasiError>() {
                Ok(WasiError::Exit(code)) => code,
                Ok(WasiError::UnknownWasiVersion) => {
                    tracing::debug!("wasi[{}]::exec-failed: unknown wasi version", pid);
                    Errno::Noexec as u32
                }
                Err(err) => {
                    tracing::debug!("wasi[{}]::exec-failed: runtime error - {}", pid, err);
                    9999u32
                }
            },
        }
    } else {
        tracing::debug!("wasi[{}]::exec-failed: missing _start function", pid);
        Errno::Noexec as u32
    };
    tracing::debug!("wasi[{}]::main() has exited with {}", pid, exit_code);

    // Cleanup the environment
    wasi_env.data(&store).cleanup(Some(exit_code));

    // Send the result
    sender.on_exit(exit_code);
}
