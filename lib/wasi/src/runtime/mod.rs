pub mod task_manager;

pub use self::task_manager::{SpawnType, SpawnedMemory, VirtualTaskManager};

use std::{fmt, sync::Arc};

use wasmer_vnet::{DynVirtualNetworking, VirtualNetworking};

use crate::{http::DynHttpClient, os::TtyBridge};

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

    /// Get a [`wasmer::Engine`] for module compilation.
    #[cfg(feature = "sys")]
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
    fn tty(&self) -> Option<&dyn TtyBridge> {
        None
    }
}

#[derive(Clone, Debug)]
pub struct PluggableRuntimeImplementation {
    pub rt: Arc<dyn VirtualTaskManager>,
    pub networking: DynVirtualNetworking,
    pub http_client: Option<DynHttpClient>,
    #[cfg(feature = "sys")]
    pub engine: Option<wasmer::Engine>,
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

        Self {
            rt,
            networking,
            http_client,
            #[cfg(feature = "sys")]
            engine: None,
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

    #[cfg(not(feature = "sys-thread"))]
    fn default() -> Self {
        unimplemented!("Default WasiRuntime is not implemented on this target")
    }
}

impl WasiRuntime for PluggableRuntimeImplementation {
    fn networking(&self) -> &DynVirtualNetworking {
        &self.networking
    }

    fn http_client(&self) -> Option<&DynHttpClient> {
        self.http_client.as_ref()
    }

    #[cfg(feature = "sys")]
    fn engine(&self) -> Option<wasmer::Engine> {
        self.engine.clone()
    }

    fn task_manager(&self) -> &Arc<dyn VirtualTaskManager> {
        &self.rt
    }
}
