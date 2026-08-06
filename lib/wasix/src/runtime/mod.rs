pub mod module_cache;
pub mod package_loader;
pub mod resolver;
pub mod task_manager;

use self::module_cache::CacheError;
pub use self::task_manager::{SpawnType, VirtualTaskManager};
use module_cache::HashedModuleData;
use wasmer_types::{CompilationProgressCallback, ModuleHash};

use std::{
    borrow::Cow,
    fmt,
    ops::Deref,
    sync::{Arc, Mutex},
};

use anyhow::Context as _;
use futures::future::BoxFuture;
use virtual_mio::block_on;
use virtual_net::{DynVirtualNetworking, VirtualNetworking};
use wasmer::{Engine, Module, RuntimeError};
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

/// Opaque per-instantiation state, created by
/// [`InstantiationHook::additional_imports`] and handed back to
/// [`InstantiationHook::configure_new_instance`] for the instance built with
/// those imports.
///
/// Hooks put the data they need to carry between the two phases in with
/// [`InstantiationState::new`] and get it back out with
/// [`InstantiationState::take`]. Callers only pass the value along, unmodified.
// Carrying the state through the instantiation, instead of parking it in the
// hook, is what makes concurrent instantiations safe: an implementation never
// has to guess which pending instantiation a configure_new_instance call
// belongs to, so two threads cold-starting the same module in different
// stores cannot receive each other's state.
#[derive(Default)]
pub struct InstantiationState {
    state: Option<Box<dyn std::any::Any + Send>>,
}

impl InstantiationState {
    /// State that carries no data, for hooks that need nothing from the import
    /// phase.
    pub fn empty() -> Self {
        Self { state: None }
    }

    /// Carries `state` from the import phase to the instance setup phase.
    pub fn new<T: std::any::Any + Send>(state: T) -> Self {
        Self {
            state: Some(Box::new(state)),
        }
    }

    /// Whether this state carries no data.
    pub fn is_empty(&self) -> bool {
        self.state.is_none()
    }

    /// Takes back the data stored by [`InstantiationState::new`].
    ///
    /// Fails if the state is empty or holds a different type, both of which
    /// mean it did not come from the matching import phase.
    pub fn take<T: std::any::Any + Send>(self) -> anyhow::Result<T> {
        let state = self
            .state
            .context("missing instantiation state from the import phase")?;
        state
            .downcast::<T>()
            .map(|state| *state)
            .map_err(|_| anyhow::anyhow!("instantiation state does not belong to this hook"))
    }
}

impl fmt::Debug for InstantiationState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            f.write_str("InstantiationState::empty()")
        } else {
            f.write_str("InstantiationState(..)")
        }
    }
}

/// A hook into the instantiation of WASIX module instances.
///
/// Registered on [`PluggableRuntime::with_instantiation_hook`] or
/// [`OverriddenRuntime::with_instantiation_hook`], and invoked once per
/// instance the runtime creates (process bootstrap, thread spawn, dynamically
/// linked side module).
///
/// Both methods have a no-op default, so an implementation only needs to
/// provide the phases it cares about.
// Keeping both phases on one trait is what lets each hook route its own
// InstantiationState from its import phase to its own setup phase when
// several hooks are registered on the same runtime.
pub trait InstantiationHook: fmt::Debug + Send + Sync + 'static {
    /// Creates additional imports for an instance about to be created in
    /// `store`.
    ///
    /// The returned [`InstantiationState`] is handed back to
    /// [`InstantiationHook::configure_new_instance`] for the instance built
    /// with these imports. If instantiation fails, the state is dropped.
    fn additional_imports(
        &self,
        module: &wasmer::Module,
        store: &mut wasmer::StoreMut,
    ) -> anyhow::Result<(wasmer::Imports, InstantiationState)> {
        let _ = (module, store);
        Ok((wasmer::Imports::new(), InstantiationState::empty()))
    }

    /// Configures an instantiated instance before initialization/startup.
    ///
    /// `state` is the [`InstantiationState`] this hook returned from the
    /// [`InstantiationHook::additional_imports`] call whose imports the
    /// instance was created with.
    fn configure_new_instance(
        &self,
        module: &wasmer::Module,
        store: &mut wasmer::StoreMut,
        instance: &wasmer::Instance,
        imported_memory: Option<&wasmer::Memory>,
        state: InstantiationState,
    ) -> anyhow::Result<()> {
        let _ = (module, store, instance, imported_memory, state);
        Ok(())
    }
}

