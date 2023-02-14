pub mod task_manager;

pub use self::task_manager::{SpawnType, SpawnedMemory, VirtualTaskManager};

use std::{fmt, sync::Arc};

use futures::future::BoxFuture;
use wasmer_vnet::{DynVirtualNetworking, VirtualNetworking};

use crate::{bin_factory::BinaryPackage, http::DynHttpClient, os::DynTtyBridge};

#[cfg(feature = "sys")]
pub type ArcTunables = std::sync::Arc<dyn wasmer::Tunables + Send + Sync>;

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

    /// Get the [`wasmer::Engine`] to be used for module compilation.
    // TODO: remove this in favor of just support [`ModuleResolver::build_module`].
    #[cfg(feature = "sys")]
    fn engine(&self) -> Option<wasmer::Engine> {
        None
    }

    fn module_resolver(&self) -> Option<&Arc<dyn ModuleResolver>>;

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
    fn tty(&self) -> Option<&DynTtyBridge> {
        None
    }
}

/// Loads webc packages and builds WASM modules.
///
/// Implementors should use a cache to prevent lookups from being too expensive.
pub trait ModuleResolver: Send + Sync + std::fmt::Debug + 'static {
    /// Build (parse and compile) some WASM code into a [`wasmer::Module`].
    fn build_module(&self, wasm: &[u8]) -> Result<wasmer::Module, anyhow::Error>;

    /// Load a (remote) webc package form the given URI.
    ///
    /// The URI can be a full URL, but also just a "namespace/package" or
    /// "namespace/package@version", in which case it shouuld use a default
    /// webc registry.
    fn resolve_webc(
        &self,
        uri: &str,
    ) -> BoxFuture<'static, Result<Arc<webc::WebCMmap>, anyhow::Error>>;

    /// Load a bundles [`BinaryPackage`] from a given webc URI.
    ///
    /// See [`Self::resolve_webc`] for details about the URI.
    fn resolve_binpackage(
        &self,
        uri: &str,
    ) -> BoxFuture<'static, Result<Option<Arc<BinaryPackage>>, anyhow::Error>>;
}

#[cfg(feature = "sys")]
#[derive(Clone, Debug)]
pub struct LocalModuleResolver {
    pub engine: wasmer::Engine,
}

#[cfg(feature = "sys")]
impl ModuleResolver for LocalModuleResolver {
    fn build_module(&self, wasm: &[u8]) -> Result<wasmer::Module, anyhow::Error> {
        wasmer::Module::new(&self.engine, wasm).map_err(anyhow::Error::from)
    }

    fn resolve_webc(
        &self,
        uri: &str,
    ) -> BoxFuture<'static, Result<Arc<webc::WebCMmap>, anyhow::Error>> {
        let res = Err(anyhow::anyhow!(
            "Loading webc packages is not supported on this platform"
        ));
        Box::pin(std::future::ready(res))
    }

    fn resolve_binpackage(
        &self,
        uri: &str,
    ) -> BoxFuture<'static, Result<Option<Arc<BinaryPackage>>, anyhow::Error>> {
        let res = Err(anyhow::anyhow!(
            "Loading webc packages is not supported on this platform"
        ));
        Box::pin(std::future::ready(res))
    }
}

#[derive(Clone, Debug)]
pub struct PluggableRuntimeImplementation {
    pub rt: Arc<dyn VirtualTaskManager>,
    pub networking: DynVirtualNetworking,
    pub http_client: Option<DynHttpClient>,
    #[cfg(feature = "sys")]
    pub engine: Option<wasmer::Engine>,

    pub module_resolver: Option<Arc<dyn ModuleResolver>>,
}

impl PluggableRuntimeImplementation {
    pub fn set_networking_implementation<I>(&mut self, net: I)
    where
        I: VirtualNetworking + Sync,
    {
        self.networking = Arc::new(net)
    }

    #[cfg(feature = "sys")]
    pub fn set_engine(&mut self, engine: Option<wasmer::Engine>) {
        self.engine = engine;
    }

    pub fn new(rt: Arc<dyn VirtualTaskManager>) -> Self {
        // TODO: the cfg flags below should instead be handled by separate implementations.
        cfg_if::cfg_if! {
            if #[cfg(feature = "host-vnet")] {
                let networking = Arc::new(wasmer_wasi_local_networking::LocalNetworking::default());
            } else {
                let networking = Arc::new(wasmer_vnet::UnsupportedVirtualNetworking::default());
            }
        }
        cfg_if::cfg_if! {
            if #[cfg(feature = "host-reqwest")] {
                let http_client = Some(Arc::new(
                    crate::http::reqwest::ReqwestHttpClient::default()) as DynHttpClient
                );
            } else {
                let http_client = None;
            }
        }

        #[cfg(feature = "sys")]
        let module_resolver = Some(Arc::new(LocalModuleResolver {
            engine: wasmer::Store::default().engine().clone(),
        }) as Arc<dyn ModuleResolver>);
        #[cfg(not(feature = "sys"))]
        let module_resolver = None;

        Self {
            rt,
            networking,
            http_client,
            #[cfg(feature = "sys")]
            engine: None,
            module_resolver,
        }
    }
}

impl Default for PluggableRuntimeImplementation {
    #[cfg(feature = "sys-thread")]
    fn default() -> Self {
        let rt = task_manager::tokio::TokioTaskManager::shared();
        let mut s = Self::new(Arc::new(rt));
        let engine = wasmer::Store::default().engine().clone();
        s.engine = Some(engine);
        s
    }
}

impl WasiRuntime for PluggableRuntimeImplementation {
    fn networking(&self) -> &DynVirtualNetworking {
        &self.networking
    }

    fn http_client(&self) -> Option<&DynHttpClient> {
        self.http_client.as_ref()
    }

    fn task_manager(&self) -> &Arc<dyn VirtualTaskManager> {
        &self.rt
    }

    fn module_resolver(&self) -> Option<&Arc<dyn ModuleResolver>> {
        self.module_resolver.as_ref()
    }
}
