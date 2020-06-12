use crate::utils::{parse_envvar, parse_mapdir};
use anyhow::{Context, Result};
use std::path::PathBuf;
use wasmer::{Function, Instance, Memory, Module};
use wasmer_wasi::{
    generate_import_object_from_env, get_wasi_version, WasiEnv, WasiState, WasiVersion,
};

use structopt::StructOpt;

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
}

impl Wasi {
    /// Gets the WASI version (if any) for the provided module
    pub fn get_version(module: &Module) -> Option<WasiVersion> {
        // Get the wasi version on strict mode, so no other imports are
        // allowed.
        get_wasi_version(&module, true)
    }

    /// Helper function for executing Wasi from the `Run` command.
    pub fn execute(
        &self,
        module: Module,
        wasi_version: WasiVersion,
        program_name: String,
        args: Vec<String>,
    ) -> Result<()> {
        let mut wasi_state_builder = WasiState::new(&program_name);

        let args = args.iter().cloned().map(|arg| arg.into_bytes());

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

        let wasi_state = wasi_state_builder.build()?;
        let mut wasi_env = WasiEnv::new(wasi_state);
        let import_object =
            generate_import_object_from_env(module.store(), &mut wasi_env, wasi_version);

        let instance = Instance::new(&module, &import_object)?;

        let memory: &Memory = instance.exports.get("memory")?;
        wasi_env.set_memory(memory);

        let start: &Function = instance.exports.get("_start")?;

        start
            .call(&[])
            .with_context(|| "failed to run WASI `_start` function")?;

        Ok(())
    }
}
