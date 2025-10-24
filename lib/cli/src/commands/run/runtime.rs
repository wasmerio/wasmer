//! Provides CLI-specific Wasix components.

use anyhow::Error;
use std::{sync::Arc, time::Duration};

use futures::future::BoxFuture;
use indicatif::ProgressBar;
use is_terminal::IsTerminal as _;
use wasmer::{Engine, Module};
use wasmer_config::package::PackageSource;
use wasmer_types::ModuleHash;
use wasmer_wasix::{
    SpawnError,
    bin_factory::{BinaryPackage, BinaryPackageCommand},
    runtime::{
        module_cache::HashedModuleData,
        resolver::{PackageSummary, QueryError},
    },
};
use webc::Container;

/// Special wasix runtime implementation for the CLI.
///
/// Wraps an undelrying runtime and adds progress monitoring for package
/// compilation.
#[derive(Debug)]
pub struct MonitoringRuntime<R> {
    pub runtime: Arc<R>,
    progress: ProgressBar,
    quiet_mode: bool,
}

impl<R> MonitoringRuntime<R> {
    pub fn new(runtime: R, progress: ProgressBar, quiet_mode: bool) -> Self {
        MonitoringRuntime {
            runtime: Arc::new(runtime),
            progress,
            quiet_mode,
        }
    }
}

impl<R: wasmer_wasix::Runtime + Send + Sync> wasmer_wasix::Runtime for MonitoringRuntime<R> {
    fn networking(&self) -> &virtual_net::DynVirtualNetworking {
        self.runtime.networking()
    }

    fn task_manager(&self) -> &Arc<dyn wasmer_wasix::VirtualTaskManager> {
        self.runtime.task_manager()
    }

    fn package_loader(
        &self,
    ) -> Arc<dyn wasmer_wasix::runtime::package_loader::PackageLoader + Send + Sync> {
        let inner = self.runtime.package_loader();
        Arc::new(MonitoringPackageLoader {
            inner,
            progress: self.progress.clone(),
        })
    }

    fn module_cache(
        &self,
    ) -> Arc<dyn wasmer_wasix::runtime::module_cache::ModuleCache + Send + Sync> {
        self.runtime.module_cache()
    }

    fn source(&self) -> Arc<dyn wasmer_wasix::runtime::resolver::Source + Send + Sync> {
        let inner = self.runtime.source();
        Arc::new(MonitoringSource {
            inner,
            progress: self.progress.clone(),
        })
    }

    fn engine(&self) -> Engine {
        self.runtime.engine()
    }

    fn new_store(&self) -> wasmer::Store {
        self.runtime.new_store()
    }

    fn http_client(&self) -> Option<&wasmer_wasix::http::DynHttpClient> {
        self.runtime.http_client()
    }

    fn tty(&self) -> Option<&(dyn wasmer_wasix::os::TtyBridge + Send + Sync)> {
        self.runtime.tty()
    }

