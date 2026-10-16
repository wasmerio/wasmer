//! The runtime `wasmer run` resolves, downloads and compiles packages with.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::Error;
use futures::future::BoxFuture;
use indicatif::ProgressBar;
use wasmer::{Engine, Module};
use wasmer_config::package::{PackageId, PackageSource};
use wasmer_wasix::{
    SpawnError,
    bin_factory::BinaryPackage,
    runtime::{
        ModuleInput,
        module_cache::progress::{ModuleLoadProgress, ModuleLoadProgressReporter},
        package_loader::load_package_tree,
        resolver::{PackageSummary, QueryError, Resolution, ResolvedFileSystemMapping},
    },
};
use webc::Container;

/// Special wasix runtime implementation for the CLI.
///
/// Wraps an underlying runtime and adds progress monitoring for package
/// compilation, and keeps the containers of the package tree it loads.
#[derive(Debug)]
pub struct MonitoringRuntime<R> {
    pub runtime: Arc<R>,
    progress: ProgressBar,
    quiet_mode: bool,
    tree: Arc<Mutex<PackageTree>>,
}

impl<R> MonitoringRuntime<R> {
    pub fn new(runtime: R, progress: ProgressBar, quiet_mode: bool) -> Self {
        MonitoringRuntime {
            runtime: Arc::new(runtime),
            progress,
            quiet_mode,
            tree: Arc::default(),
        }
    }

    /// The containers and volume mappings of the package tree loaded last.
    pub fn package_tree(&self) -> PackageTree {
        self.tree.lock().unwrap().clone()
    }
}

/// What the machine needs of a loaded package tree, which a `BinaryPackage` does not keep: the
/// containers whose volumes its filesystem table mounts.
#[derive(Debug, Clone, Default)]
pub struct PackageTree {
    /// Every container loaded, by package.
    pub containers: HashMap<PackageId, Container>,
    /// The tree's filesystem table; mappings earlier in the list obscure later ones.
    pub filesystem: Vec<ResolvedFileSystemMapping>,
    /// The root package, if a tree was loaded.
    pub root: Option<PackageId>,
    /// Whether the root is a package directory, whose own mappings name host directories.
    pub root_is_local_dir: bool,
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
            tree: self.tree.clone(),
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
                            if step_count != 0 {
                                write!(
                                    &mut msg,
                                    " ({:.0}%)",
                                    100.0 * step as f32 / step_count as f32
                                )
                                .unwrap();
                            }
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
    tree: Arc<Mutex<PackageTree>>,
}

#[async_trait::async_trait]
impl wasmer_wasix::runtime::package_loader::PackageLoader for MonitoringPackageLoader {
    async fn load(&self, summary: &PackageSummary) -> Result<Container, Error> {
        let pkg_id = summary.package_id();
        self.progress.set_message(format!("Downloading {pkg_id}"));

        let container = self.inner.load(summary).await?;
        self.tree
            .lock()
            .unwrap()
            .containers
            .insert(pkg_id, container.clone());
        Ok(container)
    }

    async fn load_package_tree(
        &self,
        root: &Container,
        resolution: &Resolution,
        root_is_local_dir: bool,
    ) -> Result<BinaryPackage, Error> {
        {
            let mut tree = self.tree.lock().unwrap();
            let id = resolution.package.root_package.clone();
            tree.containers.insert(id.clone(), root.clone());
            tree.filesystem = resolution.package.filesystem.clone();
            tree.root = Some(id);
            tree.root_is_local_dir = root_is_local_dir;
        }
        // Through `self`, so the dependencies' containers are kept too.
        load_package_tree(root, self, resolution, root_is_local_dir).await
    }
}
