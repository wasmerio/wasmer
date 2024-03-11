pub mod module_cache;
pub mod package_loader;
pub mod resolver;
pub mod task_manager;

pub use self::task_manager::{SpawnMemoryType, VirtualTaskManager};
use self::{
    module_cache::{CacheError, ModuleHash},
    task_manager::InlineWaker,
};

use std::{
    fmt,
    ops::Deref,
    sync::{Arc, Mutex},
};

use derivative::Derivative;
use futures::future::BoxFuture;
use virtual_net::{DynVirtualNetworking, VirtualNetworking};
use wasmer::{Module, RuntimeError};
use wasmer_wasix_types::wasi::ExitCode;

#[cfg(feature = "journal")]
use crate::journal::DynJournal;
use crate::{
    http::{DynHttpClient, HttpClient},
    os::TtyBridge,
    runtime::{
        module_cache::{ModuleCache, ThreadLocalCache},
        package_loader::{PackageLoader, UnsupportedPackageLoader},
        resolver::{MultiSource, Source, WapmSource},
    },
    WasiTtyState,
};

#[derive(Clone)]
pub enum TaintReason {
    UnknownWasiVersion,
    NonZeroExitCode(ExitCode),
    RuntimeError(RuntimeError),
}

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
        Arc::new(UnsupportedPackageLoader)
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
    fn load_module<'a>(&'a self, wasm: &'a [u8]) -> BoxFuture<'a, anyhow::Result<Module>> {
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

    /// Callback thats invokes whenever the instance is tainted, tainting can occur
    /// for multiple reasons however the most common is a panic within the process
    fn on_taint(&self, _reason: TaintReason) {}

    /// The list of journals which will be used to restore the state of the
    /// runtime at a particular point in time
    #[cfg(feature = "journal")]
    fn journals(&self) -> &'_ Vec<Arc<DynJournal>> {
        &EMPTY_JOURNAL_LIST
    }

    /// The snapshot capturer takes and restores snapshots of the WASM process at specific
    /// points in time by reading and writing log entries
    #[cfg(feature = "journal")]
    fn active_journal(&self) -> Option<&'_ DynJournal> {
        None
    }
}

pub type DynRuntime = dyn Runtime + Send + Sync;

#[cfg(feature = "journal")]
static EMPTY_JOURNAL_LIST: Vec<Arc<DynJournal>> = Vec::new();

/// Load a a Webassembly module, trying to use a pre-compiled version if possible.
///
// This function exists to provide a reusable baseline implementation for
// implementing [`Runtime::load_module`], so custom logic can be added on top.
#[tracing::instrument(level = "debug", skip_all)]
pub async fn load_module(
    engine: &wasmer::Engine,
    module_cache: &(dyn ModuleCache + Send + Sync),
    wasm: &[u8],
) -> Result<Module, anyhow::Error> {
    let hash = ModuleHash::hash(wasm);
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
    #[cfg(feature = "journal")]
    #[derivative(Debug = "ignore")]
    pub journals: Vec<Arc<DynJournal>>,
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

        let loader = UnsupportedPackageLoader;

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
            #[cfg(feature = "journal")]
            journals: Vec::new(),
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

    #[cfg(feature = "journal")]
    pub fn add_journal(&mut self, journal: Arc<DynJournal>) -> &mut Self {
        self.journals.push(journal);
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

    #[cfg(feature = "journal")]
    fn journals(&self) -> &'_ Vec<Arc<DynJournal>> {
        &self.journals
    }

    #[cfg(feature = "journal")]
    fn active_journal(&self) -> Option<&DynJournal> {
        self.journals.iter().last().map(|a| a.as_ref())
    }
}

/// Runtime that allows for certain things to be overridden
/// such as the active journals
#[derive(Clone, Derivative)]
#[derivative(Debug)]
pub struct OverriddenRuntime {
    inner: Arc<DynRuntime>,
    task_manager: Option<Arc<dyn VirtualTaskManager>>,
    networking: Option<DynVirtualNetworking>,
    http_client: Option<DynHttpClient>,
    package_loader: Option<Arc<dyn PackageLoader + Send + Sync>>,
    source: Option<Arc<dyn Source + Send + Sync>>,
    engine: Option<wasmer::Engine>,
    module_cache: Option<Arc<dyn ModuleCache + Send + Sync>>,
    #[derivative(Debug = "ignore")]
    tty: Option<Arc<dyn TtyBridge + Send + Sync>>,
    #[cfg(feature = "journal")]
    #[derivative(Debug = "ignore")]
    journals: Option<Vec<Arc<DynJournal>>>,
}

