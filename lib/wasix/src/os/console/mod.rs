#![allow(unused_imports)]
#![allow(dead_code)]

pub mod cconst;

use std::{
    borrow::Cow,
    collections::HashMap,
    io::Write,
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc, Mutex},
};

use derivative::*;
use linked_hash_set::LinkedHashSet;
use tokio::sync::{mpsc, RwLock};
#[allow(unused_imports, dead_code)]
use tracing::{debug, error, info, trace, warn};
use virtual_fs::{
    ArcBoxFile, ArcFile, AsyncWriteExt, CombineFile, DeviceFile, DuplexPipe, FileSystem, Pipe,
    PipeRx, PipeTx, RootFileSystemBuilder, StaticFile, VirtualFile,
};
#[cfg(feature = "sys")]
use wasmer::Engine;
use wasmer_wasix_types::{types::__WASI_STDIN_FILENO, wasi::Errno};

use super::{cconst::ConsoleConst, common::*, task::TaskJoinHandle};
use crate::{
    bin_factory::{spawn_exec, BinFactory, BinaryPackage},
    capabilities::Capabilities,
    os::task::{control_plane::WasiControlPlane, process::WasiProcess},
    runtime::{resolver::PackageSpecifier, task_manager::InlineWaker},
    Runtime, SpawnError, WasiEnv, WasiEnvBuilder, WasiRuntimeError,
};

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Console {
    user_agent: Option<String>,
    boot_cmd: String,
    uses: LinkedHashSet<String>,
    is_mobile: bool,
    is_ssh: bool,
    whitelabel: bool,
    token: Option<String>,
    no_welcome: bool,
    prompt: String,
    env: HashMap<String, String>,
    runtime: Arc<dyn Runtime + Send + Sync>,
    stdin: ArcBoxFile,
    stdout: ArcBoxFile,
    stderr: ArcBoxFile,
    capabilities: Capabilities,
    ro_files: HashMap<String, Cow<'static, [u8]>>,
    memfs_memory_limiter: Option<virtual_fs::limiter::DynFsMemoryLimiter>,
}

impl Console {
    pub fn new(webc_boot_package: &str, runtime: Arc<dyn Runtime + Send + Sync + 'static>) -> Self {
        Self {
            boot_cmd: webc_boot_package.to_string(),
            uses: LinkedHashSet::new(),
            is_mobile: false,
            is_ssh: false,
            user_agent: None,
            whitelabel: false,
            token: None,
            no_welcome: false,
            env: HashMap::new(),
            runtime,
            prompt: "wasmer.sh".to_string(),
            stdin: ArcBoxFile::new(Box::new(Pipe::channel().0)),
            stdout: ArcBoxFile::new(Box::new(Pipe::channel().0)),
            stderr: ArcBoxFile::new(Box::new(Pipe::channel().0)),
            capabilities: Default::default(),
            memfs_memory_limiter: None,
            ro_files: Default::default(),
        }
    }

    pub fn with_prompt(mut self, prompt: String) -> Self {
        self.prompt = prompt;
        self
    }

    pub fn with_boot_cmd(mut self, cmd: String) -> Self {
        let prog = cmd.split_once(' ').map(|a| a.0).unwrap_or(cmd.as_str());
        self.uses.insert(prog.to_string());
        self.boot_cmd = cmd;
        self
    }

    pub fn with_uses(mut self, uses: Vec<String>) -> Self {
        self.uses = uses.into_iter().collect();
        self
    }

    pub fn with_env(mut self, env: HashMap<String, String>) -> Self {
        self.env = env;
        self
    }

    pub fn with_user_agent(mut self, user_agent: &str) -> Self {
        self.is_mobile = is_mobile(user_agent);
        self.is_ssh = is_ssh(user_agent);
        self.user_agent = Some(user_agent.to_string());
        self
    }

    pub fn with_no_welcome(mut self, no_welcome: bool) -> Self {
        self.no_welcome = no_welcome;
        self
    }

    pub fn with_token(mut self, token: String) -> Self {
        self.token = Some(token);
        self
    }

    pub fn with_capabilities(mut self, caps: Capabilities) -> Self {
        self.capabilities = caps;
        self
    }

    pub fn with_stdin(mut self, stdin: Box<dyn VirtualFile + Send + Sync + 'static>) -> Self {
        self.stdin = ArcBoxFile::new(stdin);
        self
    }

    pub fn with_stdout(mut self, stdout: Box<dyn VirtualFile + Send + Sync + 'static>) -> Self {
        self.stdout = ArcBoxFile::new(stdout);
        self
    }

