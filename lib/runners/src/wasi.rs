#![cfg(feature = "webc_runner_rt_wasi")]
//! WebC container support for running WASI modules

use crate::runners::WapmContainer;
use crate::{WasiEnv, WasiEnvBuilder};
use serde::{Deserialize, Serialize};
use std::error::Error as StdError;
use std::sync::Arc;
use wasmer::{Module, Store};
use wasmer_vfs::webc_fs::WebcFileSystem;
use webc::{Command, WebCMmap};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct WasiRunner {
    args: Vec<String>,
    #[serde(skip, default)]
    store: Store,
}

impl WasiRunner {
    /// Constructs a new `WasiRunner` given an `Store`
    pub fn new(store: Store) -> Self {
        Self {
            args: Vec::new(),
            store,
        }
    }

    /// Returns the current arguments for this `WasiRunner`
    pub fn get_args(&self) -> Vec<String> {
        self.args.clone()
    }

    /// Builder method to provide CLI args to the runner
    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.set_args(args);
        self
    }

    /// Set the CLI args
    pub fn set_args(&mut self, args: Vec<String>) {
        self.args = args;
    }
}

impl crate::runners::Runner for WasiRunner {
    type Output = ();

    fn can_run_command(
        &self,
        _command_name: &str,
        command: &Command,
    ) -> Result<bool, Box<dyn StdError>> {
        Ok(command.runner.starts_with("https://webc.org/runner/wasi"))
    }

    #[allow(unreachable_code, unused_variables)]
    fn run_command(
        &mut self,
        command_name: &str,
        _command: &Command,
        container: &WapmContainer,
    ) -> Result<Self::Output, Box<dyn StdError>> {
        let atom_name = container.get_atom_name_for_command("wasi", command_name)?;
        let atom_bytes = container.get_atom(&container.get_package_name(), &atom_name)?;

        let mut module = Module::new(&self.store, atom_bytes)?;
        module.set_name(&atom_name);

        let builder = prepare_webc_env(container.webc.clone(), &atom_name, &self.args)?;

        let init = builder.build_init()?;

        let (instance, env) = WasiEnv::instantiate(init, module, &mut self.store)?;

        let _result = instance
            .exports
            .get_function("_start")?
            .call(&mut self.store, &[])?;

        Ok(())
    }
}

// https://github.com/tokera-com/ate/blob/42c4ce5a0c0aef47aeb4420cc6dc788ef6ee8804/term-lib/src/eval/exec.rs#L444
fn prepare_webc_env(
    webc: Arc<WebCMmap>,
    command: &str,
    args: &[String],
) -> Result<WasiEnvBuilder, anyhow::Error> {
    use webc::FsEntryType;

    let package_name = webc.get_package_name();
    let top_level_dirs = webc
        .get_volumes_for_package(&package_name)
        .into_iter()
        .flat_map(|volume| {
            webc.volumes
                .get(&volume)
                .unwrap()
                .header
                .top_level
                .iter()
                .filter(|e| e.fs_type == FsEntryType::Dir)
                .map(|e| e.text.to_string())
        })
        .collect::<Vec<_>>();

    let filesystem = Box::new(WebcFileSystem::init(webc, &package_name));
    let mut builder = WasiEnv::builder(command).fs(filesystem).args(args);
    for f_name in top_level_dirs.iter() {
        builder.add_preopen_build(|p| p.directory(f_name).read(true).write(true).create(true))?;
    }

    Ok(builder)
}
