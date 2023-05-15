pub mod module_cache;
pub mod package_loader;
pub mod resolver;
pub mod task_manager;

pub use self::task_manager::{SpawnMemoryType, VirtualTaskManager};

use std::{
    fmt,
    sync::{Arc, Mutex},
};

use derivative::Derivative;
use futures::future::BoxFuture;
use virtual_net::{DynVirtualNetworking, VirtualNetworking};

use crate::{
    bin_factory::BinaryPackage,
    http::DynHttpClient,
    os::TtyBridge,
    runtime::{
        module_cache::ModuleCache,
        package_loader::{BuiltinLoader, PackageLoader},
        resolver::{MultiSourceRegistry, Registry, Resolution, WapmSource},
    },
    WasiTtyState,
};

/// Represents an implementation of the WASI runtime - by default everything is
/// unimplemented.
///
/// # Loading Packages
///
/// Loading a package, complete with dependencies, can feel a bit involved
/// because it requires several non-trivial components.
///
/// ```rust
/// use wasmer_wasix::{
///   runtime::{
///     WasiRuntime,
///     resolver::{PackageSpecifier, resolve},
///   },
///   bin_factory::BinaryPackage,
/// };
///
/// async fn with_runtime(runtime: &dyn WasiRuntime) -> Result<(), Box<dyn std::error::Error + Send +Sync>> {
///   let registry = runtime.registry();
///   let specifier: PackageSpecifier = "python/python@3.10".parse()?;
///   let root_package = registry.latest(&specifier).await?;
///   let resolution = resolve(&root_package, &registry).await?;
///   let pkg: BinaryPackage = runtime.load_package_tree(&resolution).await?;
///   Ok(())
/// }
#[allow(unused_variables)]
pub trait WasiRuntime
where
    Self: fmt::Debug + Sync,
{
    /// Provides access to all the networking related functions such as sockets.
    /// By default networking is not implemented.
    fn networking(&self) -> &DynVirtualNetworking;

    /// Retrieve the active [`VirtualTaskManager`].
    fn task_manager(&self) -> &Arc<dyn VirtualTaskManager>;

    /// A package loader.
    fn package_loader(&self) -> Arc<dyn PackageLoader + Send + Sync>;

    /// A cache for compiled modules.
    fn module_cache(&self) -> Arc<dyn ModuleCache + Send + Sync>;

    /// The package registry.
    fn registry(&self) -> Arc<dyn Registry + Send + Sync>;

    /// Get a [`wasmer::Engine`] for module compilation.
    fn engine(&self) -> Option<wasmer::Engine> {
        None
    }

    /// Create a new [`wasmer::Store`].
    fn new_store(&self) -> wasmer::Store {
        cfg_if::cfg_if! {
            if #[cfg(feature = "sys")] {
                if let Some(engine) = self.engine() {
                    wasmer::Store::new(engine)
                } else {
                    wasmer::Store::default()
                }
            } else {
                wasmer::Store::default()
            }
        }
    }

    /// Returns a HTTP client
    fn http_client(&self) -> Option<&DynHttpClient> {
        None
    }

    /// Get access to the TTY used by the environment.
    fn tty(&self) -> Option<&(dyn TtyBridge + Send + Sync)> {
        None
    }

    fn load_package_tree<'a>(
        &'a self,
        resolution: &'a Resolution,
    ) -> BoxFuture<'a, Result<BinaryPackage, Box<dyn std::error::Error + Send + Sync>>> {
        let package_loader = self.package_loader();

        Box::pin(async move {
            let pkg = resolver::load_package_tree(&package_loader, resolution).await?;
            Ok(pkg)
        })
    }
}

#[derive(Debug, Default)]
pub struct DefaultTty {
    state: Mutex<WasiTtyState>,
}

impl TtyBridge for DefaultTty {
    fn reset(&self) {
        let mut state = self.state.lock().unwrap();
        state.echo = false;
        state.line_buffered = false;
        state.line_feeds = false
    }

    fn tty_get(&self) -> WasiTtyState {
        let state = self.state.lock().unwrap();
        state.clone()
    }

