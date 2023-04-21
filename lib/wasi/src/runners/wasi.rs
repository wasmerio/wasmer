//! WebC container support for running WASI modules

use std::sync::Arc;

use anyhow::{Context, Error};
use serde::{Deserialize, Serialize};
use virtual_fs::WebcVolumeFileSystem;
use wasmer::{Engine, Module, Store};
use webc::{
    metadata::{annotations::Wasi, Command},
    Container,
};

use crate::{
    runners::{wasi_common::CommonWasiOptions, CompileModule, MappedDirectory},
    PluggableRuntime, VirtualTaskManager, WasiEnvBuilder,
};

#[derive(Serialize, Deserialize)]
pub struct WasiRunner {
    wasi: CommonWasiOptions,
    #[serde(skip, default)]
    store: Store,
    #[serde(skip, default)]
    pub(crate) tasks: Option<Arc<dyn VirtualTaskManager>>,
    #[serde(skip, default)]
    compile: Option<Box<CompileModule>>,
}

impl WasiRunner {
    /// Constructs a new `WasiRunner` given an `Store`
    pub fn new(store: Store) -> Self {
        Self {
            store,
            wasi: CommonWasiOptions::default(),
            tasks: None,
            compile: None,
        }
    }

    /// Sets the compile function
    pub fn with_compile(
        mut self,
        compile: impl Fn(&Engine, &[u8]) -> Result<Module, Error> + Send + Sync + 'static,
    ) -> Self {
        self.compile = Some(Box::new(compile));
        self
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
        container: &Container,
        program_name: &str,
        wasi: &Wasi,
    ) -> Result<WasiEnvBuilder, anyhow::Error> {
        let mut builder = WasiEnvBuilder::new(program_name);
        let container_fs = Arc::new(WebcVolumeFileSystem::mount_all(container));
        self.wasi
            .prepare_webc_env(&mut builder, container_fs, wasi)?;

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
        container: &Container,
    ) -> Result<Self::Output, Error> {
        let wasi = command
            .annotation("wasi")?
            .unwrap_or_else(|| Wasi::new(command_name));
        let atom_name = &wasi.atom;
        let atoms = container.atoms();
        let atom = atoms
            .get(atom_name)
            .with_context(|| format!("Unable to get the \"{atom_name}\" atom"))?;

        let compile = self
            .compile
            .as_deref()
            .unwrap_or(&crate::runners::default_compile);
        let mut module = compile(self.store.engine(), atom)?;
        module.set_name(atom_name);

        self.prepare_webc_env(container, atom_name, &wasi)?
            .run(module)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_and_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<WasiRunner>();
        assert_sync::<WasiRunner>();
    }
}
