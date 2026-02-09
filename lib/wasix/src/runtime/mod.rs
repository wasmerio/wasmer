pub mod module_cache;
pub mod package_loader;
pub mod resolver;
pub mod task_manager;

use self::module_cache::CacheError;
pub use self::task_manager::{SpawnType, VirtualTaskManager};
use module_cache::HashedModuleData;
use wasmer_config::package::SuggestedCompilerOptimizations;
use wasmer_types::{
    CompilationProgressCallback, ModuleHash,
    target::UserCompilerOptimizations as WasmerSuggestedCompilerOptimizations,
};

use std::{
    borrow::Cow,
    fmt,
    ops::Deref,
    sync::{Arc, Mutex},
};

use futures::future::BoxFuture;
use virtual_mio::block_on;
use virtual_net::{DynVirtualNetworking, VirtualNetworking};
use wasmer::{CompileError, Engine, Module, RuntimeError};
use wasmer_wasix_types::wasi::ExitCode;

#[cfg(feature = "journal")]
use crate::journal::{DynJournal, DynReadableJournal};
use crate::{
    SpawnError, WasiTtyState,
    bin_factory::BinaryPackageCommand,
    http::{DynHttpClient, HttpClient},
    os::TtyBridge,
    runtime::{
        module_cache::{
            ModuleCache, ThreadLocalCache,
            progress::{ModuleLoadProgress, ModuleLoadProgressReporter},
        },
        package_loader::{PackageLoader, UnsupportedPackageLoader},
        resolver::{BackendSource, MultiSource, Source},
    },
};

#[derive(Clone)]
pub enum TaintReason {
    UnknownWasiVersion,
    NonZeroExitCode(ExitCode),
    RuntimeError(RuntimeError),
    DlSymbolResolutionFailed(String),
}

