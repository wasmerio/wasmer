#![cfg(feature = "webc_runner_rt_emscripten")]
//! WebC container support for running WASI modules

use crate::runners::WapmContainer;
use crate::{WasiFunctionEnv, WasiState};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::error::Error as StdError;
use std::sync::Arc;
use wasmer::{Cranelift, Instance, Module, Store};
use wasmer_vfs::webc_fs::WebcFileSystem;
use webc::{Command, WebCMmap};

#[derive(Debug, Default, Clone, PartialEq, PartialOrd, Hash, Serialize, Deserialize)]
pub struct WasiRunner {
    args: Vec<String>,
}

impl WasiRunner {
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

    fn run_command(
        &mut self,
        command_name: &str,
        _command: &Command,
        container: &WapmContainer,
    ) -> Result<Self::Output, Box<dyn StdError>> {
        let atom_name = container.get_atom_name_for_command("wasi", command_name)?;
        let atom_bytes = container.get_atom(&container.get_package_name(), &atom_name)?;

        let compiler = Cranelift::default();
        let mut store = Store::new(compiler);
        let mut module = Module::new(&store, atom_bytes)?;
        module.set_name(&atom_name);

        let env = prepare_webc_env(&mut store, container.webc.clone(), &atom_name, &self.args)?;

        exec_module(&mut store, &module, env)?;

        Ok(())
    }
}

// https://github.com/tokera-com/ate/blob/42c4ce5a0c0aef47aeb4420cc6dc788ef6ee8804/term-lib/src/eval/exec.rs#L444
fn prepare_webc_env(
    store: &mut Store,
    webc: Arc<WebCMmap>,
    command: &str,
    args: &[String],
) -> Result<WasiFunctionEnv, anyhow::Error> {
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
    let mut wasi_env = WasiState::new(command);
    wasi_env.set_fs(filesystem);
    wasi_env.args(args);
    for f_name in top_level_dirs.iter() {
        wasi_env.preopen(|p| p.directory(f_name).read(true).write(true).create(true))?;
    }

    Ok(wasi_env.finalize(store)?)
}

pub(crate) fn exec_module(
    store: &mut Store,
    module: &Module,
    wasi_env: crate::WasiFunctionEnv,
) -> Result<(), anyhow::Error> {
    let import_object = wasi_env.import_object(store, module)?;
    let instance = Instance::new(store, module, &import_object)?;
    let memory = instance.exports.get_memory("memory")?;
    wasi_env.data_mut(store).set_memory(memory.clone());

    // If this module exports an _initialize function, run that first.
    if let Ok(initialize) = instance.exports.get_function("_initialize") {
        initialize
            .call(store, &[])
            .with_context(|| "failed to run _initialize function")?;
    }

    let _result = instance.exports.get_function("_start")?.call(store, &[])?;

    Ok(())
}
