pub mod binary_package;
pub mod bindings;
pub mod capabilities;
pub mod http;
pub mod module_cache;
pub mod package_loader;
pub mod resolver;
pub mod runner;
pub mod snapshot;
pub mod task_manager;
pub mod tty_sys;

pub use self::binary_package::{BinaryPackage, BinaryPackageCommand};
pub use self::runner::Runner;
pub use self::task_manager::{SpawnMemoryType, VirtualTaskManager};
pub use self::tty_sys::{SysTty, TtyBridge, TtyState};

use self::module_cache::{CacheError, ModuleHash};
use self::task_manager::InlineWaker;

use std::{fmt, sync::Arc};

use derivative::Derivative;
use futures::future::BoxFuture;
use virtual_net::{DynVirtualNetworking, VirtualNetworking};
use wasmer::Module;

use crate::{
    http::{DynHttpClient, HttpClient},
    module_cache::{ModuleCache, ThreadLocalCache},
    package_loader::{PackageLoader, UnsupportedPackageLoader},
    resolver::{MultiSource, Source, WapmSource},
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
    fn package_loader(&self) -> Arc<dyn PackageLoader + Send + Sync> {
        Arc::new(UnsupportedPackageLoader::default())
    }

    /// A cache for compiled modules.
    fn module_cache(&self) -> Arc<dyn ModuleCache + Send + Sync> {
        // Return a cache that uses a thread-local variable. This isn't ideal
        // because it allows silently sharing state, possibly between runtimes.
        //
        // That said, it means people will still get *some* level of caching
        // because each cache returned by this default implementation will go
        // through the same thread-local variable.
        Arc::new(ThreadLocalCache::default())
    }

    /// The package registry.
    fn source(&self) -> Arc<dyn Source + Send + Sync>;

    /// Get a [`wasmer::Engine`] for module compilation.
    fn engine(&self) -> wasmer::Engine {
        wasmer::Engine::default()
    }

    /// Create a new [`wasmer::Store`].
    fn new_store(&self) -> wasmer::Store {
        cfg_if::cfg_if! {
            if #[cfg(feature = "sys")] {
                wasmer::Store::new(self.engine())
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

    /// Load a a Webassembly module, trying to use a pre-compiled version if possible.
    fn load_module<'a>(&'a self, wasm: &'a [u8]) -> BoxFuture<'a, Result<Module, anyhow::Error>> {
        let engine = self.engine();
        let module_cache = self.module_cache();

        let task = async move { load_module(&engine, &module_cache, wasm).await };

        Box::pin(task)
    }

    /// Load a a Webassembly module, trying to use a pre-compiled version if possible.
    ///
    /// Non-async version of [`Self::load_module`].
    fn load_module_sync(&self, wasm: &[u8]) -> Result<Module, anyhow::Error> {
        InlineWaker::block_on(self.load_module(wasm))
    }
}

/// Load a a Webassembly module, trying to use a pre-compiled version if possible.
///
// This function exists to provide a reusable baseline implementation for
// implementing [`Runtime::load_module`], so custom logic can be added on top.
pub async fn load_module(
    engine: &wasmer::Engine,
    module_cache: &(dyn ModuleCache + Send + Sync),
    wasm: &[u8],
) -> Result<Module, anyhow::Error> {
    let hash = ModuleHash::sha256(wasm);
    let result = module_cache.load(hash, engine).await;

    match result {
        Ok(module) => return Ok(module),
        Err(CacheError::NotFound) => {}
        Err(other) => {
            tracing::warn!(
                %hash,
                error=&other as &dyn std::error::Error,
                "Unable to load the cached module",
            );
        }
    }

    let module = Module::new(&engine, wasm)?;

    if let Err(e) = module_cache.save(hash, engine, &module).await {
        tracing::warn!(
            %hash,
            error=&e as &dyn std::error::Error,
            "Unable to cache the compiled module",
        );
    }

    Ok(module)
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

        let loader = UnsupportedPackageLoader::default();

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

    pub fn set_http_client(
        &mut self,
        client: impl HttpClient + Send + Sync + 'static,
    ) -> &mut Self {
        self.http_client = Some(Arc::new(client));
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

    fn engine(&self) -> wasmer::Engine {
        if let Some(engine) = self.engine.clone() {
            engine
        } else {
            wasmer::Engine::default()
        }
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
