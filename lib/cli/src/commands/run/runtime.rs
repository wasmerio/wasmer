//! Provides CLI-specific Wasix components.

use std::{sync::Arc, time::Duration};

use anyhow::Error;
use futures::future::BoxFuture;
use indicatif::ProgressBar;
use std::io::IsTerminal as _;
use wasmer::{Engine, Module};
use wasmer_config::package::PackageSource;
use wasmer_types::ModuleHash;
use wasmer_wasix::{
    SpawnError,
    bin_factory::{BinaryPackage, BinaryPackageCommand},
    runtime::{
        ModuleInput,
        module_cache::{
            HashedModuleData,
            progress::{ModuleLoadProgress, ModuleLoadProgressReporter},
        },
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

    fn resolve_module<'a>(
        &'a self,
        input: ModuleInput<'a>,
        engine: Option<&Engine>,
        on_progress: Option<ModuleLoadProgressReporter>,
    ) -> BoxFuture<'a, Result<Module, SpawnError>> {
        // If a progress reporter is already provided, or quiet mode is enabled,
        // just delegate to the inner runtime.
        if on_progress.is_some() || self.quiet_mode {
            return self.runtime.resolve_module(input, engine, on_progress);
        }

        // Compile with progress monitoring through the progress bar.

        use std::fmt::Write as _;

        let short_hash = input.hash().short_hash();
        let progress_msg = match &input {
            ModuleInput::Bytes(_) | ModuleInput::Hashed(_) => {
                format!("Compiling module ({short_hash})")
            }
            ModuleInput::Command(cmd) => format!("Compiling {}", cmd.name()),
        };

        let pb = self.progress.clone();

        let on_progress = Some(ModuleLoadProgressReporter::new({
            let base_msg = progress_msg.clone();
            move |prog| {
                let msg = match prog {
                    ModuleLoadProgress::CompilingModule(c) => {
                        let mut msg = base_msg.clone();
                        if let (Some(step), Some(step_count)) =
                            (c.phase_step(), c.phase_step_count())
                        {
                            pb.set_length(step_count);
                            pb.set_position(step);
                            // Note: writing to strings can not fail.
                            msg.push_str(&format!(
                                " ({:.0}%)",
                                100.0 * step as f32 / step_count as f32
                            ));
                        };
                        pb.tick();

                        msg
                    }
                    _ => base_msg.clone(),
                };

                pb.set_message(msg);
                Ok(())
            }
        }));

        let engine = engine.cloned();

        let style = indicatif::ProgressStyle::default_bar()
            .template("{spinner} {wide_bar:.cyan/blue} {msg}")
            .expect("invalid progress bar template");
        self.progress.set_style(style);

        self.progress.reset();
        if self.progress.is_hidden() {
            self.progress
                .set_draw_target(indicatif::ProgressDrawTarget::stderr());
        }
        self.progress.set_message(progress_msg);

        let f = async move {
            let res = self
                .runtime
                .resolve_module(input, engine.as_ref(), on_progress)
                .await;

            // Hide the progress bar and reset it to the default spinner style.
            // Needed because future module downloads should not show a bar.
            self.progress
                .set_style(indicatif::ProgressStyle::default_spinner());
            self.progress.reset();
            self.progress.finish_and_clear();

            res
        };

        Box::pin(f)
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
