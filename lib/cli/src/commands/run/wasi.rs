use crate::utils::{parse_envvar, parse_mapdir};
use anyhow::Result;
use std::collections::BTreeSet;
use std::path::PathBuf;
use wasmer::{Instance, Module, RuntimeError, Val};
use wasmer_wasi::{get_wasi_versions, WasiError, WasiState, WasiVersion};

use structopt::StructOpt;

#[derive(Debug, StructOpt, Clone, Default)]
/// WASI Options
pub struct Wasi {
    /// WASI pre-opened directory
    #[structopt(
        long = "dir",
        name = "DIR",
        multiple = true,
        group = "wasi",
        number_of_values = 1
    )]
    pre_opened_directories: Vec<PathBuf>,

    /// Map a host directory to a different location for the Wasm module
    #[structopt(
        long = "mapdir",
        name = "GUEST_DIR:HOST_DIR",
        multiple = true,
        parse(try_from_str = parse_mapdir),
        number_of_values = 1,
    )]
    mapped_dirs: Vec<(String, PathBuf)>,

    /// Pass custom environment variables
    #[structopt(
        long = "env",
        name = "KEY=VALUE",
        multiple = true,
        parse(try_from_str = parse_envvar),
    )]
    env_vars: Vec<(String, String)>,

    /// Enable experimental IO devices
    #[cfg(feature = "experimental-io-devices")]
    #[structopt(long = "enable-experimental-io-devices")]
    enable_experimental_io_devices: bool,

    /// Allow WASI modules to import multiple versions of WASI without a warning.
    #[structopt(long = "allow-multiple-wasi-versions")]
    pub allow_multiple_wasi_versions: bool,

    /// Require WASI modules to only import 1 version of WASI.
    #[structopt(long = "deny-multiple-wasi-versions")]
    pub deny_multiple_wasi_versions: bool,
}

#[allow(dead_code)]
impl Wasi {
    /// Gets the WASI version (if any) for the provided module
    pub fn get_versions(module: &Module) -> Option<BTreeSet<WasiVersion>> {
        // Get the wasi version in strict mode, so no other imports are
        // allowed.
        get_wasi_versions(&module, true)
    }

    /// Checks if a given module has any WASI imports at all.
    pub fn has_wasi_imports(module: &Module) -> bool {
        // Get the wasi version in non-strict mode, so no other imports
        // are allowed
        get_wasi_versions(&module, false).is_some()
    }

    /// Helper function for instantiating a module with Wasi imports for the `Run` command.
    pub fn instantiate(
        &self,
        module: &Module,
        program_name: String,
        args: Vec<String>,
    ) -> Result<Instance> {
        let args = args.iter().cloned().map(|arg| arg.into_bytes());

        let mut wasi_state_builder = WasiState::new(program_name);
        wasi_state_builder
            .args(args)
            .envs(self.env_vars.clone())
            .preopen_dirs(self.pre_opened_directories.clone())?
            .map_dirs(self.mapped_dirs.clone())?;

        #[cfg(feature = "experimental-io-devices")]
        {
            if self.enable_experimental_io_devices {
                wasi_state_builder
                    .setup_fs(Box::new(wasmer_wasi_experimental_io_devices::initialize));
            }
        }

        let mut wasi_env = wasi_state_builder.finalize()?;
        let resolver = wasi_env.import_object_for_all_wasi_versions(&module)?;
        let instance = Instance::new(&module, &resolver)?;
        Ok(instance)
    }

    /// Helper function for handling the result of a Wasi _start function.
    pub fn handle_result(&self, result: Result<Box<[Val]>, RuntimeError>) -> Result<()> {
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
            .unwrap_or(PathBuf::from("."));
        Ok(Self {
            deny_multiple_wasi_versions: true,
            env_vars: env::vars().collect(),
            pre_opened_directories: vec![dir],
            ..Self::default()
        })
    }
}
