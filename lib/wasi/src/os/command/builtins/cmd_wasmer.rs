use std::{any::Any, sync::Arc};

use crate::{vbus::BusSpawnedProcess, WasiRuntimeError};
use wasmer::{FunctionEnvMut, Store};

use crate::{
    syscalls::stderr_write, VirtualTaskManager, VirtualTaskManagerExt, WasiEnv, WasiRuntime,
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
}

impl CmdWasmer {
    const NAME: &str = "wasmer";

    pub fn new(runtime: Arc<dyn WasiRuntime + Send + Sync + 'static>) -> Self {
        Self { runtime }
    }
}

impl CmdWasmer {
    fn run<'a>(
        &self,
        parent_ctx: &FunctionEnvMut<'a, WasiEnv>,
        name: &str,
        store: Store,
        mut env: WasiEnv,
        what: Option<String>,
        mut args: Vec<String>,
    ) -> Result<BusSpawnedProcess, WasiRuntimeError> {
        if let Some(what) = what {
            // Set the arguments of the environment by replacing the state
            // TODO: why the fork here?
            let mut state = env.state.fork(true);
            args.insert(0, what.clone());
            state.args = args;
            env.state = Arc::new(state);

            // TODO: is this okay?
            env.runtime
                .task_manager()
                .runtime()
                .block_on(crate::bin_factory::spawn_exec_command(name, store, env))
        } else {
            parent_ctx.data().tasks().block_on(async move {
                let _ = stderr_write(parent_ctx, HELP_RUN.as_bytes()).await;
            });
            Ok(BusSpawnedProcess::exited_process(0))
        }
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
        store: Store,
        env: WasiEnv,
    ) -> Result<BusSpawnedProcess, WasiRuntimeError> {
        let mut args = env.state.args.iter().map(|a| a.as_str());
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
                Ok(BusSpawnedProcess::exited_process(0))
            }
            Some(what) => {
                let what = Some(what.to_string());
                let args = args.map(|a| a.to_string()).collect();
                self.run(parent_ctx, name, store, env, what, args)
            }
        }
    }
}
