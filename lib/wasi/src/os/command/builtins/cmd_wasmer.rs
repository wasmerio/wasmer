use std::{any::Any, ops::Deref, sync::Arc};

use crate::{
    os::task::{OwnedTaskStatus, TaskJoinHandle},
    VirtualBusError,
};
use wasmer::{FunctionEnvMut, Store};
use wasmer_wasi_types::wasi::Errno;

use crate::{
    bin_factory::{spawn_exec, BinaryPackage, ModuleCache},
    syscalls::stderr_write,
    VirtualTaskManagerExt, WasiEnv, WasiRuntime,
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
    runtime: Arc<dyn WasiRuntime + Send + Sync + 'static>,
    cache: Arc<ModuleCache>,
}

impl CmdWasmer {
    const NAME: &str = "wasmer";

    pub fn new(
        runtime: Arc<dyn WasiRuntime + Send + Sync + 'static>,
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
        config: &mut Option<WasiEnv>,
        what: Option<String>,
        mut args: Vec<String>,
    ) -> Result<TaskJoinHandle, VirtualBusError> {
        if let Some(what) = what {
            let store = store.take().ok_or(VirtualBusError::UnknownError)?;
            let mut env = config.take().ok_or(VirtualBusError::UnknownError)?;

            // Set the arguments of the environment by replacing the state
            let mut state = env.state.fork();
            args.insert(0, what.clone());
            state.args = args;
            env.state = Arc::new(state);

            // Get the binary
            if let Some(binary) = self.get_package(what.clone()) {
                // Now run the module
                spawn_exec(binary, name, store, env, &self.runtime, &self.cache)
            } else {
                parent_ctx.data().tasks().block_on(async move {
                    let _ = stderr_write(
                        parent_ctx,
                        format!("package not found - {}\r\n", what).as_bytes(),
                    )
                    .await;
                });
                let handle = OwnedTaskStatus::new_finished_with_code(Errno::Noent as u32).handle();
                Ok(handle)
            }
        } else {
            parent_ctx.data().tasks().block_on(async move {
                let _ = stderr_write(parent_ctx, HELP_RUN.as_bytes()).await;
            });
            let handle = OwnedTaskStatus::new_finished_with_code(0).handle();
            Ok(handle)
        }
    }

    pub fn get_package(&self, name: String) -> Option<BinaryPackage> {
        self.cache.get_webc(name.as_str(), self.runtime.deref())
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
        env: &mut Option<WasiEnv>,
    ) -> Result<TaskJoinHandle, VirtualBusError> {
        // Read the command we want to run
        let env_inner = env.as_ref().ok_or(VirtualBusError::UnknownError)?;
        let mut args = env_inner.state.args.iter().map(|a| a.as_str());
        let _alias = args.next();
        let cmd = args.next();

        // Check the command
        match cmd {
            Some("run") => {
                let what = args.next().map(|a| a.to_string());
                let args = args.map(|a| a.to_string()).collect();
                self.run(parent_ctx, name, store, env, what, args)
            }
            Some("--help") | None => {
                parent_ctx.data().tasks().block_on(async move {
                    let _ = stderr_write(parent_ctx, HELP.as_bytes()).await;
                });
                let handle = OwnedTaskStatus::new_finished_with_code(0).handle();
                Ok(handle)
            }
            Some(what) => {
                let what = Some(what.to_string());
                let args = args.map(|a| a.to_string()).collect();
                self.run(parent_ctx, name, store, env, what, args)
            }
        }
    }
}
