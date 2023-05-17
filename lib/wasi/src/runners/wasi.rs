//! WebC container support for running WASI modules

use std::sync::Arc;

use anyhow::{Context, Error};
use serde::{Deserialize, Serialize};
use virtual_fs::FileSystem;
use webc::metadata::{annotations::Wasi, Command};

use crate::{
    bin_factory::BinaryPackage,
    runners::{wasi_common::CommonWasiOptions, MappedDirectory},
    WasiEnvBuilder, WasiRuntime,
};

#[derive(Default, Serialize, Deserialize)]
pub struct WasiRunner {
    wasi: CommonWasiOptions,
}

impl WasiRunner {
    /// Constructs a new `WasiRunner`.
    pub fn new() -> Self {
        WasiRunner::default()
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

    fn prepare_webc_env(
        &self,
        program_name: &str,
        wasi: &Wasi,
        container_fs: Arc<dyn FileSystem + Send + Sync>,
        runtime: Arc<dyn WasiRuntime + Send + Sync>,
    ) -> Result<WasiEnvBuilder, anyhow::Error> {
        let mut builder = WasiEnvBuilder::new(program_name);
        self.wasi
            .prepare_webc_env(&mut builder, container_fs, wasi)?;

        builder.set_runtime(runtime);

        Ok(builder)
    }
}

impl crate::runners::Runner for WasiRunner {
    fn can_run_command(command: &Command) -> Result<bool, Error> {
        Ok(command
            .runner
            .starts_with(webc::metadata::annotations::WASI_RUNNER_URI))
    }

    #[tracing::instrument(skip_all)]
    fn run_command(
        &mut self,
        pkg: &BinaryPackage,
        command_name: &str,
        metadata: &Command,
        runtime: Arc<dyn WasiRuntime + Send + Sync>,
    ) -> Result<(), Error> {
        let wasi = metadata
            .annotation("wasi")?
            .unwrap_or_else(|| Wasi::new(command_name));
        let cmd = pkg
            .commands
            .iter()
            .find(|cmd| cmd.name() == command_name)
            .with_context(|| format!("The package doesn't contain a \"{command_name}\" command"))?;

        let module = crate::runners::compile_module(cmd.atom(), &*runtime)?;
        let mut store = runtime.new_store();

        self.prepare_webc_env(command_name, &wasi, Arc::clone(&pkg.webc_fs), runtime)?
            .run_with_store(module, &mut store)?;

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