    pub fn with_stderr(mut self, stderr: Box<dyn VirtualFile + Send + Sync + 'static>) -> Self {
        self.stderr = ArcBoxFile::new(stderr);
        self
    }

    pub fn with_ro_files(mut self, ro_files: HashMap<String, Cow<'static, [u8]>>) -> Self {
        self.ro_files = ro_files;
        self
    }

    pub fn with_mem_fs_memory_limiter(
        mut self,
        limiter: virtual_fs::limiter::DynFsMemoryLimiter,
    ) -> Self {
        self.memfs_memory_limiter = Some(limiter);
        self
    }

    pub fn run(&mut self) -> Result<(TaskJoinHandle, WasiProcess), SpawnError> {
        // Extract the program name from the arguments
        let empty_args: Vec<&str> = Vec::new();
        let (webc, prog, args) = match self.boot_cmd.split_once(' ') {
            Some((webc, args)) => (
                webc,
                webc.split_once('/').map(|a| a.1).unwrap_or(webc),
                args.split(' ').collect::<Vec<_>>(),
            ),
            None => (
                self.boot_cmd.as_str(),
                self.boot_cmd
                    .split_once('/')
                    .map(|a| a.1)
                    .unwrap_or(self.boot_cmd.as_str()),
                empty_args,
            ),
        };

        let webc_ident: PackageSpecifier = match webc.parse() {
            Ok(ident) => ident,
            Err(e) => {
                tracing::debug!(webc, error = &*e, "Unable to parse the WEBC identifier");
                return Err(SpawnError::BadRequest);
            }
        };

        let resolved_package = InlineWaker::block_on(BinaryPackage::from_registry(
            &webc_ident,
            self.runtime.as_ref(),
        ));

        let pkg = match resolved_package {
            Ok(pkg) => pkg,
            Err(e) => {
                let mut stderr = self.stderr.clone();
                InlineWaker::block_on(async {
                    let mut buffer = Vec::new();
                    writeln!(buffer, "Error: {e}").ok();
                    let mut source = e.source();
                    while let Some(s) = source {
                        writeln!(buffer, "  Caused by: {s}").ok();
                        source = s.source();
                    }

                    virtual_fs::AsyncWriteExt::write_all(&mut stderr, &buffer)
                        .await
                        .ok();
                });
                tracing::debug!("failed to get webc dependency - {}", webc);
                return Err(SpawnError::NotFound);
            }
        };

        let wasi_opts = webc::metadata::annotations::Wasi::new(prog);

        let root_fs = RootFileSystemBuilder::new()
            .with_tty(Box::new(CombineFile::new(
                Box::new(self.stdout.clone()),
                Box::new(self.stdin.clone()),
            )))
            .build();

        if let Some(limiter) = &self.memfs_memory_limiter {
            root_fs.set_memory_limiter(limiter.clone());
        }

        let builder = crate::runners::wasi::WasiRunner::new()
            .with_envs(self.env.clone().into_iter())
            .with_args(args)
            .with_capabilities(self.capabilities.clone())
            .with_stdin(Box::new(self.stdin.clone()))
            .with_stdout(Box::new(self.stdout.clone()))
            .with_stderr(Box::new(self.stderr.clone()))
            .prepare_webc_env(
                prog,
                &wasi_opts,
                Some(&pkg),
                self.runtime.clone(),
                Some(root_fs),
            )
            // TODO: better error conversion
            .map_err(|err| SpawnError::Other(err.into()))?;

        let env = builder.build()?;

        // Display the welcome message
        if !self.whitelabel && !self.no_welcome {
            InlineWaker::block_on(self.draw_welcome());
        }

        let wasi_process = env.process.clone();

        if let Err(err) = env.uses(self.uses.clone()) {
            let mut stderr = self.stderr.clone();
            InlineWaker::block_on(async {
                virtual_fs::AsyncWriteExt::write_all(
                    &mut stderr,
                    format!("{}\r\n", err).as_bytes(),
                )
                .await
                .ok();
            });
            tracing::debug!("failed to load used dependency - {}", err);
            return Err(SpawnError::BadRequest);
        }

        // The custom readonly files have to be added after the uses packages
        // otherwise they will be overriden by their attached file systems
        for (path, data) in self.ro_files.clone() {
            let path = PathBuf::from(path);
            env.fs_root().remove_file(&path).ok();
            let mut file = env
                .fs_root()
                .new_open_options()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&path)
                .map_err(|err| SpawnError::Other(err.into()))?;
            InlineWaker::block_on(file.copy_reference(Box::new(StaticFile::new(data))))
                .map_err(|err| SpawnError::Other(err.into()))?;
        }

        // Build the config
        // Run the binary
        let store = self.runtime.new_store();
        let process = InlineWaker::block_on(spawn_exec(pkg, prog, store, env, &self.runtime))?;