/// The input to load a module.
///
/// Exists because the semantics for resolving modules can vary between
/// different sources.
///
/// All variants are wrapped in `Cow` to allow for zero-copy usage when possible.
pub enum ModuleInput<'a> {
    /// Raw bytes.
    Bytes(Cow<'a, [u8]>),
    /// Pre-hashed module data.
    Hashed(Cow<'a, HashedModuleData>),
    /// A binary package command.
    Command(Cow<'a, BinaryPackageCommand>),
}

impl<'a> ModuleInput<'a> {
    /// Convert to an owned version of the module input.
    pub fn to_owned(&'a self) -> ModuleInput<'static> {
        // The manual code below is needed due to compiler issues with the lifetime.
        match self {
            Self::Bytes(Cow::Borrowed(b)) => {
                let v: Vec<u8> = (*b).to_owned();
                let c: Cow<'static, [u8]> = Cow::from(v);
                ModuleInput::Bytes(c)
            }
            Self::Bytes(Cow::Owned(b)) => ModuleInput::Bytes(Cow::Owned((*b).clone())),
            Self::Hashed(Cow::Borrowed(h)) => ModuleInput::Hashed(Cow::Owned((*h).clone())),
            Self::Hashed(Cow::Owned(h)) => ModuleInput::Hashed(Cow::Owned(h.clone())),
            Self::Command(Cow::Borrowed(c)) => ModuleInput::Command(Cow::Owned((*c).clone())),
            Self::Command(Cow::Owned(c)) => ModuleInput::Command(Cow::Owned(c.clone())),
        }
    }

    /// Get the module hash.
    ///
    /// NOTE: may be expensive, depending on the variant.
    pub fn hash(&self) -> ModuleHash {
        match self {
            Self::Bytes(b) => {
                // Hash on the fly
                ModuleHash::new(b)
            }
            Self::Hashed(hashed) => *hashed.hash(),
            Self::Command(cmd) => *cmd.hash(),
        }
    }

    /// Get the raw WebAssembly bytes.
    pub fn wasm(&self) -> &[u8] {
        match self {
            Self::Bytes(b) => b,
            Self::Hashed(hashed) => hashed.wasm().as_ref(),
            Self::Command(cmd) => cmd.atom_ref().as_ref(),
        }
    }

    /// Convert to a `HashedModuleData`.
    ///
    /// May involve cloning and hashing.
    pub fn to_hashed(&self) -> HashedModuleData {
        match self {
            Self::Bytes(b) => HashedModuleData::new(b.as_ref()),
            Self::Hashed(hashed) => hashed.as_ref().clone(),
            Self::Command(cmd) => HashedModuleData::from_command(cmd),
        }
    }
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
    fn engine(&self) -> Engine {
        Engine::default()
    }

    fn engine_with_suggested_opts(
        &self,
        suggested_opts: &SuggestedCompilerOptimizations,
    ) -> Result<Engine, CompileError> {
        let mut engine = self.engine();
        engine.with_opts(&WasmerSuggestedCompilerOptimizations {
            pass_params: suggested_opts.pass_params,
        })?;
        Ok(engine)
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

    /// The primary way to load a module given a module input.
    ///
    /// The engine to use can be optionally provided, otherwise the most appropriate engine
    /// should be selected.
    ///
    /// An optional progress reporter callback can be provided to report progress during module loading.
    fn resolve_module<'a>(
        &'a self,
        input: ModuleInput<'a>,
        engine: Option<&Engine>,
        on_progress: Option<ModuleLoadProgressReporter>,
    ) -> BoxFuture<'a, Result<Module, SpawnError>> {
        let data = input.to_hashed();

        let engine = if let Some(e) = engine {
            e.clone()
        } else {
            match &input {
                ModuleInput::Bytes(_) => self.engine(),
                ModuleInput::Hashed(_) => self.engine(),
                ModuleInput::Command(cmd) => {
                    match self
                        .engine_with_suggested_opts(&cmd.as_ref().suggested_compiler_optimizations)
                    {
                        Ok(engine) => engine,
                        Err(error) => {
                            return Box::pin(async move {
                                Err(SpawnError::CompileError {
                                    module_hash: *data.hash(),
                                    error,
                                })
                            });
                        }
                    }
                }
            }
        };

        let module_cache = self.module_cache();

        let task = async move { load_module(&engine, &module_cache, input, on_progress).await };
        Box::pin(task)
    }

    /// Sync variant of [`Self::resolve_module`].
    fn resolve_module_sync(
        &self,
        input: ModuleInput<'_>,
        engine: Option<&Engine>,
        on_progress: Option<ModuleLoadProgressReporter>,
    ) -> Result<Module, SpawnError> {
        block_on(self.resolve_module(input, engine, on_progress))
    }

    /// Load the module for a command.
    ///
    /// Will load the module from the cache if possible, otherwise will compile.
    ///
    /// NOTE: This always be preferred over [`Self::load_module`] to avoid
    /// re-hashing the module!
    #[deprecated(since = "0.601.0", note = "Use `resolve_module` instead")]
    fn load_command_module(
        &self,
        cmd: &BinaryPackageCommand,
    ) -> BoxFuture<'_, Result<Module, SpawnError>> {
        self.resolve_module(ModuleInput::Command(Cow::Owned(cmd.clone())), None, None)
    }

    /// Sync version of [`Self::load_command_module`].
    #[deprecated(since = "0.601.0", note = "Use `resolve_module_sync` instead")]
    fn load_command_module_sync(&self, cmd: &BinaryPackageCommand) -> Result<Module, SpawnError> {
        block_on(self.resolve_module(ModuleInput::Command(Cow::Borrowed(cmd)), None, None))
    }

    /// Load a WebAssembly module from raw bytes.
    ///
    /// Will load the module from the cache if possible, otherwise will compile.
    #[deprecated(since = "0.601.0", note = "Use `resolve_module` instead")]
    fn load_module<'a>(&'a self, wasm: &'a [u8]) -> BoxFuture<'a, Result<Module, SpawnError>> {
        self.resolve_module(ModuleInput::Bytes(Cow::Borrowed(wasm)), None, None)
    }

    /// Synchronous version of [`Self::load_module`].
    #[deprecated(
        since = "0.601.0",
        note = "Use `load_command_module` or `load_hashed_module` instead - this method can have high overhead"
    )]
    fn load_module_sync(&self, wasm: &[u8]) -> Result<Module, SpawnError> {
        block_on(self.resolve_module(ModuleInput::Bytes(Cow::Borrowed(wasm)), None, None))
    }

    /// Load a WebAssembly module from pre-hashed data.
    ///
    /// Will load the module from the cache if possible, otherwise will compile.
    fn load_hashed_module(
        &self,
        module: HashedModuleData,
        engine: Option<&Engine>,
    ) -> BoxFuture<'_, Result<Module, SpawnError>> {
        self.resolve_module(ModuleInput::Hashed(Cow::Owned(module)), engine, None)
    }

    /// Synchronous version of [`Self::load_hashed_module`].
    fn load_hashed_module_sync(
        &self,
        wasm: HashedModuleData,
        engine: Option<&Engine>,
    ) -> Result<Module, SpawnError> {
        block_on(self.resolve_module(ModuleInput::Hashed(Cow::Owned(wasm)), engine, None))
    }

    /// Callback thats invokes whenever the instance is tainted, tainting can occur
    /// for multiple reasons however the most common is a panic within the process
    fn on_taint(&self, _reason: TaintReason) {}

    /// The list of all read-only journals which will be used to restore the state of the
    /// runtime at a particular point in time
    #[cfg(feature = "journal")]
    fn read_only_journals<'a>(&'a self) -> Box<dyn Iterator<Item = Arc<DynReadableJournal>> + 'a> {
        Box::new(std::iter::empty())
    }

    /// The list of writable journals which will be appended to
    #[cfg(feature = "journal")]
    fn writable_journals<'a>(&'a self) -> Box<dyn Iterator<Item = Arc<DynJournal>> + 'a> {
        Box::new(std::iter::empty())
    }

    /// The snapshot capturer takes and restores snapshots of the WASM process at specific
    /// points in time by reading and writing log entries
    #[cfg(feature = "journal")]
    fn active_journal(&self) -> Option<&'_ DynJournal> {
        None
    }
}