impl<H: InstantiationHook + ?Sized> InstantiationHook for Arc<H> {
    fn additional_imports(
        &self,
        module: &wasmer::Module,
        store: &mut wasmer::StoreMut,
    ) -> anyhow::Result<(wasmer::Imports, InstantiationState)> {
        (**self).additional_imports(module, store)
    }

    fn configure_new_instance(
        &self,
        module: &wasmer::Module,
        store: &mut wasmer::StoreMut,
        instance: &wasmer::Instance,
        imported_memory: Option<&wasmer::Memory>,
        state: InstantiationState,
    ) -> anyhow::Result<()> {
        (**self).configure_new_instance(module, store, instance, imported_memory, state)
    }
}

/// Adapts an import-creation closure to [`InstantiationHook`].
struct ImportsOnlyHook<F>(F);

impl<F> fmt::Debug for ImportsOnlyHook<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImportsOnlyHook(..)")
    }
}

impl<F> InstantiationHook for ImportsOnlyHook<F>
where
    F: Fn(&wasmer::Module, &mut wasmer::StoreMut) -> anyhow::Result<wasmer::Imports>
        + Send
        + Sync
        + 'static,
{
    fn additional_imports(
        &self,
        module: &wasmer::Module,
        store: &mut wasmer::StoreMut,
    ) -> anyhow::Result<(wasmer::Imports, InstantiationState)> {
        Ok(((self.0)(module, store)?, InstantiationState::empty()))
    }
}

/// Adapts an instance-setup closure to [`InstantiationHook`].
struct InstanceSetupHook<F>(F);

impl<F> fmt::Debug for InstanceSetupHook<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InstanceSetupHook(..)")
    }
}

impl<F> InstantiationHook for InstanceSetupHook<F>
where
    F: Fn(
            &wasmer::Module,
            &mut wasmer::StoreMut,
            &wasmer::Instance,
            Option<&wasmer::Memory>,
        ) -> anyhow::Result<()>
        + Send
        + Sync
        + 'static,
{
    fn configure_new_instance(
        &self,
        module: &wasmer::Module,
        store: &mut wasmer::StoreMut,
        instance: &wasmer::Instance,
        imported_memory: Option<&wasmer::Memory>,
        _state: InstantiationState,
    ) -> anyhow::Result<()> {
        (self.0)(module, store, instance, imported_memory)
    }
}

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
#[allow(clippy::large_enum_variant)]
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

    /// Create a new [`wasmer::Store`].
    fn new_store(&self) -> wasmer::Store {
        cfg_select! {
            feature = "sys" => {
                wasmer::Store::new(self.engine())
            }
            _ => {
                wasmer::Store::default()
            }
        }
    }

    /// Create additional imports for a new WASIX instance in the provided store.
    ///
    /// This callback may be invoked multiple times (e.g. process bootstrap,
    /// thread spawn), so implementations should create imports that are valid
    /// for the given store each time.
    ///
    /// The returned [`InstantiationState`] is per-instantiation state that the
    /// caller must pass to [`Runtime::configure_new_instance`] once the
    /// instance built from these imports exists. If instantiation fails, the
    /// state is simply dropped.
    fn additional_imports(
        &self,
        _module: &wasmer::Module,
        _store: &mut wasmer::StoreMut,
    ) -> anyhow::Result<(wasmer::Imports, InstantiationState)> {
        Ok((wasmer::Imports::new(), InstantiationState::empty()))
    }

    /// Configure an instantiated instance before initialization/startup.
    ///
    /// `state` must be the [`InstantiationState`] returned by the
    /// [`Runtime::additional_imports`] call whose imports this instance was
    /// created with.
    fn configure_new_instance(
        &self,
        _module: &wasmer::Module,
        _store: &mut wasmer::StoreMut,
        _instance: &wasmer::Instance,
        _imported_memory: Option<&wasmer::Memory>,
        _state: InstantiationState,
    ) -> anyhow::Result<()> {
        Ok(())
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
                ModuleInput::Command(cmd) => self.engine(),
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

/// Load a Webassembly module, trying to use a pre-compiled version if possible.
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
        #[cfg(feature = "sys")]
        {
            if engine.is_sys() {
                use wasmer::sys::NativeEngineExt;
                engine.new_module_with_progress(input.wasm(), p)
            } else {
                Module::new(&engine, input.wasm())
            }
        }
        #[cfg(not(feature = "sys"))]
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
    pub instantiation_hooks: Vec<Arc<dyn InstantiationHook>>,
}