    #[cfg(feature = "journal")]
    fn read_only_journals<'a>(
        &'a self,
    ) -> Box<dyn Iterator<Item = Arc<wasmer_wasix::journal::DynReadableJournal>> + 'a> {
        self.runtime.read_only_journals()
    }

    #[cfg(feature = "journal")]
    fn writable_journals<'a>(
        &'a self,
    ) -> Box<dyn Iterator<Item = Arc<wasmer_wasix::journal::DynJournal>> + 'a> {
        self.runtime.writable_journals()
    }

    #[cfg(feature = "journal")]
    fn active_journal(&self) -> Option<&'_ wasmer_wasix::journal::DynJournal> {
        self.runtime.active_journal()
    }

    fn load_hashed_module(
        &self,
        module: HashedModuleData,
        engine: Option<&Engine>,
    ) -> BoxFuture<'_, Result<Module, SpawnError>> {
        let hash = *module.hash();
        let fut = self.runtime.load_hashed_module(module, engine);
        Box::pin(compile_with_progress(fut, hash, None, self.quiet_mode))
    }

    fn load_hashed_module_sync(
        &self,
        wasm: HashedModuleData,
        engine: Option<&Engine>,
    ) -> Result<Module, wasmer_wasix::SpawnError> {
        let hash = *wasm.hash();
        compile_with_progress_sync(
            || self.runtime.load_hashed_module_sync(wasm, engine),
            &hash,
            None,
            self.quiet_mode,
        )
    }

    fn load_command_module(
        &self,
        cmd: &BinaryPackageCommand,
    ) -> BoxFuture<'_, Result<Module, SpawnError>> {
        let fut = self.runtime.load_command_module(cmd);

        Box::pin(compile_with_progress(
            fut,
            *cmd.hash(),
            Some(cmd.name().to_owned()),
            self.quiet_mode,
        ))
    }

    fn load_command_module_sync(
        &self,
        cmd: &wasmer_wasix::bin_factory::BinaryPackageCommand,
    ) -> Result<Module, wasmer_wasix::SpawnError> {
        compile_with_progress_sync(
            || self.runtime.load_command_module_sync(cmd),
            cmd.hash(),
            Some(cmd.name()),
            self.quiet_mode,
        )
    }
}

async fn compile_with_progress<'a, F, T>(
    fut: F,
    hash: ModuleHash,
    name: Option<String>,
    quiet_mode: bool,
) -> T
where
    F: std::future::Future<Output = T> + Send + 'a,
    T: Send + 'static,
{
    let mut pb = new_progressbar_compile(&hash, name.as_deref(), quiet_mode);
    let res = fut.await;
    pb.finish_and_clear();
    res
}

fn compile_with_progress_sync<F, T>(
    f: F,
    hash: &ModuleHash,
    name: Option<&str>,
    quiet_mode: bool,
) -> T
where
    F: FnOnce() -> T,
{
    let mut pb = new_progressbar_compile(hash, name, quiet_mode);
    let res = f();
    pb.finish_and_clear();
    res
}

fn new_progressbar_compile(hash: &ModuleHash, name: Option<&str>, quiet_mode: bool) -> ProgressBar {
    // Only show a spinner if we're running in a TTY
    let hash = hash.to_string();
    let hash = &hash[0..8];
    if !quiet_mode && std::io::stderr().is_terminal() {
        let msg = if let Some(name) = name {
            format!("Compiling WebAssembly module for command '{name}' ({hash})...")
        } else {
            format!("Compiling WebAssembly module {hash}...")
        };
        let pb = ProgressBar::new_spinner().with_message(msg);
        pb.enable_steady_tick(Duration::from_millis(100));
        pb
    } else {
        ProgressBar::hidden()
    }
}

#[derive(Debug)]
struct MonitoringSource {
    inner: Arc<dyn wasmer_wasix::runtime::resolver::Source + Send + Sync>,
    progress: ProgressBar,
}

#[async_trait::async_trait]
impl wasmer_wasix::runtime::resolver::Source for MonitoringSource {
    async fn query(&self, package: &PackageSource) -> Result<Vec<PackageSummary>, QueryError> {
        self.progress.set_message(format!("Looking up {package}"));
        self.inner.query(package).await
    }
}

#[derive(Debug)]
struct MonitoringPackageLoader {
    inner: Arc<dyn wasmer_wasix::runtime::package_loader::PackageLoader + Send + Sync>,
    progress: ProgressBar,
}

#[async_trait::async_trait]
impl wasmer_wasix::runtime::package_loader::PackageLoader for MonitoringPackageLoader {
    async fn load(&self, summary: &PackageSummary) -> Result<Container, Error> {
        let pkg_id = summary.package_id();
        self.progress.set_message(format!("Downloading {pkg_id}"));

        self.inner.load(summary).await
    }

    async fn load_package_tree(
        &self,
        root: &Container,
        resolution: &wasmer_wasix::runtime::resolver::Resolution,
        root_is_local_dir: bool,
    ) -> Result<BinaryPackage, Error> {
        self.inner
            .load_package_tree(root, resolution, root_is_local_dir)
            .await
    }
}
