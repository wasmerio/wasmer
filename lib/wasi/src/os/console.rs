#![allow(unused_imports)]
#![allow(dead_code)]
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use derivative::*;
use linked_hash_set::LinkedHashSet;
use tokio::sync::mpsc;
use tokio::sync::RwLock;
#[allow(unused_imports, dead_code)]
use tracing::{debug, error, info, trace, warn};
#[cfg(feature = "sys")]
use wasmer::Engine;
use wasmer_vbus::{SpawnOptionsConfig, BusSpawnedProcess};
use wasmer_vfs::FileSystem;

use crate::{WasiControlPlane, WasiEnv, WasiProcess, WasiState};
use crate::WasiRuntimeImplementation;
use crate::bin_factory::BinFactory;
use crate::bin_factory::CachedCompiledModules;
use crate::bin_factory::spawn_exec;
use crate::WasiPipe;
use crate::runtime::RuntimeStdout;
use crate::runtime::RuntimeStderr;

use super::common::*;
use super::posix_err;
use super::cconst::ConsoleConst;

//pub const DEFAULT_BOOT_WEBC: &'static str = "sharrattj/bash";
pub const DEFAULT_BOOT_WEBC: &'static str = "sharrattj/dash";
//pub const DEFAULT_BOOT_USES: [&'static str; 2] = [ "sharrattj/coreutils", "sharrattj/catsay" ];
pub const DEFAULT_BOOT_USES: [&'static str; 0] = [ ];

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
    compiled_modules: Arc<CachedCompiledModules>,
    stdin: Option<WasiPipe>,
}

impl Console {
    pub fn new(
        runtime: Arc<dyn WasiRuntimeImplementation + Send + Sync + 'static>,
        compiled_modules: Arc<CachedCompiledModules>,
    ) -> Self {
        let mut uses = DEFAULT_BOOT_USES.iter().map(|a| a.to_string()).collect::<LinkedHashSet<_>>();
        let prog = DEFAULT_BOOT_WEBC.split_once(" ").map(|a| a.1).unwrap_or(DEFAULT_BOOT_WEBC);
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

    pub fn run(&mut self) -> wasmer_vbus::Result<BusSpawnedProcess>
    {
        // Extract the program name from the arguments
        let empty_args: Vec<&[u8]> = Vec::new();
        let (webc, prog, args) = match self.boot_cmd.split_once(" ") {
            Some((webc, args)) => {
                (
                    webc,
                    webc.split_once("/").map(|a| a.1).unwrap_or(webc),
                    args.split(" ").map(|a| a.as_bytes()).collect::<Vec<_>>()
                )
            },
            None => {
                (
                    self.boot_cmd.as_str(),
                    self.boot_cmd.split_once("/").map(|a| a.1).unwrap_or(self.boot_cmd.as_str()),
                    empty_args
                )
            }
        };
        let envs = self.env.clone();

        // Display the welcome message
        if self.whitelabel == false && self.no_welcome == false {
            self.draw_welcome();
        }

        // Build a new store that will be passed to the thread
        let store = self.compiled_modules.new_store();

        // Create the control plane, process and thread
        let control_plane = WasiControlPlane::default();
        let process = control_plane.new_process();
        let thread = process.new_thread();

        // Create the state
        let mut state = WasiState::new(prog);
        if let Some(stdin) = self.stdin.take() {
            state.stdin(Box::new(stdin));
        }

        // Open the root
        state
            .args(args.iter())
            .envs(envs.iter())
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
        let env = WasiEnv::new_ext(
            Arc::new(state),
            self.compiled_modules.clone(),
            process,
            thread,
            self.runtime.clone()
        );
        
        // Find the binary
        if let Some(binary) = self.compiled_modules.get_webc(webc, self.runtime.deref(), env.tasks.deref())
        {
            if let Err(err) = env.uses(self.uses.clone()) {
                let _ = self.runtime.stderr(
                    format!("{}\r\n", err).as_bytes()
                );
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
                self.compiled_modules.as_ref()
            ).unwrap();

            // Return the process
            Ok(process)
        } else {
            let _ = self.runtime.stderr(
                format!("package not found [{}]\r\n", self.boot_cmd).as_bytes()
            );
            Err(wasmer_vbus::VirtualBusError::NotFound)
        }
    }

    pub fn draw_welcome(&self) {
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

        let _ = self.runtime.stdout(data.as_str().as_bytes());
    }
}
