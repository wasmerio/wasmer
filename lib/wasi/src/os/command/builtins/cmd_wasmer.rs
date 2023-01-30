use std::{any::Any, ops::Deref, sync::Arc};

use wasmer::{FunctionEnvMut, Store};
use wasmer_vbus::{BusSpawnedProcess, SpawnOptions, VirtualBusError};
use wasmer_wasi_types::wasi::Errno;

use crate::{
    bin_factory::{spawn_exec, BinaryPackage, ModuleCache},
    syscalls::stderr_write,
    VirtualTaskManager, VirtualTaskManagerExt, WasiEnv, WasiRuntimeImplementation,
};

const HELP: &str = r#"USAGE:
    wasmer <SUBCOMMAND>

OPTIONS:
    -h, --help       Print help information

SUBCOMMANDS:
    run            Run a WebAssembly file. Formats accepted: wasm, wat
"#;

const HELP_RUN: &str = r#"USAGE:
    wasmer run <FILE> [ARGS]...

ARGS:
    <FILE>       File to run
    <ARGS>...    Application arguments
"#;

use crate::os::command::VirtualCommand;

#[derive(Debug, Clone)]
pub struct CmdWasmer {
    runtime: Arc<dyn WasiRuntimeImplementation + Send + Sync + 'static>,
    cache: Arc<ModuleCache>,
}

impl CmdWasmer {
    const NAME: &str = "wasmer";

    pub fn new(
        runtime: Arc<dyn WasiRuntimeImplementation + Send + Sync + 'static>,
        compiled_modules: Arc<ModuleCache>,
    ) -> Self {
        Self {
            runtime,
            cache: compiled_modules,
        }
    }
}

impl CmdWasmer {
    fn run<'a>(
        &self,
        parent_ctx: &FunctionEnvMut<'a, WasiEnv>,
        name: &str,
        store: &mut Option<Store>,
        config: &mut Option<SpawnOptions<WasiEnv>>,
        what: Option<String>,
        mut args: Vec<String>,
    ) -> wasmer_vbus::Result<BusSpawnedProcess> {
        if let Some(what) = what {
            let store = store.take().ok_or(VirtualBusError::UnknownError)?;
            let mut config = config.take().ok_or(VirtualBusError::UnknownError)?.conf();

            // Set the arguments of the environment by replacing the state
            let mut state = config.env().state.fork(true);
            args.insert(0, what.clone());
            state.args = args;
            config.env_mut().state = Arc::new(state);

            // Get the binary
            let tasks = parent_ctx.data().tasks();
            if let Some(binary) = self.get_package(what.clone(), tasks) {
                // Now run the module
                spawn_exec(binary, name, store, config, &self.runtime, &self.cache)
            } else {
                parent_ctx.data().tasks.clone().block_on(async move {
                    let _ = stderr_write(
                        parent_ctx,
                        format!("package not found - {}\r\n", what).as_bytes(),
                    )
                    .await;
                });
                Ok(BusSpawnedProcess::exited_process(Errno::Noent as u32))
            }
        } else {
            parent_ctx.data().tasks.clone().block_on(async move {
                let _ = stderr_write(parent_ctx, HELP_RUN.as_bytes()).await;
            });
            Ok(BusSpawnedProcess::exited_process(0))
        }
    }

    pub fn get_package(
        &self,
        name: String,
        tasks: &dyn VirtualTaskManager,
    ) -> Option<BinaryPackage> {
        self.cache
            .get_webc(name.as_str(), self.runtime.deref(), tasks)
    }
}

impl VirtualCommand for CmdWasmer {
    fn name(&self) -> &str {
        Self::NAME
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn exec<'a>(
        &self,
        parent_ctx: &FunctionEnvMut<'a, WasiEnv>,
        name: &str,
        store: &mut Option<Store>,
        config: &mut Option<SpawnOptions<WasiEnv>>,
    ) -> wasmer_vbus::Result<BusSpawnedProcess> {
        // Read the command we want to run
        let config_inner = config.as_ref().ok_or(VirtualBusError::UnknownError)?;
        let mut args = config_inner
            .conf_ref()
            .env()
            .state
            .args
            .iter()
            .map(|a| a.as_str());
        let _alias = args.next();
        let cmd = args.next();

        // Check the command
        match cmd {
            Some("run") => {
                let what = args.next().map(|a| a.to_string());
                let args = args.map(|a| a.to_string()).collect();
                self.run(parent_ctx, name, store, config, what, args)
            }
            Some("--help") | None => {
                parent_ctx.data().tasks.clone().block_on(async move {
                    let _ = stderr_write(parent_ctx, HELP.as_bytes()).await;
                });
                Ok(BusSpawnedProcess::exited_process(0))
            }
            Some(what) => {
                let what = Some(what.to_string());
                let args = args.map(|a| a.to_string()).collect();
                self.run(parent_ctx, name, store, config, what, args)
            }
        }
    }
}