impl PluggableRuntime {
    pub fn new(rt: Arc<dyn VirtualTaskManager>) -> Self {
        // TODO: the cfg flags below should instead be handled by separate implementations.
        cfg_select! {
            feature = "host-vnet" => {
                let networking = Arc::new(virtual_net::host::LocalNetworking::default());
            }
            _ => {
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
            instantiation_hooks: Vec::new(),
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

    /// Registers a hook that only creates additional imports.
    pub fn with_additional_imports(
        &mut self,
        imports: impl Fn(&wasmer::Module, &mut wasmer::StoreMut) -> anyhow::Result<wasmer::Imports>
        + Send
        + Sync
        + 'static,
    ) -> &mut Self {
        self.with_instantiation_hook(ImportsOnlyHook(imports))
    }

    /// Registers a hook that only configures newly created instances.
    pub fn with_instance_setup(
        &mut self,
        callback: impl Fn(
            &wasmer::Module,
            &mut wasmer::StoreMut,
            &wasmer::Instance,
            Option<&wasmer::Memory>,
        ) -> anyhow::Result<()>
        + Send
        + Sync
        + 'static,
    ) -> &mut Self {
        self.with_instantiation_hook(InstanceSetupHook(callback))
    }

    /// Registers a hook that takes part in both phases of instantiation, so it
    /// can carry [`InstantiationState`] from its imports to its instance setup.
    pub fn with_instantiation_hook(&mut self, hook: impl InstantiationHook) -> &mut Self {
        self.instantiation_hooks.push(Arc::new(hook));
        self
    }
}

/// Runs the import phase of `hooks`, returning the merged imports and the
/// per-hook states, aligned by index with `hooks`.
fn run_import_hooks(
    hooks: &[Arc<dyn InstantiationHook>],
    module: &wasmer::Module,
    store: &mut wasmer::StoreMut,
) -> anyhow::Result<(wasmer::Imports, Vec<InstantiationState>)> {
    let mut imports = wasmer::Imports::new();
    let mut states = Vec::with_capacity(hooks.len());
    for hook in hooks {
        let (hook_imports, state) = hook.additional_imports(module, store)?;
        imports.extend(&hook_imports);
        states.push(state);
    }
    Ok((imports, states))
}

/// Composite state used by [`OverriddenRuntime`] to carry the inner
/// runtime's state alongside its own hooks' states.
struct OverriddenInstantiationState {
    inner: InstantiationState,
    own: Vec<InstantiationState>,
}

/// Runs the setup phase of `hooks`, handing each hook the state it produced
/// during the import phase.
fn run_setup_hooks(
    hooks: &[Arc<dyn InstantiationHook>],
    states: Vec<InstantiationState>,
    module: &wasmer::Module,
    store: &mut wasmer::StoreMut,
    instance: &wasmer::Instance,
    imported_memory: Option<&wasmer::Memory>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        states.len() == hooks.len(),
        "instance setup state does not match the registered instantiation hooks \
         (got {} states for {} hooks)",
        states.len(),
        hooks.len(),
    );
    for (hook, state) in hooks.iter().zip(states) {
        hook.configure_new_instance(module, store, instance, imported_memory, state)?;
    }
    Ok(())
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

    fn additional_imports(
        &self,
        module: &wasmer::Module,
        store: &mut wasmer::StoreMut,
    ) -> anyhow::Result<(wasmer::Imports, InstantiationState)> {
        if self.instantiation_hooks.is_empty() {
            return Ok((wasmer::Imports::new(), InstantiationState::empty()));
        }
        let (imports, states) = run_import_hooks(&self.instantiation_hooks, module, store)?;
        Ok((imports, InstantiationState::new(states)))
    }

    fn configure_new_instance(
        &self,
        module: &wasmer::Module,
        store: &mut wasmer::StoreMut,
        instance: &wasmer::Instance,
        imported_memory: Option<&wasmer::Memory>,
        state: InstantiationState,
    ) -> anyhow::Result<()> {
        if self.instantiation_hooks.is_empty() {
            return Ok(());
        }
        let states = state
            .take::<Vec<InstantiationState>>()
            .context("invalid instance setup state from additional_imports")?;
        run_setup_hooks(
            &self.instantiation_hooks,
            states,
            module,
            store,
            instance,
            imported_memory,
        )
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
    instantiation_hooks: Vec<Arc<dyn InstantiationHook>>,
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
            instantiation_hooks: Vec::new(),
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

    /// Registers a hook that only creates additional imports.
    pub fn with_additional_imports(
        self,
        imports: impl Fn(&wasmer::Module, &mut wasmer::StoreMut) -> anyhow::Result<wasmer::Imports>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.with_instantiation_hook(ImportsOnlyHook(imports))
    }

    /// Registers a hook that only configures newly created instances.
    pub fn with_instance_setup(
        self,
        callback: impl Fn(
            &wasmer::Module,
            &mut wasmer::StoreMut,
            &wasmer::Instance,
            Option<&wasmer::Memory>,
        ) -> anyhow::Result<()>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.with_instantiation_hook(InstanceSetupHook(callback))
    }

    /// Registers a hook that takes part in both phases of instantiation, so it
    /// can carry [`InstantiationState`] from its imports to its instance setup.
    pub fn with_instantiation_hook(mut self, hook: impl InstantiationHook) -> Self {
        self.instantiation_hooks.push(Arc::new(hook));
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

    fn additional_imports(
        &self,
        module: &wasmer::Module,
        store: &mut wasmer::StoreMut,
    ) -> anyhow::Result<(wasmer::Imports, InstantiationState)> {
        let (mut imports, inner_state) = self.inner.additional_imports(module, store)?;
        if self.instantiation_hooks.is_empty() && inner_state.is_empty() {
            return Ok((imports, InstantiationState::empty()));
        }
        let (own_imports, own_states) = run_import_hooks(&self.instantiation_hooks, module, store)?;
        imports.extend(&own_imports);
        Ok((
            imports,
            InstantiationState::new(OverriddenInstantiationState {
                inner: inner_state,
                own: own_states,
            }),
        ))
    }

    fn configure_new_instance(
        &self,
        module: &wasmer::Module,
        store: &mut wasmer::StoreMut,
        instance: &wasmer::Instance,
        imported_memory: Option<&wasmer::Memory>,
        state: InstantiationState,
    ) -> anyhow::Result<()> {
        let state = if state.is_empty() {
            anyhow::ensure!(
                self.instantiation_hooks.is_empty(),
                "missing instance setup state from additional_imports"
            );
            OverriddenInstantiationState {
                inner: InstantiationState::empty(),
                own: Vec::new(),
            }
        } else {
            state
                .take::<OverriddenInstantiationState>()
                .context("invalid instance setup state from additional_imports")?
        };
        self.inner
            .configure_new_instance(module, store, instance, imported_memory, state.inner)?;
        run_setup_hooks(
            &self.instantiation_hooks,
            state.own,
            module,
            store,
            instance,
            imported_memory,
        )
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

#[cfg(test)]
mod tests {
    use super::InstantiationState;

    #[test]
    fn instantiation_state_round_trips_the_hook_data() {
        let state = InstantiationState::new(42u32);
        assert!(!state.is_empty());
        assert_eq!(state.take::<u32>().unwrap(), 42);
    }

    #[test]
    fn empty_instantiation_state_carries_nothing() {
        let state = InstantiationState::empty();
        assert!(state.is_empty());
        let err = state.take::<u32>().unwrap_err();
        assert!(err.to_string().contains("missing instantiation state"));
    }

    #[test]
    fn instantiation_state_from_another_hook_is_rejected() {
        // What a hook receiving state that isn't its own must see, rather than
        // silently operating on another instantiation's data.
        let state = InstantiationState::new("some other hook's state");
        let err = state.take::<u32>().unwrap_err();
        assert!(err.to_string().contains("does not belong to this hook"));
    }
}
