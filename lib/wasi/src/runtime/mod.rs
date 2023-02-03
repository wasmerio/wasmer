mod stdio;
pub mod task_manager;
mod ws;

use self::task_manager::StubTaskManager;
pub use self::{
    stdio::*,
    task_manager::{SpawnType, SpawnedMemory, VirtualTaskManager, VirtualTaskManagerExt},
    ws::*,
};

use std::{
    fmt,
    future::Future,
    io::{self, Write},
    pin::Pin,
    sync::Arc,
};

use wasmer_vbus::{DefaultVirtualBus, VirtualBus};
use wasmer_vnet::{DynVirtualNetworking, VirtualNetworking};

use crate::{os::tty::WasiTtyState, WasiEnv};

#[cfg(feature = "termios")]
pub mod term;
use crate::http::DynHttpClient;
#[cfg(feature = "termios")]
pub use term::*;

#[cfg(feature = "sys")]
pub type ArcTunables = std::sync::Arc<dyn wasmer::Tunables + Send + Sync>;

/// Represents an implementation of the WASI runtime - by default everything is
/// unimplemented.
#[allow(unused_variables)]
pub trait WasiRuntimeImplementation
where
    Self: fmt::Debug + Sync,
{
    /// For WASI runtimes that support it they can implement a message BUS implementation
    /// which allows runtimes to pass serialized messages between each other similar to
    /// RPC's. BUS implementation can be implemented that communicate across runtimes
    /// thus creating a distributed computing architecture.
    fn bus(&self) -> Arc<dyn VirtualBus<WasiEnv> + Send + Sync + 'static>;

    /// Provides access to all the networking related functions such as sockets.
    /// By default networking is not implemented.
    fn networking(&self) -> DynVirtualNetworking;

    /// Create a new task management runtime
    fn new_task_manager(&self) -> Arc<dyn VirtualTaskManager> {
        // FIXME: move this to separate thread implementors.
        cfg_if::cfg_if! {
            if #[cfg(feature = "sys-thread")] {
                Arc::new(task_manager::tokio::TokioTaskManager::default())
            } else {
                Arc::new(task_manager::StubTaskManager)
            }
        }
    }

    /// Gets the TTY state
    #[cfg(not(feature = "host-termios"))]
    fn tty_get(&self) -> WasiTtyState {
        Default::default()
    }

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

    /// Sets the TTY state
    #[cfg(not(feature = "host-termios"))]
    fn tty_set(&self, _tty_state: WasiTtyState) {}

    #[cfg(feature = "host-termios")]
    fn tty_get(&self) -> WasiTtyState {
        let mut echo = false;
        let mut line_buffered = false;
        let mut line_feeds = false;

        if let Ok(termios) = termios::Termios::from_fd(0) {
            echo = (termios.c_lflag & termios::ECHO) != 0;
            line_buffered = (termios.c_lflag & termios::ICANON) != 0;
            line_feeds = (termios.c_lflag & termios::ONLCR) != 0;
        }

        if let Some((w, h)) = term_size::dimensions() {
            WasiTtyState {
                cols: w as u32,
                rows: h as u32,
                width: 800,
                height: 600,
                stdin_tty: true,
                stdout_tty: true,
                stderr_tty: true,
                echo,
                line_buffered,
                line_feeds,
            }
        } else {
            WasiTtyState {
                rows: 80,
                cols: 25,
                width: 800,
                height: 600,
                stdin_tty: true,
                stdout_tty: true,
                stderr_tty: true,
                echo,
                line_buffered,
                line_feeds,
            }
        }
    }

    /// Sets the TTY state
    #[cfg(feature = "host-termios")]
    fn tty_set(&self, tty_state: WasiTtyState) {
        if tty_state.echo {
            set_mode_echo();
        } else {
            set_mode_no_echo();
        }
        if tty_state.line_buffered {
            set_mode_line_buffered();
        } else {
            set_mode_no_line_buffered();
        }
        if tty_state.line_feeds {
            set_mode_line_feeds();
        } else {
            set_mode_no_line_feeds();
        }
    }

    /// Returns a HTTP client
    fn http_client(&self) -> Option<&DynHttpClient> {
        None
    }

    /// Make a web socket connection to a particular URL
    #[cfg(not(feature = "host-ws"))]
    fn web_socket(
        &self,
        url: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn WebSocketAbi>, String>>>> {
        Box::pin(async move { Err("not supported".to_string()) })
    }

    /// Make a web socket connection to a particular URL
    #[cfg(feature = "host-ws")]
    fn web_socket(
        &self,
        url: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn WebSocketAbi>, String>>>> {
        let url = url.to_string();
        Box::pin(async move { Box::new(TerminalWebSocket::new(url.as_str())).await })
    }

    /// Writes output to the console
    fn stdout(&self, data: &[u8]) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + Sync>> {
        let data = data.to_vec();
        Box::pin(async move {
            let mut handle = io::stdout();
            handle.write_all(&data[..])
        })
    }

    /// Writes output to the console
    fn stderr(&self, data: &[u8]) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + Sync>> {
        let data = data.to_vec();
        Box::pin(async move {
            let mut handle = io::stderr();
            handle.write_all(&data[..])
        })
    }

    /// Flushes the output to the console
    fn flush(&self) -> Pin<Box<dyn Future<Output = io::Result<()>>>> {
        Box::pin(async move {
            io::stdout().flush()?;
            io::stderr().flush()?;
            Ok(())
        })
    }

    /// Writes output to the log
    fn log(&self, text: String) -> Pin<Box<dyn Future<Output = io::Result<()>>>> {
        Box::pin(async move {
            tracing::info!("{}", text);
            Ok(())
        })
    }

    /// Clears the terminal
    fn cls(&self) -> Pin<Box<dyn Future<Output = io::Result<()>>>> {
        Box::pin(async move {
            let mut handle = io::stdout();
            handle.write_all("\x1B[H\x1B[2J".as_bytes())
        })
    }
}

#[derive(Debug)]
pub struct PluggableRuntimeImplementation {
    pub rt: Option<Arc<dyn VirtualTaskManager>>,
    pub bus: Arc<dyn VirtualBus<WasiEnv> + Send + Sync + 'static>,
    pub networking: DynVirtualNetworking,
    pub http_client: Option<DynHttpClient>,
    #[cfg(feature = "sys")]
    pub engine: Option<wasmer::Engine>,
}

impl PluggableRuntimeImplementation {
    pub fn set_bus_implementation<I>(&mut self, bus: I)
    where
        I: VirtualBus<WasiEnv> + Sync,
    {
        self.bus = Arc::new(bus)
    }

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

    pub fn new(rt: Option<Arc<dyn VirtualTaskManager>>) -> Self {
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
            bus: Arc::new(DefaultVirtualBus::default()),
            http_client,
            #[cfg(feature = "sys")]
            engine: None,
        }
    }
}

impl Default for PluggableRuntimeImplementation {
    fn default() -> Self {
        #[cfg(feature = "sys-thread")]
        let rt = Some(Arc::new(task_manager::tokio::TokioTaskManager::default())
            as Arc<dyn VirtualTaskManager>);
        #[cfg(not(feature = "sys-thread"))]
        let rt = None;

        Self::new(rt)
    }
}

impl WasiRuntimeImplementation for PluggableRuntimeImplementation {
    fn bus(&self) -> Arc<dyn VirtualBus<WasiEnv> + Send + Sync + 'static> {
        self.bus.clone()
    }

    fn networking(&self) -> DynVirtualNetworking {
        self.networking.clone()
    }

    fn http_client(&self) -> Option<&DynHttpClient> {
        self.http_client.as_ref()
    }

    #[cfg(feature = "sys")]
    fn engine(&self) -> Option<wasmer::Engine> {
        self.engine.clone()
    }

    fn new_task_manager(&self) -> Arc<dyn VirtualTaskManager> {
        if let Some(rt) = &self.rt {
            rt.clone()
        } else {
            Arc::new(StubTaskManager)
        }
    }
}
