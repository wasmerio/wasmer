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
use virtual_net::{DynVirtualNetworking, VirtualNetworking};

use crate::{
    http::DynHttpClient,
    os::TtyBridge,
    runtime::{
        module_cache::ModuleCache,
        package_loader::{BuiltinPackageLoader, PackageLoader},
        resolver::{MultiSource, Source, WapmSource},
    },
    WasiTtyState,
};

/// Runtime components used when running WebAssembly programs.
///
/// Think of this as the "System" in "WebAssembly Systems Interface".
#[allow(unused_variables)]
pub trait Runtime
where
    Self: fmt::Debug,
{
    /// Provides access to all the networking related functions such as sockets.
    fn networking(&self) -> &DynVirtualNetworking;

    /// Retrieve the active [`VirtualTaskManager`].
    fn task_manager(&self) -> &Arc<dyn VirtualTaskManager>;

    /// A package loader.
    fn package_loader(&self) -> Arc<dyn PackageLoader + Send + Sync>;

    /// A cache for compiled modules.
    fn module_cache(&self) -> Arc<dyn ModuleCache + Send + Sync>;

    /// The package registry.
    fn source(&self) -> Arc<dyn Source + Send + Sync>;

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

    /// Get a custom HTTP client
    fn http_client(&self) -> Option<&DynHttpClient> {
        None
    }

    /// Get access to the TTY used by the environment.
    fn tty(&self) -> Option<&(dyn TtyBridge + Send + Sync)> {
        None
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
    pub package_loader: Arc<dyn PackageLoader + Send + Sync>,
    pub source: Arc<dyn Source + Send + Sync>,
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

        let loader = BuiltinPackageLoader::from_env()
            .expect("Loading the builtin resolver should never fail");

        let mut source = MultiSource::new();
        if let Some(client) = &http_client {
            source.add_source(WapmSource::new(
                WapmSource::WASMER_PROD_ENDPOINT.parse().unwrap(),
                client.clone(),
            ));
        }

        Self {
            rt,
            networking,
            http_client,
            engine: None,
            tty: None,
            source: Arc::new(source),
            package_loader: Arc::new(loader),
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

    pub fn set_source(&mut self, source: impl Source + Send + Sync + 'static) -> &mut Self {
        self.source = Arc::new(source);
        self
    }

    pub fn set_package_loader(
        &mut self,
        package_loader: impl PackageLoader + Send + Sync + 'static,
    ) -> &mut Self {
        self.package_loader = Arc::new(package_loader);
        self
    }
}

impl Runtime for PluggableRuntime {
    fn networking(&self) -> &DynVirtualNetworking {
        &self.networking
    }

    fn http_client(&self) -> Option<&DynHttpClient> {
        self.http_client.as_ref()
    }

    fn package_loader(&self) -> Arc<dyn PackageLoader + Send + Sync> {
        Arc::clone(&self.package_loader)
    }

    fn source(&self) -> Arc<dyn Source + Send + Sync> {
        Arc::clone(&self.source)
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
