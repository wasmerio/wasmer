//! WebC container support for running WASI modules

use std::sync::Arc;

use anyhow::{Context, Error};
use serde::{Deserialize, Serialize};
use wasmer::{Module, Store};
use webc::metadata::{annotations::Wasi, Command};

use crate::{
    runners::{wasi_common::CommonWasiOptions, MappedDirectory, WapmContainer},
    PluggableRuntime, VirtualTaskManager, WasiEnvBuilder,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct WasiRunner {
    wasi: CommonWasiOptions,
    #[serde(skip, default)]
    store: Store,
    #[serde(skip, default)]
    pub(crate) tasks: Option<Arc<dyn VirtualTaskManager>>,
}

impl WasiRunner {
    /// Constructs a new `WasiRunner` given an `Store`
    pub fn new(store: Store) -> Self {
        Self {
            store,
            wasi: CommonWasiOptions::default(),
            tasks: None,
        }
    }

    /// Returns the current arguments for this `WasiRunner`
    pub fn get_args(&self) -> Vec<String> {
        self.wasi.args.clone()
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
        self.wasi.args = args.into_iter().map(|s| s.into()).collect();
    }

    /// Builder method to provide environment variables to the runner.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.set_env(key, value);
        self
    }

    /// Provide environment variables to the runner.
    pub fn set_env(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.wasi.env.insert(key.into(), value.into());
    }

    pub fn with_envs<I, K, V>(mut self, envs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.set_envs(envs);
        self
    }

    pub fn set_envs<I, K, V>(&mut self, envs: I)
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        for (key, value) in envs {
            self.wasi.env.insert(key.into(), value.into());
        }
    }

    pub fn with_forward_host_env(mut self) -> Self {
        self.set_forward_host_env();
        self
    }

    pub fn set_forward_host_env(&mut self) {
        self.wasi.forward_host_env = true;
    }

    pub fn with_mapped_directories<I, D>(mut self, dirs: I) -> Self
    where
        I: IntoIterator<Item = D>,
        D: Into<MappedDirectory>,
    {
        self.wasi
            .mapped_dirs
            .extend(dirs.into_iter().map(|d| d.into()));
        self
    }

    pub fn with_task_manager(mut self, tasks: impl VirtualTaskManager) -> Self {
        self.set_task_manager(tasks);
        self
    }

    pub fn set_task_manager(&mut self, tasks: impl VirtualTaskManager) {
        self.tasks = Some(Arc::new(tasks));
    }

    fn prepare_webc_env(
        &self,
        container: &WapmContainer,
        program_name: &str,
        wasi: &Wasi,
    ) -> Result<WasiEnvBuilder, anyhow::Error> {
        let mut builder =
            self.wasi
                .prepare_webc_env(container.container_fs(), program_name, wasi)?;

        if let Some(tasks) = &self.tasks {
            let rt = PluggableRuntime::new(Arc::clone(tasks));
            builder.set_runtime(Arc::new(rt));
        }

        Ok(builder)
    }
}

impl crate::runners::Runner for WasiRunner {
    type Output = ();

    fn can_run_command(&self, _command_name: &str, command: &Command) -> Result<bool, Error> {
        Ok(command
            .runner
            .starts_with(webc::metadata::annotations::WASI_RUNNER_URI))
    }

    #[tracing::instrument(skip(self, command, container))]
    fn run_command(
        &mut self,
        command_name: &str,
        command: &Command,
        container: &WapmContainer,
    ) -> Result<Self::Output, Error> {
        let Annotations { wasi } = command
            .get_annotation(webc::metadata::annotations::WASI_RUNNER_URI)?
            .unwrap_or_default();
        let wasi = wasi.unwrap_or_else(|| Wasi::new(command_name));
        let atom_name = &wasi.atom;
        let atom = container
            .get_atom(atom_name)
            .with_context(|| format!("Unable to get the \"{atom_name}\" atom"))?;

        let mut module = Module::new(&self.store, atom)?;
        module.set_name(atom_name);

        self.prepare_webc_env(container, atom_name, &wasi)?
            .run(module)?;

        Ok(())
    }
}

#[derive(Default, Debug, serde::Deserialize)]
struct Annotations {
    wasi: Option<Wasi>,
}