pub type DynRuntime = dyn Runtime + Send + Sync;

/// Load a a Webassembly module, trying to use a pre-compiled version if possible.
///
// This function exists to provide a reusable baseline implementation for
// implementing [`Runtime::load_module`], so custom logic can be added on top.
#[tracing::instrument(level = "debug", skip_all)]
pub async fn load_module(
    engine: &Engine,
    module_cache: &(dyn ModuleCache + Send + Sync),
    input: ModuleInput<'_>,
    on_progress: Option<ModuleLoadProgressReporter>,
) -> Result<Module, crate::SpawnError> {
    let wasm_hash = input.hash();

    let result = if let Some(on_progress) = &on_progress {
        module_cache
            .load_with_progress(wasm_hash, engine, on_progress.clone())
            .await
    } else {
        module_cache.load(wasm_hash, engine).await
    };

    match result {
        Ok(module) => return Ok(module),
        Err(CacheError::NotFound) => {}
        Err(other) => {
            tracing::warn!(
                %wasm_hash,
                error=&other as &dyn std::error::Error,
                "Unable to load the cached module",
            );
        }
    }

    let res = if let Some(progress) = on_progress {
        #[allow(unused_variables)]
        let p = CompilationProgressCallback::new(move |p| {
            progress.notify(ModuleLoadProgress::CompilingModule(p))
        });
        #[cfg(feature = "sys-default")]
        {
            if engine.is_sys() {
                use wasmer::sys::NativeEngineExt;
                engine.new_module_with_progress(input.wasm(), p)
            } else {
                Module::new(&engine, input.wasm())
            }
        }
        #[cfg(not(feature = "sys-default"))]
        {
            Module::new(&engine, input.wasm())
        }
    } else {
        Module::new(&engine, input.wasm())
    };

    let module = res.map_err(|err| crate::SpawnError::CompileError {
        module_hash: wasm_hash,
        error: err,
    })?;

    // TODO: pass a [`HashedModule`] struct that is safe by construction.
    if let Err(e) = module_cache.save(wasm_hash, engine, &module).await {
        tracing::warn!(
            %wasm_hash,
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

#[derive(Debug, Clone)]
pub struct PluggableRuntime {
    pub rt: Arc<dyn VirtualTaskManager>,
    pub networking: DynVirtualNetworking,
    pub http_client: Option<DynHttpClient>,
    pub package_loader: Arc<dyn PackageLoader + Send + Sync>,
    pub source: Arc<dyn Source + Send + Sync>,
    pub engine: Engine,
    pub module_cache: Arc<dyn ModuleCache + Send + Sync>,
    pub tty: Option<Arc<dyn TtyBridge + Send + Sync>>,
    #[cfg(feature = "journal")]
    pub read_only_journals: Vec<Arc<DynReadableJournal>>,
    #[cfg(feature = "journal")]
    pub writable_journals: Vec<Arc<DynJournal>>,
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

        let mut source = MultiSource::default();
        if let Some(client) = &http_client {
            source.add_source(BackendSource::new(
                BackendSource::WASMER_PROD_ENDPOINT.parse().unwrap(),
                client.clone(),
            ));
        }

        Self {
            rt,
            networking,
            http_client,
            engine: Default::default(),
            tty: None,
            source: Arc::new(source),
            package_loader: Arc::new(loader),
            module_cache: Arc::new(module_cache::in_memory()),
            #[cfg(feature = "journal")]
            read_only_journals: Vec::new(),
            #[cfg(feature = "journal")]
            writable_journals: Vec::new(),
        }
    }

    pub fn set_networking_implementation<I>(&mut self, net: I) -> &mut Self
    where
        I: VirtualNetworking + Sync,
    {
        self.networking = Arc::new(net);
        self
    }

    pub fn set_engine(&mut self, engine: Engine) -> &mut Self {
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

    pub fn set_source(&mut self, source: impl Source + Send + 'static) -> &mut Self {
        self.source = Arc::new(source);
        self
    }

    pub fn set_package_loader(
        &mut self,
        package_loader: impl PackageLoader + 'static,
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
    pub fn add_read_only_journal(&mut self, journal: Arc<DynReadableJournal>) -> &mut Self {
        self.read_only_journals.push(journal);
        self
    }

    #[cfg(feature = "journal")]
    pub fn add_writable_journal(&mut self, journal: Arc<DynJournal>) -> &mut Self {
        self.writable_journals.push(journal);
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

    fn engine(&self) -> Engine {
        self.engine.clone()
    }

    fn new_store(&self) -> wasmer::Store {
        wasmer::Store::new(self.engine.clone())
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
    fn read_only_journals<'a>(&'a self) -> Box<dyn Iterator<Item = Arc<DynReadableJournal>> + 'a> {
        Box::new(self.read_only_journals.iter().cloned())
    }

    #[cfg(feature = "journal")]
    fn writable_journals<'a>(&'a self) -> Box<dyn Iterator<Item = Arc<DynJournal>> + 'a> {
        Box::new(self.writable_journals.iter().cloned())
    }

    #[cfg(feature = "journal")]
    fn active_journal(&self) -> Option<&DynJournal> {
        self.writable_journals.iter().last().map(|a| a.as_ref())
    }
}

/// Runtime that allows for certain things to be overridden
/// such as the active journals
#[derive(Clone, Debug)]
pub struct OverriddenRuntime {
    inner: Arc<DynRuntime>,
    task_manager: Option<Arc<dyn VirtualTaskManager>>,
    networking: Option<DynVirtualNetworking>,
    http_client: Option<DynHttpClient>,
    package_loader: Option<Arc<dyn PackageLoader + Send + Sync>>,
    source: Option<Arc<dyn Source + Send + Sync>>,
    engine: Option<Engine>,
    module_cache: Option<Arc<dyn ModuleCache + Send + Sync>>,
    tty: Option<Arc<dyn TtyBridge + Send + Sync>>,
    #[cfg(feature = "journal")]
    pub read_only_journals: Option<Vec<Arc<DynReadableJournal>>>,
    #[cfg(feature = "journal")]
    pub writable_journals: Option<Vec<Arc<DynJournal>>>,
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
            read_only_journals: None,
            #[cfg(feature = "journal")]
            writable_journals: None,
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

    pub fn with_engine(mut self, engine: Engine) -> Self {
        self.engine.replace(engine);
        self
    }

    pub fn with_module_cache(mut self, module_cache: Arc<dyn ModuleCache + Send + Sync>) -> Self {
        self.module_cache.replace(module_cache);
        self
    }

    pub fn with_tty(mut self, tty: Arc<dyn TtyBridge + Send + Sync>) -> Self {
        self.tty.replace(tty);
        self
    }

    #[cfg(feature = "journal")]
    pub fn with_read_only_journals(mut self, journals: Vec<Arc<DynReadableJournal>>) -> Self {
        self.read_only_journals.replace(journals);
        self
    }

    #[cfg(feature = "journal")]
    pub fn with_writable_journals(mut self, journals: Vec<Arc<DynJournal>>) -> Self {
        self.writable_journals.replace(journals);
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

    fn engine(&self) -> Engine {
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
    fn read_only_journals<'a>(&'a self) -> Box<dyn Iterator<Item = Arc<DynReadableJournal>> + 'a> {
        if let Some(journals) = self.read_only_journals.as_ref() {
            Box::new(journals.iter().cloned())
        } else {
            self.inner.read_only_journals()
        }
    }

    #[cfg(feature = "journal")]
    fn writable_journals<'a>(&'a self) -> Box<dyn Iterator<Item = Arc<DynJournal>> + 'a> {
        if let Some(journals) = self.writable_journals.as_ref() {
            Box::new(journals.iter().cloned())
        } else {
            self.inner.writable_journals()
        }
    }

    #[cfg(feature = "journal")]
    fn active_journal(&self) -> Option<&'_ DynJournal> {
        if let Some(journals) = self.writable_journals.as_ref() {
            journals.iter().last().map(|a| a.as_ref())
        } else {
            self.inner.active_journal()
        }
    }
}
