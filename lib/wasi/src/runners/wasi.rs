//! WebC container support for running WASI modules

use std::{path::PathBuf, sync::Arc};

use crate::{
    runners::{wcgi::MappedDirectory, WapmContainer},
    PluggableRuntimeImplementation, VirtualTaskManager, WasiEnvBuilder,
};
use anyhow::{Context, Error};
use serde::{Deserialize, Serialize};
use wasmer::{Module, Store};
use webc::metadata::{annotations::Wasi, Command};

#[derive(Debug, Serialize, Deserialize)]
pub struct WasiRunner {
    args: Vec<String>,
    mapped_dirs: Vec<MappedDirectory>,
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
            mapped_dirs: Vec::new(),
            tasks: None,
        }
    }

    /// Returns the current arguments for this `WasiRunner`
    pub fn get_args(&self) -> Vec<String> {
        self.args.clone()
    }

    /// Builder method to provide CLI args to the runner
    pub fn with_args<A, S>(mut self, args: A) -> Self
    where
        A: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.set_args(args);
        self
    }

    /// Set the CLI args
    pub fn set_args<A, S>(&mut self, args: A)
    where
        A: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args = args.into_iter().map(|s| s.into()).collect();
    }

    pub fn with_mapped_directory(
        mut self,
        host: impl Into<PathBuf>,
        guest: impl Into<String>,
    ) -> Self {
        self.map_directory(host, guest);
        self
    }

    pub fn map_directory(
        &mut self,
        host: impl Into<PathBuf>,
        guest: impl Into<String>,
    ) -> &mut Self {
        self.mapped_dirs.push(MappedDirectory {
            host: host.into(),
            guest: guest.into(),
        });
        self
    }

    pub fn with_map_directories<I, H, G>(mut self, mappings: I) -> Self
    where
        I: IntoIterator<Item = (H, G)>,
        H: Into<PathBuf>,
        G: Into<String>,
    {
        self.map_directories(mappings);
        self
    }

    pub fn map_directories<I, H, G>(&mut self, mappings: I) -> &mut Self
    where
        I: IntoIterator<Item = (H, G)>,
        H: Into<PathBuf>,
        G: Into<String>,
    {
        let mappings = mappings.into_iter().map(|(h, g)| MappedDirectory {
            host: h.into(),
            guest: g.into(),
        });
        self.mapped_dirs.extend(mappings);
        self
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
            let rt = PluggableRuntimeImplementation::new(Arc::clone(tasks));
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
    let (filesystem, preopen_dirs) = container.container_fs();
    let mut builder = WasiEnvBuilder::new(command).args(args);

    for entry in preopen_dirs {
        builder.add_preopen_build(|p| p.directory(&entry).read(true).write(true).create(true))?;
    }

    builder.set_fs(filesystem);

    Ok(builder)
}