    fn tty_set(&self, tty_state: WasiTtyState) {
        let mut state = self.state.lock().unwrap();
        *state = tty_state;
    }
}

#[derive(Clone, Derivative)]
#[derivative(Debug)]
pub struct PluggableRuntime {
    pub rt: Arc<dyn VirtualTaskManager>,
    pub networking: DynVirtualNetworking,
    pub http_client: Option<DynHttpClient>,
    pub loader: Arc<dyn PackageLoader + Send + Sync>,
    pub registry: Arc<dyn Registry + Send + Sync>,
    pub engine: Option<wasmer::Engine>,
    pub module_cache: Arc<dyn ModuleCache + Send + Sync>,
    #[derivative(Debug = "ignore")]
    pub tty: Option<Arc<dyn TtyBridge + Send + Sync>>,
}

impl PluggableRuntime {
    pub fn new(rt: Arc<dyn VirtualTaskManager>) -> Self {
        // TODO: the cfg flags below should instead be handled by separate implementations.
        cfg_if::cfg_if! {
            if #[cfg(feature = "host-vnet")] {
                let networking = Arc::new(virtual_net::host::LocalNetworking::default());
            } else {
                let networking = Arc::new(virtual_net::UnsupportedVirtualNetworking::default());
            }
        }
        let http_client =
            crate::http::default_http_client().map(|client| Arc::new(client) as DynHttpClient);

        let loader =
            BuiltinLoader::from_env().expect("Loading the builtin resolver should never fail");

        let mut registry = MultiSourceRegistry::new();
        if let Some(client) = &http_client {
            registry.add_source(WapmSource::new(
                WapmSource::WAPM_PROD_ENDPOINT.parse().unwrap(),
                client.clone(),
            ));
        }

        Self {
            rt,
            networking,
            http_client,
            engine: None,
            tty: None,
            registry: Arc::new(registry),
            loader: Arc::new(loader),
            module_cache: Arc::new(module_cache::in_memory()),
        }
    }

    pub fn set_networking_implementation<I>(&mut self, net: I) -> &mut Self
    where
        I: VirtualNetworking + Sync,
    {
        self.networking = Arc::new(net);
        self
    }

    pub fn set_engine(&mut self, engine: Option<wasmer::Engine>) -> &mut Self {
        self.engine = engine;
        self
    }

    pub fn set_tty(&mut self, tty: Arc<dyn TtyBridge + Send + Sync>) -> &mut Self {
        self.tty = Some(tty);
        self
    }

    pub fn set_module_cache(
        &mut self,
        module_cache: impl ModuleCache + Send + Sync + 'static,
    ) -> &mut Self {
        self.module_cache = Arc::new(module_cache);
        self
    }

    pub fn set_registry(&mut self, registry: impl Registry + Send + Sync + 'static) -> &mut Self {
        self.registry = Arc::new(registry);
        self
    }

    pub fn set_loader(&mut self, loader: impl PackageLoader + Send + Sync + 'static) -> &mut Self {
        self.loader = Arc::new(loader);
        self
    }
}

impl WasiRuntime for PluggableRuntime {
    fn networking(&self) -> &DynVirtualNetworking {
        &self.networking
    }

    fn http_client(&self) -> Option<&DynHttpClient> {
        self.http_client.as_ref()
    }

    fn package_loader(&self) -> Arc<dyn PackageLoader + Send + Sync> {
        Arc::clone(&self.loader)
    }

    fn registry(&self) -> Arc<dyn Registry + Send + Sync> {
        Arc::clone(&self.registry)
    }

    fn engine(&self) -> Option<wasmer::Engine> {
        self.engine.clone()
    }

    fn new_store(&self) -> wasmer::Store {
        self.engine
            .clone()
            .map(wasmer::Store::new)
            .unwrap_or_default()
    }

    fn task_manager(&self) -> &Arc<dyn VirtualTaskManager> {
        &self.rt
    }

    fn tty(&self) -> Option<&(dyn TtyBridge + Send + Sync)> {
        self.tty.as_deref()
    }

    fn module_cache(&self) -> Arc<dyn ModuleCache + Send + Sync> {
        self.module_cache.clone()
    }
}
