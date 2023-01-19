use std::fmt;
use std::ops::Deref;
use std::sync::atomic::{AtomicU32, Ordering};
use thiserror::Error;
use wasmer_vbus::{UnsupportedVirtualBus, VirtualBus};
use wasmer_vnet::VirtualNetworking;
use wasmer_wasi_types::wasi::Errno;

use super::WasiError;
use super::WasiThreadId;

#[derive(Error, Debug)]
pub enum WasiThreadError {
    #[error("Multithreading is not supported")]
    Unsupported,
    #[error("The method named is not an exported function")]
    MethodNotFound,
}

impl From<WasiThreadError> for Errno {
    fn from(a: WasiThreadError) -> Errno {
        match a {
            WasiThreadError::Unsupported => Errno::Notsup,
            WasiThreadError::MethodNotFound => Errno::Inval,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WasiTtyState {
    pub cols: u32,
    pub rows: u32,
    pub width: u32,
    pub height: u32,
    pub stdin_tty: bool,
    pub stdout_tty: bool,
    pub stderr_tty: bool,
    pub echo: bool,
    pub line_buffered: bool,
}

/// Represents an implementation of the WASI runtime - by default everything is
/// unimplemented.
pub trait WasiRuntimeImplementation: fmt::Debug + Sync {
    /// For WASI runtimes that support it they can implement a message BUS implementation
    /// which allows runtimes to pass serialized messages between each other similar to
    /// RPC's. BUS implementation can be implemented that communicate across runtimes
    /// thus creating a distributed computing architecture.
    fn bus(&self) -> &(dyn VirtualBus);

    /// Provides access to all the networking related functions such as sockets.
    /// By default networking is not implemented.
    fn networking(&self) -> &(dyn VirtualNetworking);

    /// Generates a new thread ID
    fn thread_generate_id(&self) -> WasiThreadId;

    /// Gets the TTY state
    fn tty_get(&self) -> WasiTtyState {
        WasiTtyState {
            rows: 25,
            cols: 80,
            width: 800,
            height: 600,
            stdin_tty: false,
            stdout_tty: false,
            stderr_tty: false,
            echo: true,
            line_buffered: true,
        }
    }

    /// Sets the TTY state
    fn tty_set(&self, _tty_state: WasiTtyState) {}

    /// Spawns a new thread by invoking the
    fn thread_spawn(
        &self,
        _callback: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), WasiThreadError> {
        Err(WasiThreadError::Unsupported)
    }

    /// Returns the amount of parallelism that is possible on this platform
    fn thread_parallelism(&self) -> Result<usize, WasiThreadError> {
        Err(WasiThreadError::Unsupported)
    }

    /// Invokes whenever a WASM thread goes idle. In some runtimes (like singlethreaded
    /// execution environments) they will need to do asynchronous work whenever the main
    /// thread goes idle and this is the place to hook for that.
    fn yield_now(&self, _id: WasiThreadId) -> Result<(), WasiError> {
        std::thread::yield_now();
        Ok(())
    }

    /// Gets the current process ID
    fn getpid(&self) -> Option<u32> {
        None
    }
}

#[derive(Debug)]
pub struct PluggableRuntimeImplementation {
    pub bus: Box<dyn VirtualBus + Sync>,
    pub networking: Box<dyn VirtualNetworking + Sync>,
    pub thread_id_seed: AtomicU32,
}

impl PluggableRuntimeImplementation {
    pub fn set_bus_implementation<I>(&mut self, bus: I)
    where
        I: VirtualBus + Sync,
    {
        self.bus = Box::new(bus)
    }

    pub fn set_networking_implementation<I>(&mut self, net: I)
    where
        I: VirtualNetworking + Sync,
    {
        self.networking = Box::new(net)
    }
}

impl Default for PluggableRuntimeImplementation {
    fn default() -> Self {
        Self {
            #[cfg(not(feature = "host-vnet"))]
            networking: Box::new(wasmer_vnet::UnsupportedVirtualNetworking::default()),
            #[cfg(feature = "host-vnet")]
            networking: Box::new(wasmer_wasi_local_networking::LocalNetworking::default()),
            bus: Box::new(UnsupportedVirtualBus::default()),
            thread_id_seed: Default::default(),
        }
    }
}

impl WasiRuntimeImplementation for PluggableRuntimeImplementation {
    fn bus(&self) -> &(dyn VirtualBus) {
        self.bus.deref()
    }

    fn networking(&self) -> &(dyn VirtualNetworking) {
        self.networking.deref()
    }

    fn thread_generate_id(&self) -> WasiThreadId {
        self.thread_id_seed.fetch_add(1, Ordering::Relaxed).into()
    }
}
