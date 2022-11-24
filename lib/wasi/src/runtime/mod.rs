pub mod compiler;
mod stdio;
pub mod task_manager;
mod ws;

pub use self::{
    stdio::*,
    task_manager::{SpawnType, SpawnedMemory, VirtualTaskManager},
    ws::*,
};

use std::{
    fmt,
    future::Future,
    io::{self, Write},
    pin::Pin,
    sync::Arc,
};

use thiserror::Error;
use tracing::*;
use wasmer_vbus::{DefaultVirtualBus, VirtualBus};
use wasmer_vnet::{DynVirtualNetworking, VirtualNetworking};
use wasmer_wasi_types::wasi::Errno;

use crate::{os::tty::WasiTtyState, WasiEnv};

#[cfg(feature = "termios")]
pub mod term;
use crate::http::DynHttpClient;
#[cfg(feature = "termios")]
pub use term::*;

#[derive(Error, Debug)]
pub enum WasiThreadError {
    #[error("Multithreading is not supported")]
    Unsupported,
    #[error("The method named is not an exported function")]
    MethodNotFound,
    #[error("Failed to create the requested memory")]
    MemoryCreateFailed,
    /// This will happen if WASM is running in a thread has not been created by the spawn_wasm call
    #[error("WASM context is invalid")]
    InvalidWasmContext,
}

impl From<WasiThreadError> for Errno {
    fn from(a: WasiThreadError) -> Errno {
        match a {
            WasiThreadError::Unsupported => Errno::Notsup,
            WasiThreadError::MethodNotFound => Errno::Inval,
            WasiThreadError::MemoryCreateFailed => Errno::Fault,
            WasiThreadError::InvalidWasmContext => Errno::Noexec,
        }
    }
}

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
    fn new_task_manager(&self) -> Arc<dyn VirtualTaskManager + Send + Sync + 'static> {
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

    /// Create a new [`wasmer::Store`].
    // TODO: remove default implementation
    // This should be implemented by concrete runtimes.
    // The default impl is here to make migration easier.
    fn new_store(&self, tunables: Option<self::compiler::ArcTunables>) -> wasmer::Store {
        cfg_if::cfg_if! {
            if #[cfg(feature = "sys")] {
                let engine = build_engine();
                if let Some(tunables) = tunables {
                    wasmer::Store::new_with_tunables(engine, tunables)
                } else {
                    wasmer::Store::new(engine)
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

/// Create a new [`wasmer::Engine`] that can be used to compile new modules.
#[cfg(feature = "sys")]
fn build_engine() -> wasmer::Engine {
    // Build the features list
    let mut features = wasmer::Features::new();
    features.threads(true);
    features.memory64(true);
    features.bulk_memory(true);
    #[cfg(feature = "singlepass")]
    features.multi_value(false);

    // Choose the right compiler
    #[cfg(feature = "compiler-cranelift")]
    {
        let compiler = wasmer_compiler_cranelift::Cranelift::default();
        return wasmer_compiler::EngineBuilder::new(compiler)
            .set_features(Some(features))
            .engine();
    }
    #[cfg(all(not(feature = "compiler-cranelift"), feature = "compiler-llvm"))]
    {
        let compiler = wasmer_compiler_llvm::LLVM::default();
        return wasmer_compiler::EngineBuilder::new(compiler)
            .set_features(Some(features))
            .engine();
    }
    #[cfg(all(
        not(feature = "compiler-cranelift"),
        not(feature = "compiler-singlepass"),
        feature = "compiler-llvm"
    ))]
    {
        let compiler = wasmer_compiler_singlepass::Singlepass::default();
        return wasmer_compiler::EngineBuilder::new(compiler)
            .set_features(Some(features))
            .engine();
    }
    #[cfg(all(
        not(feature = "compiler-cranelift"),
        not(feature = "compiler-singlepass"),
        not(feature = "compiler-llvm")
    ))]
    panic!("wasmer not built with a compiler")
}

#[derive(Debug)]
pub struct PluggableRuntimeImplementation {
    pub bus: Arc<dyn VirtualBus<WasiEnv> + Send + Sync + 'static>,
    pub networking: DynVirtualNetworking,
    pub http_client: Option<DynHttpClient>,
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
}

impl Default for PluggableRuntimeImplementation {
    fn default() -> Self {
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
            networking,
            bus: Arc::new(DefaultVirtualBus::default()),
            http_client,
        }
    }
}

impl WasiRuntimeImplementation for PluggableRuntimeImplementation {
    fn bus<'a>(&'a self) -> Arc<dyn VirtualBus<WasiEnv> + Send + Sync + 'static> {
        self.bus.clone()
    }

    fn networking<'a>(&'a self) -> DynVirtualNetworking {
        self.networking.clone()
    }

    fn http_client(&self) -> Option<&DynHttpClient> {
        self.http_client.as_ref()
    }
}
