pub mod module_cache;
pub mod resolver;
pub mod task_manager;

pub use self::task_manager::{SpawnMemoryType, VirtualTaskManager};

use std::{
    fmt,
    sync::{Arc, Mutex},
};

use anyhow::Context;
use derivative::Derivative;
use virtual_net::{DynVirtualNetworking, VirtualNetworking};
use wasmer::Module;

use crate::{
    http::DynHttpClient,
    os::TtyBridge,
    runtime::{
        module_cache::{CacheError, ModuleCache, ModuleHash},
        resolver::{PackageResolver, RegistryResolver},
    },
    WasiTtyState,
};

/// Represents an implementation of the WASI runtime - by default everything is
/// unimplemented.
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

    fn package_resolver(&self) -> Arc<dyn PackageResolver + Send + Sync>;

    /// A cache for compiled modules.
    fn module_cache(&self) -> Arc<dyn ModuleCache + Send + Sync>;

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
    pub resolver: Arc<dyn PackageResolver + Send + Sync>,
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

        let resolver =
            RegistryResolver::from_env().expect("Loading the builtin resolver should never fail");

        Self {
            rt,
            networking,
            http_client,
            engine: None,
            tty: None,
            resolver: Arc::new(resolver),
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

    pub fn set_module_cache<M>(&mut self, module_cache: M) -> &mut Self
    where
        M: ModuleCache + Send + Sync + 'static,
    {
        self.module_cache = Arc::new(module_cache);
        self
    }

    pub fn set_resolver(
        &mut self,
        resolver: impl PackageResolver + Send + Sync + 'static,
    ) -> &mut Self {
        self.resolver = Arc::new(resolver);
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

    fn package_resolver(&self) -> Arc<dyn PackageResolver + Send + Sync> {
        Arc::clone(&self.resolver)
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

/// Compile a module, trying to use a pre-compiled version if possible.
pub(crate) fn compile_module(
    wasm: &[u8],
    runtime: &dyn WasiRuntime,
) -> Result<Module, anyhow::Error> {
    let engine = runtime.engine().context("No engine provided")?;
    let task_manager = runtime.task_manager().clone();
    let module_cache = runtime.module_cache();

    let hash = ModuleHash::sha256(wasm);
    let result = task_manager.block_on(module_cache.load(hash, &engine));

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

    if let Err(e) = task_manager.block_on(module_cache.save(hash, &engine, &module)) {
        tracing::warn!(
            %hash,
            error=&e as &dyn std::error::Error,
            "Unable to cache the compiled module",
        );
    }

    Ok(module)
}
