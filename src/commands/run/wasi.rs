use anyhow::{bail, Result};
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
    #[structopt(long = "dir", multiple = true, group = "wasi")]
    pre_opened_directories: Vec<PathBuf>,

    /// Map a host directory to a different location for the wasm module
    #[structopt(long = "mapdir", multiple = true)]
    mapped_dirs: Vec<String>,

    /// Pass custom environment variables
    #[structopt(long = "env", multiple = true)]
    env_vars: Vec<String>,

    /// Enable experimental IO devices
    #[cfg(feature = "experimental-io-devices")]
    #[structopt(long = "enable-experimental-io-devices")]
    enable_experimental_io_devices: bool,
}

impl Wasi {
    fn get_mapped_dirs(&self) -> Result<Vec<(String, PathBuf)>> {
        let mut md = vec![];
        for entry in self.mapped_dirs.iter() {
            if let [alias, real_dir] = entry.split(':').collect::<Vec<&str>>()[..] {
                let pb = PathBuf::from(&real_dir);
                if let Ok(pb_metadata) = pb.metadata() {
                    if !pb_metadata.is_dir() {
                        bail!("\"{}\" exists, but it is not a directory", &real_dir);
                    }
                } else {
                    bail!("Directory \"{}\" does not exist", &real_dir);
                }
                md.push((alias.to_string(), pb));
                continue;
            }
            bail!(
                "Directory mappings must consist of two paths separate by a colon. Found {}",
                &entry
            );
        }
        Ok(md)
    }

    fn get_env_vars(&self) -> Result<Vec<(&str, &str)>> {
        let mut env = vec![];
        for entry in self.env_vars.iter() {
            if let [env_var, value] = entry.split('=').collect::<Vec<&str>>()[..] {
                env.push((env_var, value));
            } else {
                bail!(
                    "Env vars must be of the form <var_name>=<value>. Found {}",
                    &entry
                );
            }
        }
        Ok(env)
    }

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
        let env_vars = self.get_env_vars()?;
        let preopened_files = self.pre_opened_directories.clone();
        let mapped_dirs = self.get_mapped_dirs()?;

        wasi_state_builder
            .args(args)
            .envs(env_vars)
            .preopen_dirs(preopened_files)?
            .map_dirs(mapped_dirs)?;

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

        start.call(&[])?;

        Ok(())
    }
}
