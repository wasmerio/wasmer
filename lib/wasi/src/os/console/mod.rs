#![allow(unused_imports)]
#![allow(dead_code)]

pub mod cconst;

use std::{
    collections::HashMap,
    io::Write,
    ops::{Deref, DerefMut},
    path::Path,
    sync::{atomic::AtomicBool, Arc, Mutex},
};

use derivative::*;
use linked_hash_set::LinkedHashSet;
use tokio::sync::{mpsc, RwLock};
#[allow(unused_imports, dead_code)]
use tracing::{debug, error, info, trace, warn};
#[cfg(feature = "sys")]
use wasmer::Engine;
use wasmer_vbus::{BusSpawnedProcess, SpawnOptionsConfig};
use wasmer_vfs::{FileSystem, RootFileSystemBuilder, SpecialFile, WasiPipe};
use wasmer_wasi_types::types::__WASI_STDIN_FILENO;

use super::{cconst::ConsoleConst, common::*};
use crate::{
    bin_factory::{spawn_exec, BinFactory, ModuleCache},
    os::task::{control_plane::WasiControlPlane, process::WasiProcess},
    runtime::{RuntimeStderr, RuntimeStdout},
    state::Capabilities,
    TtyFile, VirtualTaskManagerExt, WasiEnv, WasiRuntimeImplementation, WasiState,
};

//pub const DEFAULT_BOOT_WEBC: &'static str = "sharrattj/bash";
pub const DEFAULT_BOOT_WEBC: &'static str = "sharrattj/dash";
//pub const DEFAULT_BOOT_USES: [&'static str; 2] = [ "sharrattj/coreutils", "sharrattj/catsay" ];
pub const DEFAULT_BOOT_USES: [&'static str; 0] = [];

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
    runtime: Arc<dyn WasiRuntimeImplementation + Send + Sync + 'static>,
    compiled_modules: Arc<ModuleCache>,
    stdin: Option<WasiPipe>,
    capabilities: Capabilities,
}

impl Console {
    pub fn new(
        runtime: Arc<dyn WasiRuntimeImplementation + Send + Sync + 'static>,
        compiled_modules: Arc<ModuleCache>,
    ) -> Self {
        let mut uses = DEFAULT_BOOT_USES
            .iter()
            .map(|a| a.to_string())
            .collect::<LinkedHashSet<_>>();
        let prog = DEFAULT_BOOT_WEBC
            .split_once(" ")
            .map(|a| a.1)
            .unwrap_or(DEFAULT_BOOT_WEBC);
        uses.insert(prog.to_string());
        Self {
            boot_cmd: DEFAULT_BOOT_WEBC.to_string(),
            uses,
            is_mobile: false,
            is_ssh: false,
            user_agent: None,
            whitelabel: false,
            token: None,
            no_welcome: false,
            env: HashMap::new(),
            runtime,
            prompt: "wasmer.sh".to_string(),
            compiled_modules,
            stdin: None,
            capabilities: Default::default(),
        }
    }

    pub fn with_stdin(mut self, stdin: WasiPipe) -> Self {
        self.stdin = Some(stdin);
        self
    }

    pub fn with_prompt(mut self, prompt: String) -> Self {
        self.prompt = prompt;
        self
    }

    pub fn with_boot_cmd(mut self, cmd: String) -> Self {
        let prog = cmd.split_once(" ").map(|a| a.0).unwrap_or(cmd.as_str());
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

    pub fn run(&mut self) -> wasmer_vbus::Result<(BusSpawnedProcess, WasiProcess)> {
        // Extract the program name from the arguments
        let empty_args: Vec<&[u8]> = Vec::new();
        let (webc, prog, args) = match self.boot_cmd.split_once(" ") {
            Some((webc, args)) => (
                webc,
                webc.split_once("/").map(|a| a.1).unwrap_or(webc),
                args.split(" ").map(|a| a.as_bytes()).collect::<Vec<_>>(),
            ),
            None => (
                self.boot_cmd.as_str(),
                self.boot_cmd
                    .split_once("/")
                    .map(|a| a.1)
                    .unwrap_or(self.boot_cmd.as_str()),
                empty_args,
            ),
        };
        let envs = self.env.clone();

        // Build a new store that will be passed to the thread
        let store = self.runtime.new_store();

        // Create the control plane, process and thread
        let control_plane = WasiControlPlane::default();
        let wasi_process = control_plane.new_process();
        let wasi_thread = wasi_process.new_thread();

        // Create the state
        let mut state = WasiState::builder(prog);
        if let Some(stdin) = self.stdin.take() {
            state.stdin(Box::new(stdin));
        }

        // If we preopen anything from the host then shallow copy it over
        let root_fs = RootFileSystemBuilder::new()
            .with_tty(Box::new(TtyFile::new(
                self.runtime.clone(),
                Box::new(SpecialFile::new(__WASI_STDIN_FILENO)),
            )))
            .build();

        // Open the root
        state
            .args(args.iter())
            .envs(envs.iter())
            .set_sandbox_fs(root_fs)
            .preopen_dir(Path::new("/"))
            .unwrap()
            .map_dir(".", "/")
            .unwrap();

        let state = state
            .stdout(Box::new(RuntimeStdout::new(self.runtime.clone())))
            .stderr(Box::new(RuntimeStderr::new(self.runtime.clone())))
            .build()
            .unwrap();

        // Create the environment
        let mut env = WasiEnv::new_ext(
            Arc::new(state),
            self.compiled_modules.clone(),
            wasi_process.clone(),
            wasi_thread,
            self.runtime.clone(),
        );
        env.capabilities = self.capabilities.clone();

        // Display the welcome message
        let tasks = env.tasks.clone();
        if self.whitelabel == false && self.no_welcome == false {
            tasks.block_on(self.draw_welcome());
        }

        let binary = if let Some(binary) =
            self.compiled_modules
                .get_webc(webc, self.runtime.deref(), env.tasks.deref())
        {
            binary
        } else {
            tasks.block_on(async {
                let _ = self
                    .runtime
                    .stderr(format!("package not found [{}]\r\n", webc).as_bytes())
                    .await;
            });
            tracing::debug!("failed to get webc dependency - {}", webc);
            return Err(wasmer_vbus::VirtualBusError::NotFound);
        };

        if let Err(err) = env.uses(self.uses.clone()) {
            tasks.block_on(async {
                let _ = self.runtime.stderr(format!("{}\r\n", err).as_bytes()).await;
            });
            tracing::debug!("failed to load used dependency - {}", err);
            return Err(wasmer_vbus::VirtualBusError::BadRequest);
        }

        // Build the config
        let config = SpawnOptionsConfig {
            reuse: false,
            env,
            remote_instance: None,
            access_token: self.token.clone(),
        };

        // Run the binary
        let process = spawn_exec(
            binary,
            prog,
            store,
            config,
            &self.runtime,
            self.compiled_modules.as_ref(),
        )?;

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

        let _ = self.runtime.stdout(data.as_str().as_bytes()).await;
    }
}
