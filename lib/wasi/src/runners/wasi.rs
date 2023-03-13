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
use virtual_fs::{FileSystem, PassthruFileSystem, RootFileSystemBuilder};
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
        let root_fs = unioned_filesystem(&self.mapped_dirs, container)?;

        let mut builder = WasiEnv::builder(command)
            .args(&self.args)
            .fs(Box::new(root_fs))
            .preopen_dir("/")?;

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

/// Create a [`FileSystem`] which merges the WAPM container's volumes with any
/// directories that were mapped from the host.
fn unioned_filesystem(
    mapped_dirs: &[MappedDirectory],
    container: &WapmContainer,
) -> Result<impl FileSystem, Error> {
    let mut fs = wasmer_vfs::UnionFileSystem::new();

    // First, mount the root filesystem so we get things like "/dev/"
    fs.mount(
        "base",
        "/",
        false,
        Box::new(RootFileSystemBuilder::new().build()),
        None,
    );

    // We also want the container's volumes
    fs.mount("webc", "/", false, container.container_fs(), None);

    // Now, let's merge in the host volumes.
    if !mapped_dirs.is_empty() {
        let mapped_fs = wasmer_vfs::TmpFileSystem::new();

        let host_fs: Arc<dyn FileSystem + Send + Sync> =
            Arc::new(PassthruFileSystem::new(crate::default_fs_backing()));

        for MappedDirectory { host, guest } in mapped_dirs.iter() {
            let guest = match guest.starts_with('/') {
                true => PathBuf::from(guest),
                false => Path::new("/").join(guest),
            };
            tracing::debug!(
                host=%host.display(),
                guest=%guest.display(),
                "mounting host directory",
            );

            if let Some(parent) = guest.parent() {
                mapped_fs.create_dir_all(parent.as_ref())?;
            }

            mapped_fs
                .mount(guest.clone(), &host_fs, host.clone())
                .with_context(|| {
                    format!(
                        "Unable to mount \"{}\" to \"{}\"",
                        host.display(),
                        guest.display()
                    )
                })?;
        }

        fs.mount("host", "/", true, Box::new(mapped_fs), None);
    }

    Ok(fs)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use tokio::io::AsyncReadExt;

    use super::*;

    #[track_caller]
    async fn read_file(fs: &dyn FileSystem, path: impl AsRef<Path>) -> String {
        let mut f = fs.new_open_options().read(true).open(path).unwrap();
        let mut contents = String::new();
        f.read_to_string(&mut contents).await.unwrap();

        contents
    }

    #[track_caller]
    fn read_dir(fs: &dyn FileSystem, path: impl AsRef<Path>) -> Vec<PathBuf> {
        fs.read_dir(path.as_ref())
            .unwrap()
            .filter_map(|result| result.ok())
            .map(|entry| entry.path)
            .collect()
    }

    #[tokio::test]
    async fn construct_the_unioned_fs() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("file.txt"), b"Hello, World!").unwrap();
        let webc: &[u8] =
            include_bytes!("../../../../lib/c-api/examples/assets/python-0.1.0.wasmer");
        let container = WapmContainer::from_bytes(webc.into()).unwrap();
        let mapped_dirs = [MappedDirectory {
            guest: "/path/to/".to_string(),
            host: temp.path().to_path_buf(),
        }];

        let fs = unioned_filesystem(&mapped_dirs, &container).unwrap();

        // Files that were mounted on the host
        assert_eq!(
            read_dir(&fs, "/path/to/"),
            vec![PathBuf::from("/path/to/file.txt")]
        );
        assert_eq!(read_file(&fs, "/path/to/file.txt").await, "Hello, World!");
        // Files from the Python WEBC file's volumes
        assert_eq!(
            read_dir(&fs, "lib/python3.6/collections/"),
            vec![
                PathBuf::from("lib/python3.6/collections/__init__.py"),
                PathBuf::from("lib/python3.6/collections/abc.py"),
            ]
        );
        let abc = read_file(&fs, "lib/python3.6/collections/abc.py").await;
        assert_eq!(abc, "from _collections_abc import *");
    }
}
