use std::{any::Any, path::PathBuf, sync::Arc};

use crate::{
    bin_factory::spawn_exec_wasm,
    os::task::{OwnedTaskStatus, TaskJoinHandle},
    runtime::task_manager::InlineWaker,
    SpawnError,
};
use virtual_fs::{AsyncReadExt, FileSystem};
use wasmer::FunctionEnvMut;
use wasmer_package::utils::from_bytes;
use wasmer_wasix_types::wasi::Errno;

use crate::{
    bin_factory::{spawn_exec, BinaryPackage},
    syscalls::stderr_write,
    Runtime, WasiEnv,
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
    runtime: Arc<dyn Runtime + Send + Sync + 'static>,
}

impl CmdWasmer {
    const NAME: &'static str = "wasmer";

    pub fn new(runtime: Arc<dyn Runtime + Send + Sync + 'static>) -> Self {
        Self { runtime }
    }
}

impl CmdWasmer {
    async fn run<'a>(
        &self,
        parent_ctx: &FunctionEnvMut<'a, WasiEnv>,
        name: &str,
        config: &mut Option<WasiEnv>,
        what: Option<String>,
        mut args: Vec<String>,
    ) -> Result<TaskJoinHandle, SpawnError> {
        pub enum Executable {
            Wasm(bytes::Bytes),
            BinaryPackage(BinaryPackage),
        }

        // If the first argument is a '--' then skip it
        if args.first().map(|a| a.as_str()) == Some("--") {
            args = args.into_iter().skip(1).collect();
        }

        if let Some(what) = what {
            let mut env = config.take().ok_or(SpawnError::UnknownError)?;

            // Set the arguments of the environment by replacing the state
            let mut state = env.state.fork();
            args.insert(0, what.clone());
            state.args = std::sync::Mutex::new(args);
            env.state = Arc::new(state);

            let file_path = if what.starts_with('/') {
                PathBuf::from(&what)
            } else {
                // convert relative path to absolute path
                let cwd = env.state.fs.current_dir.lock().unwrap().clone();

                PathBuf::from(cwd).join(&what)
            };

            let fs = env.fs_root();
            let f = fs.new_open_options().read(true).open(&file_path);
            let executable = if let Ok(mut file) = f {
                let mut data = Vec::with_capacity(file.size() as usize);
                file.read_to_end(&mut data).await.unwrap();

                let bytes: bytes::Bytes = data.into();

                if let Ok(container) = from_bytes(bytes.clone()) {
                    let pkg = BinaryPackage::from_webc(&container, &*self.runtime)
                        .await
                        .unwrap();

                    Executable::BinaryPackage(pkg)
                } else {
                    Executable::Wasm(bytes)
                }
            } else if let Ok(pkg) = self.get_package(&what).await {
                Executable::BinaryPackage(pkg)
            } else {
                let _ = unsafe { stderr_write(parent_ctx, HELP_RUN.as_bytes()) }.await;
                let handle =
                    OwnedTaskStatus::new_finished_with_code(Errno::Success.into()).handle();
                return Ok(handle);
            };

            match executable {
                Executable::BinaryPackage(binary) => {
                    // Infer the command that is going to be executed
                    let cmd_name: &str =
                        binary
                            .infer_entrypoint()
                            .map_err(|_| SpawnError::MissingEntrypoint {
                                package_id: binary.id.clone(),
                            })?;

                    let cmd = binary
                        .get_command(cmd_name)
                        .ok_or_else(|| SpawnError::NotFound {
                            message: format!("{cmd_name} command in package: {}", binary.id),
                        })?;

                    env.prepare_spawn(cmd);

                    env.use_package_async(&binary).await.unwrap();

                    // Now run the module
                    spawn_exec(binary, name, env, &self.runtime).await
                }
                Executable::Wasm(bytes) => spawn_exec_wasm(&bytes, name, env, &self.runtime).await,
            }
        } else {
            let _ = unsafe { stderr_write(parent_ctx, HELP_RUN.as_bytes()) }.await;
            let handle = OwnedTaskStatus::new_finished_with_code(Errno::Success.into()).handle();
            Ok(handle)
        }
    }

    pub async fn get_package(&self, name: &str) -> Result<BinaryPackage, anyhow::Error> {
        // Need to make sure this task runs on the main runtime.
        let (tx, rx) = tokio::sync::oneshot::channel();
        let specifier = name.parse()?;
        let rt = self.runtime.clone();
        self.runtime.task_manager().task_shared(Box::new(|| {
            Box::pin(async move {
                let res = BinaryPackage::from_registry(&specifier, rt.as_ref()).await;
                tx.send(res)
                    .expect("could not send response to output channel");
            })
        }))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("package retrieval response channel died"))?
    }
}

impl VirtualCommand for CmdWasmer {
    fn name(&self) -> &str {
        Self::NAME
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn exec(
        &self,
        parent_ctx: &FunctionEnvMut<'_, WasiEnv>,
        name: &str,
        env: &mut Option<WasiEnv>,
    ) -> Result<TaskJoinHandle, SpawnError> {
        // Read the command we want to run
        let env_inner = env.as_ref().ok_or(SpawnError::UnknownError)?;
        let args = env_inner.state.args.lock().unwrap().clone();
        let mut args = args.iter().map(|s| s.as_str());
        let _alias = args.next();
        let cmd = args.next();

        // Check the command
        let fut = async {
            match cmd {
                Some("run") => {
                    let what = args.next().map(|a| a.to_string());
                    let args = args.map(|a| a.to_string()).collect();
                    self.run(parent_ctx, name, env, what, args).await
                }
                Some("--help") | None => {
                    unsafe { stderr_write(parent_ctx, HELP.as_bytes()) }
                        .await
                        .ok();
                    let handle =
                        OwnedTaskStatus::new_finished_with_code(Errno::Success.into()).handle();
                    Ok(handle)
                }
                Some(what) => {
                    let what = Some(what.to_string());
                    let args = args.map(|a| a.to_string()).collect();
                    self.run(parent_ctx, name, env, what, args).await
                }
            }
        };

        InlineWaker::block_on(fut)
    }
}
