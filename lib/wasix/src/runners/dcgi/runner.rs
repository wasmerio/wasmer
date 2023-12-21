use std::{net::SocketAddr, sync::Arc};

use anyhow::Error;
use wcgi_host::CgiDialect;
use webc::metadata::Command;

use crate::{
    bin_factory::BinaryPackage,
    capabilities::Capabilities,
    runners::{
        dcgi::handler::Handler,
        wcgi::{self, NoopCallbacks},
        MappedDirectory,
    },
    Runtime,
};

use super::{DcgiCallbacks, DcgiInstanceFactory, DcgiMetadata};

#[derive(Debug)]
pub struct DcgiRunner {
    config: Config,
    inner: wcgi::WcgiRunner<DcgiMetadata>,
}

impl DcgiRunner {
    pub fn new(factory: DcgiInstanceFactory) -> Self {
        let callbacks = DcgiCallbacks::new(factory, NoopCallbacks);
        DcgiRunner {
            config: Config {
                inner: wcgi::Config::new(callbacks),
            },
            inner: Default::default(),
        }
    }

    pub fn config(&mut self) -> &mut Config {
        &mut self.config
    }

    #[tracing::instrument(skip_all)]
    fn prepare_handler(
        &mut self,
        command_name: &str,
        pkg: &BinaryPackage,
        runtime: Arc<dyn Runtime + Send + Sync>,
    ) -> Result<Handler, Error> {
        let inner: wcgi::Handler<DcgiMetadata> =
            self.inner
                .prepare_handler(command_name, pkg, true, CgiDialect::Rfc3875, runtime)?;
        Ok(Handler::from_wcgi_handler(inner))
    }
}

/// The base URI used by a [`Dcgi`] runner.
pub const DCGI_RUNNER_URI: &str = "https://webc.org/runner/dcgi";

impl crate::runners::Runner for DcgiRunner {
    fn can_run_command(command: &Command) -> Result<bool, Error> {
        Ok(command.runner.starts_with(DCGI_RUNNER_URI))
    }

    fn run_command(
        &mut self,
        command_name: &str,
        pkg: &BinaryPackage,
        runtime: Arc<dyn Runtime + Send + Sync>,
    ) -> Result<(), Error> {
        let handler = self.prepare_handler(command_name, pkg, Arc::clone(&runtime))?;
        self.inner.run_command_with_handler(handler, runtime)
    }
}

#[derive(Debug)]
pub struct Config {
    inner: wcgi::Config<DcgiMetadata>,
}

impl Config {
    pub fn inner(&mut self) -> &mut wcgi::Config<DcgiMetadata> {
        &mut self.inner
    }

    pub fn addr(&mut self, addr: SocketAddr) -> &mut Self {
        self.inner.addr(addr);
        self
    }

    /// Add an argument to the WASI executable's command-line arguments.
    pub fn arg(&mut self, arg: impl Into<String>) -> &mut Self {
        self.inner.arg(arg);
        self
    }

    /// Add multiple arguments to the WASI executable's command-line arguments.
    pub fn args<A, S>(&mut self, args: A) -> &mut Self
    where
        A: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.inner.args(args);
        self
    }

    /// Expose an environment variable to the guest.
    pub fn env(&mut self, name: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.inner.env(name, value);
        self
    }

    /// Expose multiple environment variables to the guest.
    pub fn envs<I, K, V>(&mut self, variables: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.inner.envs(variables);
        self
    }

    /// Forward all of the host's environment variables to the guest.
    pub fn forward_host_env(&mut self) -> &mut Self {
        self.inner.forward_host_env();
        self
    }

    pub fn map_directory(&mut self, dir: MappedDirectory) -> &mut Self {
        self.inner.map_directory(dir);
        self
    }

    pub fn map_directories(
        &mut self,
        mappings: impl IntoIterator<Item = MappedDirectory>,
    ) -> &mut Self {
        self.inner.map_directories(mappings);
        self
    }

    /// Set callbacks that will be triggered at various points in the runner's
    /// lifecycle.
    pub fn callbacks(
        &mut self,
        callbacks: impl wcgi::Callbacks<DcgiMetadata> + Send + Sync + 'static,
    ) -> &mut Self {
        self.inner.callbacks(callbacks);
        self
    }

    /// Add a package that should be available to the instance at runtime.
    pub fn inject_package(&mut self, pkg: BinaryPackage) -> &mut Self {
        self.inner.inject_package(pkg);
        self
    }

    /// Add packages that should be available to the instance at runtime.
    pub fn inject_packages(
        &mut self,
        packages: impl IntoIterator<Item = BinaryPackage>,
    ) -> &mut Self {
        self.inner.inject_packages(packages);
        self
    }

    pub fn capabilities(&mut self) -> &mut Capabilities {
        self.inner.capabilities()
    }

    pub fn add_snapshot_trigger(&mut self, on: crate::journal::SnapshotTrigger) {
        self.inner.add_snapshot_trigger(on);
    }

    pub fn add_default_snapshot_triggers(&mut self) -> &mut Self {
        self.inner.add_default_snapshot_triggers();
        self
    }

    pub fn has_snapshot_trigger(&self, on: crate::journal::SnapshotTrigger) -> bool {
        self.inner.has_snapshot_trigger(on)
    }

    pub fn with_snapshot_interval(&mut self, period: std::time::Duration) -> &mut Self {
        self.inner.with_snapshot_interval(period);
        self
    }

    pub fn add_journal(&mut self, journal: Arc<crate::journal::DynJournal>) -> &mut Self {
        self.inner.add_journal(journal);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_and_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<DcgiRunner>();
        assert_sync::<DcgiRunner>();
    }
}