impl OverriddenRuntime {
    pub fn new(inner: Arc<DynRuntime>) -> Self {
        Self {
            inner,
            task_manager: None,
            networking: None,
            http_client: None,
            package_loader: None,
            source: None,
            engine: None,
            module_cache: None,
            tty: None,
            #[cfg(feature = "journal")]
            journals: None,
        }
    }

    pub fn with_task_manager(mut self, task_manager: Arc<dyn VirtualTaskManager>) -> Self {
        self.task_manager.replace(task_manager);
        self
    }

    pub fn with_networking(mut self, networking: DynVirtualNetworking) -> Self {
        self.networking.replace(networking);
        self
    }

    pub fn with_http_client(mut self, http_client: DynHttpClient) -> Self {
        self.http_client.replace(http_client);
        self
    }

    pub fn with_package_loader(
        mut self,
        package_loader: Arc<dyn PackageLoader + Send + Sync>,
    ) -> Self {
        self.package_loader.replace(package_loader);
        self
    }

    pub fn with_source(mut self, source: Arc<dyn Source + Send + Sync>) -> Self {
        self.source.replace(source);
        self
    }

    pub fn with_engine(mut self, engine: wasmer::Engine) -> Self {
        self.engine.replace(engine);
        self
    }

    pub fn with_module_cache(mut self, module_cache: Arc<dyn ModuleCache + Send + Sync>) -> Self {
        self.module_cache.replace(module_cache);
        self
    }

    #[cfg(feature = "journal")]
    pub fn with_tty(mut self, tty: Arc<dyn TtyBridge + Send + Sync>) -> Self {
        self.tty.replace(tty);
        self
    }

    #[cfg(feature = "journal")]
    pub fn with_journals(mut self, journals: Vec<Arc<DynJournal>>) -> Self {
        self.journals.replace(journals);
        self
    }
}

impl Runtime for OverriddenRuntime {
    fn networking(&self) -> &DynVirtualNetworking {
        if let Some(net) = self.networking.as_ref() {
            net
        } else {
            self.inner.networking()
        }
    }

    fn task_manager(&self) -> &Arc<dyn VirtualTaskManager> {
        if let Some(rt) = self.task_manager.as_ref() {
            rt
        } else {
            self.inner.task_manager()
        }
    }

    fn source(&self) -> Arc<dyn Source + Send + Sync> {
        if let Some(source) = self.source.clone() {
            source
        } else {
            self.inner.source()
        }
    }

    fn package_loader(&self) -> Arc<dyn PackageLoader + Send + Sync> {
        if let Some(loader) = self.package_loader.clone() {
            loader
        } else {
            self.inner.package_loader()
        }
    }

    fn module_cache(&self) -> Arc<dyn ModuleCache + Send + Sync> {
        if let Some(cache) = self.module_cache.clone() {
            cache
        } else {
            self.inner.module_cache()
        }
    }

    fn engine(&self) -> wasmer::Engine {
        if let Some(engine) = self.engine.clone() {
            engine
        } else {
            self.inner.engine()
        }
    }

    fn new_store(&self) -> wasmer::Store {
        if let Some(engine) = self.engine.clone() {
            wasmer::Store::new(engine)
        } else {
            self.inner.new_store()
        }
    }

    fn http_client(&self) -> Option<&DynHttpClient> {
        if let Some(client) = self.http_client.as_ref() {
            Some(client)
        } else {
            self.inner.http_client()
        }
    }

    fn tty(&self) -> Option<&(dyn TtyBridge + Send + Sync)> {
        if let Some(tty) = self.tty.as_ref() {
            Some(tty.deref())
        } else {
            self.inner.tty()
        }
    }

    #[cfg(feature = "journal")]
    fn journals(&self) -> &'_ Vec<Arc<DynJournal>> {
        if let Some(journals) = self.journals.as_ref() {
            journals
        } else {
            self.inner.journals()
        }
    }

    #[cfg(feature = "journal")]
    fn active_journal(&self) -> Option<&'_ DynJournal> {
        if let Some(journals) = self.journals.as_ref() {
            journals.iter().last().map(|a| a.as_ref())
        } else {
            self.inner.active_journal()
        }
    }

    fn load_module<'a>(&'a self, wasm: &'a [u8]) -> BoxFuture<'a, anyhow::Result<Module>> {
        if self.engine.is_some() || self.module_cache.is_some() {
            let engine = self.engine();
            let module_cache = self.module_cache();
            let task = async move { load_module(&engine, &module_cache, wasm).await };
            Box::pin(task)
        } else {
            self.inner.load_module(wasm)
        }
    }

    fn load_module_sync(&self, wasm: &[u8]) -> Result<Module, anyhow::Error> {
        if self.engine.is_some() || self.module_cache.is_some() {
            InlineWaker::block_on(self.load_module(wasm))
        } else {
            self.inner.load_module_sync(wasm)
        }
    }
}
