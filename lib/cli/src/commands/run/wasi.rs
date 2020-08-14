use crate::utils::{parse_envvar, parse_mapdir};
use anyhow::{Context, Result};
use std::path::PathBuf;
use wasmer::{Instance, Module};
use wasmer_wasi::{get_wasi_version, WasiError, WasiState, WasiVersion};

use structopt::StructOpt;

#[cfg(feature = "wasio")]
mod wasio {
    use anyhow::{Error, Result};
    use std::str::FromStr;
    use std::string::ToString;
    use std::sync::Arc;
    use wasmer_wasi::wasio::Executor;

    /// The executor used for wasio
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ExecutorType {
        /// Dummy executor
        Dummy,
        /// Tokio executor
        Tokio,
    }

    impl Default for ExecutorType {
        fn default() -> Self {
            Self::Tokio
        }
    }

    impl ExecutorType {
        /// The executor associated to this `ExecutorType`.
        pub fn executor(&self) -> Result<Arc<dyn Executor>> {
            match self {
                Self::Dummy => Ok(Arc::new(wasmer_wasi::wasio::DummyExecutor)),
                Self::Tokio => Ok(Arc::new(wasmer_wasi::wasio::TokioExecutor::new())),
                #[allow(unreachable_patterns)]
                _ => bail!("The `{:?}` executor is not enabled.", &self),
            }
        }
    }

    impl ToString for ExecutorType {
        fn to_string(&self) -> String {
            match self {
                Self::Dummy => "dummy".to_string(),
                Self::Tokio => "tokio".to_string(),
            }
        }
    }

    impl FromStr for ExecutorType {
        type Err = Error;
        fn from_str(s: &str) -> Result<Self> {
            match s {
                "dummy" => Ok(Self::Dummy),
                "tokio" => Ok(Self::Tokio),
                backend => bail!("The `{}` executor does not exist.", backend),
            }
        }
    }
}

#[derive(Debug, StructOpt, Clone)]
/// WASI Options
pub struct Wasi {
    /// WASI pre-opened directory
    #[structopt(long = "dir", name = "DIR", multiple = true, group = "wasi")]
    pre_opened_directories: Vec<PathBuf>,

    /// Map a host directory to a different location for the wasm module
    #[structopt(long = "mapdir", name = "GUEST_DIR:HOST_DIR", multiple = true, parse(try_from_str = parse_mapdir))]
    mapped_dirs: Vec<(String, PathBuf)>,

    /// Pass custom environment variables
    #[structopt(long = "env", name = "KEY=VALUE", multiple = true, parse(try_from_str = parse_envvar))]
    env_vars: Vec<(String, String)>,

    /// Enable experimental IO devices
    #[cfg(feature = "experimental-io-devices")]
    #[structopt(long = "enable-experimental-io-devices")]
    enable_experimental_io_devices: bool,

    #[cfg(feature = "wasio")]
    #[structopt(default_value, long = "wasio-executor")]
    wasio_executor: wasio::ExecutorType,
}

impl Wasi {
    /// Gets the WASI version (if any) for the provided module
    pub fn get_version(module: &Module) -> Option<WasiVersion> {
        // Get the wasi version on strict mode, so no other imports are
        // allowed.
        get_wasi_version(&module, false)
    }

    /// Helper function for executing Wasi from the `Run` command.
    pub fn execute(&self, module: Module, program_name: String, args: Vec<String>) -> Result<()> {
        let args = args.iter().cloned().map(|arg| arg.into_bytes());

        let mut wasi_state_builder = WasiState::new(program_name);
        wasi_state_builder
            .args(args)
            .envs(self.env_vars.clone())
            .preopen_dirs(self.pre_opened_directories.clone())?
            .map_dirs(self.mapped_dirs.clone())?;

        #[cfg(feature = "wasio")]
        {
            wasi_state_builder.wasio_executor(self.wasio_executor.executor()?);
        }

        #[cfg(feature = "experimental-io-devices")]
        {
            if self.enable_experimental_io_devices {
                wasi_state_builder
                    .setup_fs(Box::new(wasmer_wasi_experimental_io_devices::initialize));
            }
        }

        let mut wasi_env = wasi_state_builder.finalize()?;
        let import_object = wasi_env.import_object(&module)?;
        let instance = Instance::new(&module, &import_object)?;

        wasi_env.set_memory(instance.exports.get_memory("memory")?.clone());

        let start = instance.exports.get_function("_start")?;
        let result = start.call(&[]);

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
        .with_context(|| "failed to run WASI `_start` function")
    }
}
