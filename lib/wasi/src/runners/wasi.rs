//! WebC container support for running WASI modules

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    runners::{MappedDirectory, WapmContainer},
    PluggableRuntime, VirtualTaskManager,
};
use crate::{WasiEnv, WasiEnvBuilder};
use anyhow::{Context, Error};
use serde::{Deserialize, Serialize};
use virtual_fs::{FileSystem, OverlayFileSystem, RootFileSystemBuilder, TraceFileSystem};
use wasmer::{Module, Store};
use webc::metadata::{annotations::Wasi, Command};

#[derive(Debug, Serialize, Deserialize)]
pub struct WasiRunner {
    args: Vec<String>,
    env: HashMap<String, String>,
    forward_host_env: bool,
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
            env: HashMap::new(),
            store,
            mapped_dirs: Vec::new(),
            forward_host_env: false,
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

    /// Builder method to provide environment variables to the runner.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.set_env(key, value);
        self
    }

    /// Provide environment variables to the runner.
    pub fn set_env(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.env.insert(key.into(), value.into());
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
            self.env.insert(key.into(), value.into());
        }
    }

    pub fn with_forward_host_env(mut self) -> Self {
        self.set_forward_host_env();
        self
    }

    pub fn set_forward_host_env(&mut self) {
        self.forward_host_env = true;
    }

    pub fn with_mapped_directories<I, D>(mut self, dirs: I) -> Self
    where
        I: IntoIterator<Item = D>,
        D: Into<MappedDirectory>,
    {
        self.mapped_dirs.extend(dirs.into_iter().map(|d| d.into()));
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
        command: &str,
    ) -> Result<WasiEnvBuilder, anyhow::Error> {
        let mut builder = WasiEnv::builder(command).args(&self.args);

        let root_fs = RootFileSystemBuilder::default().build();

        if !self.mapped_dirs.is_empty() {
            let host_fs: Arc<dyn FileSystem + Send + Sync> = Arc::new(crate::default_fs_backing());

            for mapped in &self.mapped_dirs {
                let MappedDirectory { host, guest } = mapped;
                let guest = if guest.starts_with('/') {
                    PathBuf::from(guest)
                } else {
                    Path::new("/").join(guest)
                };
                tracing::debug!(
                    guest=%guest.display(),
                    host=%host.display(),
                    "Mounting host folder",
                );
                root_fs
                    .mount(guest.clone(), &host_fs, host.clone())
                    .with_context(|| {
                        format!(
                            "Unable to mount \"{}\" to \"{}\"",
                            host.display(),
                            guest.display()
                        )
                    })?;
            }
        }

        let (container_fs, preopen_dirs) = container.container_fs();

        for dir in preopen_dirs {
            builder.add_preopen_dir(dir)?;
        }

        builder.set_fs(Box::new(TraceFileSystem(OverlayFileSystem::new(
            root_fs,
            [container_fs],
        ))));
        // builder.set_fs(Box::new(TraceFileSystem(OverlayFileSystem::new(
        //     container_fs,
        //     [root_fs],
        // ))));
        // builder.set_fs(Box::new(TraceFileSystem(container_fs)));

        if self.forward_host_env {
            for (k, v) in std::env::vars() {
                builder.add_env(k, v);
            }
        }

        for (k, v) in &self.env {
            builder.add_env(k, v);
        }

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

        self.prepare_webc_env(container, &atom_name)?.run(module)?;

        Ok(())
    }
}