        // Return the process
        Ok((process, wasi_process))
    }

    pub async fn draw_welcome(&self) {
        let welcome = match (self.is_mobile, self.is_ssh) {
            (true, _) => ConsoleConst::WELCOME_MEDIUM,
            (_, true) => ConsoleConst::WELCOME_SMALL,
            (_, _) => ConsoleConst::WELCOME_LARGE,
        };
        let mut data = welcome
            .replace("\\x1B", "\x1B")
            .replace("\\r", "\r")
            .replace("\\n", "\n");
        data.insert_str(0, ConsoleConst::TERM_NO_WRAPAROUND);

        let mut stderr = self.stderr.clone();
        virtual_fs::AsyncWriteExt::write_all(&mut stderr, data.as_str().as_bytes())
            .await
            .ok();
    }
}

#[cfg(all(test, not(target_family = "wasm")))]
mod tests {
    use virtual_fs::{AsyncSeekExt, BufferFile, Pipe};

    use super::*;

    use std::{io::Read, sync::Arc};

    use crate::{
        runtime::{package_loader::BuiltinPackageLoader, task_manager::tokio::TokioTaskManager},
        PluggableRuntime,
    };

    /// Test that [`Console`] correctly runs a command with arguments and
    /// specified env vars, and that the TTY correctly handles stdout output.
    ///
    /// Note that this test currently aborts the process unconditionally due
    /// to a misaligned pointer access in stack_checkpoint() triggering a panic
    /// in a function that isn't allowed to unwind.
    ///
    /// See [#4284](https://github.com/wasmerio/wasmer/issues/4284) for more.
    #[test]
    #[cfg_attr(not(feature = "host-reqwest"), ignore = "Requires a HTTP client")]
    #[ignore = "Unconditionally aborts (CC #4284)"]
    fn test_console_dash_tty_with_args_and_env() {
        let tokio_rt = tokio::runtime::Runtime::new().unwrap();
        let rt_handle = tokio_rt.handle().clone();
        let _guard = rt_handle.enter();

        let tm = TokioTaskManager::new(tokio_rt);
        let mut rt = PluggableRuntime::new(Arc::new(tm));
        let client = rt.http_client().unwrap().clone();
        rt.set_engine(Some(wasmer::Engine::default()))
            .set_package_loader(BuiltinPackageLoader::new().with_shared_http_client(client));

        let env: HashMap<String, String> = [("MYENV1".to_string(), "VAL1".to_string())]
            .into_iter()
            .collect();

        // Pass some arguments.
        let cmd = "sharrattj/dash -s stdin";

        let (mut stdin_tx, stdin_rx) = Pipe::channel();
        let (stdout_tx, mut stdout_rx) = Pipe::channel();

        let (mut handle, _proc) = Console::new(cmd, Arc::new(rt))
            .with_env(env)
            .with_stdin(Box::new(stdin_rx))
            .with_stdout(Box::new(stdout_tx))
            .run()
            .unwrap();

        let code = rt_handle
            .block_on(async move {
                virtual_fs::AsyncWriteExt::write_all(
                    &mut stdin_tx,
                    b"echo hello $MYENV1 > /dev/tty; exit\n",
                )
                .await?;

                stdin_tx.close();
                std::mem::drop(stdin_tx);

                let res = handle.wait_finished().await?;
                Ok::<_, anyhow::Error>(res)
            })
            .unwrap();

        assert_eq!(code.raw(), 78);

        let mut out = String::new();
        stdout_rx.read_to_string(&mut out).unwrap();

        assert_eq!(out, "hello VAL1\n");
    }

    /// Regression test to ensure merging of multiple packages works correctly.
    #[test]
    fn test_console_python_merge() {
        let tokio_rt = tokio::runtime::Runtime::new().unwrap();
        let rt_handle = tokio_rt.handle().clone();
        let _guard = rt_handle.enter();

        let tm = TokioTaskManager::new(tokio_rt);
        let mut rt = PluggableRuntime::new(Arc::new(tm));
        let client = rt.http_client().unwrap().clone();
        rt.set_engine(Some(wasmer::Engine::default()))
            .set_package_loader(BuiltinPackageLoader::new().with_shared_http_client(client));

        let cmd = "wasmer-tests/python-env-dump --help";

        let (mut handle, _proc) = Console::new(cmd, Arc::new(rt)).run().unwrap();

        let code = rt_handle
            .block_on(async move {
                let res = handle.wait_finished().await?;
                Ok::<_, anyhow::Error>(res)
            })
            .unwrap();

        assert_eq!(code.raw(), 0);
    }
}
