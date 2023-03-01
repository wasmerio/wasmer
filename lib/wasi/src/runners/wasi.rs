//! WebC container support for running WASI modules

use std::sync::Arc;

use crate::{runners::WapmContainer, PluggableRuntimeImplementation, VirtualTaskManager};
use crate::{WasiEnv, WasiEnvBuilder};
use anyhow::{Context, Error};
use serde::{Deserialize, Serialize};
use wasmer::{Module, Store};
use webc::metadata::{annotations::Wasi, Command};

#[derive(Debug, Serialize, Deserialize)]
pub struct WasiRunner {
    args: Vec<String>,
    #[serde(skip, default)]
    store: Store,
    #[serde(skip, default)]
    tasks: Option<Arc<dyn VirtualTaskManager>>,
}

impl WasiRunner {
    /// Constructs a new `WasiRunner` given an `Store`
    pub fn new(store: Store) -> Self {
        Self {
            args: Vec::new(),
            store,
            tasks: None,
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

    pub fn with_task_manager(mut self, tasks: impl VirtualTaskManager) -> Self {
        self.set_task_manager(tasks);
        self
    }

    pub fn set_task_manager(&mut self, tasks: impl VirtualTaskManager) {
        self.tasks = Some(Arc::new(tasks));
    }
}

impl crate::runners::Runner for WasiRunner {
    type Output = ();

    fn can_run_command(&self, _command_name: &str, command: &Command) -> Result<bool, Error> {
        Ok(command
            .runner
            .starts_with(webc::metadata::annotations::WASI_RUNNER_URI))
    }

    fn run_command(
        &mut self,
        command_name: &str,
        command: &Command,
        container: &WapmContainer,
    ) -> Result<Self::Output, Error> {
        let atom_name = match command.get_annotation("wasi")? {
            Some(Wasi { atom, .. }) => atom,
            None => command_name.to_string(),
        };
        let atom = container
            .get_atom(&atom_name)
            .with_context(|| format!("Unable to get the \"{atom_name}\" atom"))?;

        let mut module = Module::new(&self.store, atom)?;
        module.set_name(&atom_name);

        let mut builder = prepare_webc_env(container, &atom_name, &self.args)?;

        if let Some(tasks) = &self.tasks {
            let rt = PluggableRuntimeImplementation::new(Arc::clone(&tasks));
            builder.set_runtime(Arc::new(rt));
        }

        builder.run(module)?;

        Ok(())
    }
}

// https://github.com/tokera-com/ate/blob/42c4ce5a0c0aef47aeb4420cc6dc788ef6ee8804/term-lib/src/eval/exec.rs#L444
fn prepare_webc_env(
    container: &WapmContainer,
    command: &str,
    args: &[String],
) -> Result<WasiEnvBuilder, anyhow::Error> {
    let filesystem = container.container_fs();
    let mut builder = WasiEnv::builder(command).args(args);

    if let Ok(dir) = filesystem.read_dir("/".as_ref()) {
        let entries = dir.filter_map(|entry| entry.ok()).filter(|entry| {
            if let Ok(file_type) = entry.file_type() {
                file_type.dir
            } else {
                false
            }
        });

        for entry in entries {
            builder.add_preopen_build(|p| {
                p.directory(&entry.path).read(true).write(true).create(true)
            })?;
        }
    }

    builder.set_fs(filesystem);

    Ok(builder)
}
