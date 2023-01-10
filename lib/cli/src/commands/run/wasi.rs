use crate::utils::{parse_envvar, parse_mapdir};
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::{collections::BTreeSet, path::Path};
use wasmer::{AsStoreMut, FunctionEnv, Instance, Module, RuntimeError, Value};
use wasmer_vfs::FileSystem;
use wasmer_vfs::{PassthruFileSystem, RootFileSystemBuilder, SpecialFile};
use wasmer_wasi::types::__WASI_STDIN_FILENO;
use wasmer_wasi::TtyFile;
use wasmer_wasi::{
    default_fs_backing, get_wasi_versions, PluggableRuntimeImplementation, WasiEnv, WasiError,
    WasiState, WasiVersion,
};

use clap::Parser;

#[derive(Debug, Parser, Clone, Default)]
/// WASI Options
pub struct Wasi {
    /// WASI pre-opened directory
    #[clap(long = "dir", name = "DIR", group = "wasi")]
    pub(crate) pre_opened_directories: Vec<PathBuf>,

    /// Map a host directory to a different location for the Wasm module
    #[clap(
        long = "mapdir",
        name = "GUEST_DIR:HOST_DIR",
        parse(try_from_str = parse_mapdir),
    )]
    pub(crate) mapped_dirs: Vec<(String, PathBuf)>,

    /// Pass custom environment variables
    #[clap(
        long = "env",
        name = "KEY=VALUE",
        parse(try_from_str = parse_envvar),
    )]
    pub(crate) env_vars: Vec<(String, String)>,

    /// List of other containers this module depends on
    #[clap(long = "use", name = "USE")]
    uses: Vec<String>,

    /// List of injected atoms
    #[clap(long = "map-command", name = "MAPCMD")]
    map_commands: Vec<String>,

    /// Enable experimental IO devices
    #[cfg(feature = "experimental-io-devices")]
    #[cfg_attr(
        feature = "experimental-io-devices",
        clap(long = "enable-experimental-io-devices")
    )]
    enable_experimental_io_devices: bool,

    /// Allow instances to send http requests.
    ///
    /// Access to domains is granted by default.
    #[clap(long)]
    pub http_client: bool,

    /// Allow WASI modules to import multiple versions of WASI without a warning.
    #[clap(long = "allow-multiple-wasi-versions")]
    pub allow_multiple_wasi_versions: bool,

    /// Require WASI modules to only import 1 version of WASI.
    #[clap(long = "deny-multiple-wasi-versions")]
    pub deny_multiple_wasi_versions: bool,
}

#[allow(dead_code)]
impl Wasi {
    pub fn map_dir(&mut self, alias: &str, target_on_disk: PathBuf) {
        self.mapped_dirs.push((alias.to_string(), target_on_disk));
    }

    pub fn set_env(&mut self, key: &str, value: &str) {
        self.env_vars.push((key.to_string(), value.to_string()));
    }

    /// Gets the WASI version (if any) for the provided module
    pub fn get_versions(module: &Module) -> Option<BTreeSet<WasiVersion>> {
        // Get the wasi version in strict mode, so no other imports are
        // allowed.
        get_wasi_versions(module, false)
    }

    /// Checks if a given module has any WASI imports at all.
    pub fn has_wasi_imports(module: &Module) -> bool {
        // Get the wasi version in non-strict mode, so no other imports
        // are allowed
        get_wasi_versions(module, false).is_some()
    }

    /// Helper function for instantiating a module with Wasi imports for the `Run` command.
    pub fn instantiate(
        &self,
        store: &mut impl AsStoreMut,
        module: &Module,
        program_name: String,
        args: Vec<String>,
    ) -> Result<(FunctionEnv<WasiEnv>, Instance)> {
        let args = args.iter().cloned().map(|arg| arg.into_bytes());

        let map_commands = self
            .map_commands
            .iter()
            .map(|map| map.split_once("=").unwrap())
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect::<HashMap<_, _>>();

        let runtime = Arc::new(PluggableRuntimeImplementation::default());

        let mut wasi_state_builder = WasiState::builder(program_name);
        wasi_state_builder
            .args(args)
            .envs(self.env_vars.clone())
            .uses(self.uses.clone())
            .runtime(&runtime)
            .map_commands(map_commands.clone());

        if wasmer_wasi::is_wasix_module(module) {
            // If we preopen anything from the host then shallow copy it over
            let root_fs = RootFileSystemBuilder::new()
                .with_tty(Box::new(TtyFile::new(
                    runtime.clone(),
                    Box::new(SpecialFile::new(__WASI_STDIN_FILENO)),
                )))
                .build();
            if self.mapped_dirs.len() > 0 {
                let fs_backing: Arc<dyn FileSystem + Send + Sync> =
                    Arc::new(PassthruFileSystem::new(default_fs_backing()));
                for (src, dst) in self.mapped_dirs.clone() {
                    let src = match src.starts_with("/") {
                        true => src,
                        false => format!("/{}", src),
                    };
                    root_fs.mount(PathBuf::from(src), &fs_backing, dst)?;
                }
            }

            // Open the root of the new filesystem
            wasi_state_builder
                .set_sandbox_fs(root_fs)
                .preopen_dir(Path::new("/"))
                .unwrap()
                .map_dir(".", "/")?;
        } else {
            wasi_state_builder
                .set_fs(default_fs_backing())
                .preopen_dirs(self.pre_opened_directories.clone())?
                .map_dirs(self.mapped_dirs.clone())?;
        }

        #[cfg(feature = "experimental-io-devices")]
        {
            if self.enable_experimental_io_devices {
                wasi_state_builder
                    .setup_fs(Box::new(wasmer_wasi_experimental_io_devices::initialize));
            }
        }

        let mut wasi_env = wasi_state_builder.finalize(store)?;

        if self.http_client {
            let caps = wasmer_wasi::http::HttpClientCapabilityV1::new_allow_all();
            wasi_env.data_mut(store).capabilities.http_client = caps;
        }

        let instance = wasmer_wasi::build_wasi_instance(module, &mut wasi_env, store)?;
        Ok((wasi_env.env, instance))
    }

    /// Helper function for handling the result of a Wasi _start function.
    pub fn handle_result(&self, result: Result<Box<[Value]>, RuntimeError>) -> Result<()> {
        match result {
            Ok(_) => Ok(()),
            Err(err) => {
                let err: anyhow::Error = match err.downcast::<WasiError>() {
                    Ok(WasiError::Exit(exit_code)) => {
                        // We should exit with the provided exit code
                        std::process::exit(exit_code as _);
                    }
                    Ok(err) => err.into(),
                    Err(err) => err.into(),
                };
                Err(err)
            }
        }
    }

    pub fn for_binfmt_interpreter() -> Result<Self> {
        use std::env;
        let dir = env::var_os("WASMER_BINFMT_MISC_PREOPEN")
            .map(Into::into)
            .unwrap_or_else(|| PathBuf::from("."));
        Ok(Self {
            deny_multiple_wasi_versions: true,
            env_vars: env::vars().collect(),
            pre_opened_directories: vec![dir],
            ..Self::default()
        })
    }
}
