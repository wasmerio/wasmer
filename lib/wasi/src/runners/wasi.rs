//! WebC container support for running WASI modules

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    runners::{wcgi::MappedDirectory, WapmContainer},
    PluggableRuntime, VirtualTaskManager,
};
use crate::{WasiEnv, WasiEnvBuilder};
use anyhow::{Context, Error};
use serde::{Deserialize, Serialize};
use wasmer::{Module, Store};
use wasmer_vfs::{FileSystem, PassthruFileSystem, RootFileSystemBuilder};
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
        let root_fs = RootFileSystemBuilder::new().build();

        let filesystem = container.container_fs();
        root_fs.union(&filesystem);

        if !self.mapped_dirs.is_empty() {
            let fs_backing: Arc<dyn FileSystem + Send + Sync> =
                Arc::new(PassthruFileSystem::new(crate::default_fs_backing()));

            for MappedDirectory { host, guest } in self.mapped_dirs.iter() {
                let guest = match guest.starts_with('/') {
                    true => PathBuf::from(guest),
                    false => Path::new("/").join(guest),
                };
                tracing::debug!(
                    host=%host.display(),
                    guest=%guest.display(),
                    "mounting host directory",
                );

                root_fs
                    .mount(guest.clone(), &fs_backing, host.clone())
                    .with_context(|| {
                        format!(
                            "Unable to mount \"{}\" to \"{}\"",
                            host.display(),
                            guest.display()
                        )
                    })?;
            }
        }

        let builder = WasiEnv::builder(command)
            .args(&self.args)
            .fs(Box::new(root_fs))
            .preopen_dir("/")?;

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

        let mut builder = self.prepare_webc_env(container, &atom_name)?;

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

        let res = builder.run(module);
        match res {
            Ok(()) => Ok(()),
            Err(crate::WasiRuntimeError::Wasi(crate::WasiError::Exit(_))) => Ok(()),
            Err(e) => Err(e),
        }?;

        Ok(())
    }
}
