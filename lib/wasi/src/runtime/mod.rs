// FIXME: figure out why file exists, but is not in module tree

use std::{
    fmt,
    future::Future,
    io::{self, Write},
    pin::Pin,
    sync::Arc,
};

use derivative::Derivative;
use thiserror::Error;
use tracing::*;
use wasmer::{vm::VMMemory, MemoryType, Module, Store};
#[cfg(feature = "sys")]
use wasmer_types::MemoryStyle;
use wasmer_vbus::{DefaultVirtualBus, VirtualBus};
use wasmer_vnet::VirtualNetworking;
use wasmer_wasi_types::wasi::Errno;

use crate::{os::tty::WasiTtyState, WasiCallingId, WasiEnv};

mod ws;
pub use ws::*;

mod stdio;
pub use stdio::*;

#[cfg(feature = "termios")]
pub mod term;
use crate::http::{HttpRequestOptions, HttpResponse};
#[cfg(feature = "termios")]
pub use term::*;
#[cfg(feature = "sys-thread")]
use tokio::runtime::{Builder, Runtime};

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

#[derive(Debug)]
pub struct SpawnedMemory {
    pub ty: MemoryType,
    #[cfg(feature = "sys")]
    pub style: MemoryStyle,
}

#[derive(Debug)]
pub enum SpawnType {
    Create,
    CreateWithType(SpawnedMemory),
    NewThread(VMMemory),
}

/// An implementation of task management
#[allow(unused_variables)]
pub trait VirtualTaskManager: fmt::Debug + Send + Sync + 'static {
    /// Invokes whenever a WASM thread goes idle. In some runtimes (like singlethreaded
    /// execution environments) they will need to do asynchronous work whenever the main
    /// thread goes idle and this is the place to hook for that.
    fn sleep_now(
        &self,
        _id: WasiCallingId,
        ms: u128,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + Sync + 'static>>;

    /// Starts an asynchronous task that will run on a shared worker pool
    /// This task must not block the execution or it could cause a deadlock
    fn task_shared(
        &self,
        task: Box<
            dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + 'static,
        >,
    ) -> Result<(), WasiThreadError>;

    /// Starts an asynchronous task on the local thread (by running it in a runtime)
    fn block_on<'a>(&self, task: Pin<Box<dyn Future<Output = ()> + 'a>>);

    /// Starts an asynchronous task on the local thread (by running it in a runtime)
    fn enter<'a>(&'a self) -> Box<dyn std::any::Any + 'a>;

    /// Starts an asynchronous task will will run on a dedicated thread
    /// pulled from the worker pool that has a stateful thread local variable
    /// It is ok for this task to block execution and any async futures within its scope
    fn task_wasm(
        &self,
        task: Box<dyn FnOnce(Store, Module, Option<VMMemory>) + Send + 'static>,
        store: Store,
        module: Module,
        spawn_type: SpawnType,
    ) -> Result<(), WasiThreadError>;

    /// Starts an asynchronous task will will run on a dedicated thread
    /// pulled from the worker pool. It is ok for this task to block execution
    /// and any async futures within its scope
    fn task_dedicated(
        &self,
        task: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), WasiThreadError>;

    /// Starts an asynchronous task will will run on a dedicated thread
    /// pulled from the worker pool. It is ok for this task to block execution
    /// and any async futures within its scope
    fn task_dedicated_async(
        &self,
        task: Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + 'static>> + Send + 'static>,
    ) -> Result<(), WasiThreadError>;

    /// Returns the amount of parallelism that is possible on this platform
    fn thread_parallelism(&self) -> Result<usize, WasiThreadError>;
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
    fn networking(&self) -> Arc<dyn VirtualNetworking + Send + Sync + 'static>;

    /// Create a new task management runtime
    fn new_task_manager(&self) -> Arc<dyn VirtualTaskManager + Send + Sync + 'static> {
        Arc::new(DefaultTaskManager::default())
    }

    /// Gets the TTY state
    #[cfg(not(feature = "host-termios"))]
    fn tty_get(&self) -> WasiTtyState {
        Default::default()
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

    /// Performs a HTTP or HTTPS request to a destination URL
    #[cfg(not(feature = "host-reqwest"))]
    fn reqwest(
        &self,
        tasks: &dyn VirtualTaskManager,
        url: &str,
        method: &str,
        options: HttpRequestOptions,
        headers: Vec<(String, String)>,
        data: Option<Vec<u8>>,
    ) -> Pin<Box<dyn Future<Output = Result<HttpResponse, Errno>>>> {
        Box::pin(async move { Err(Errno::Notsup) })
    }

    /// Performs a HTTP or HTTPS request to a destination URL
    #[cfg(feature = "host-reqwest")]
    fn reqwest(
        &self,
        tasks: &dyn VirtualTaskManager,
        url: &str,
        method: &str,
        _options: HttpRequestOptions,
        headers: Vec<(String, String)>,
        data: Option<Vec<u8>>,
    ) -> Pin<Box<dyn Future<Output = Result<HttpResponse, Errno>>>> {
        use std::convert::TryFrom;

        let url = url.to_string();
        let method = method.to_string();

        Box::pin(async move {
            let method = reqwest::Method::try_from(method.as_str()).map_err(|err| {
                debug!("failed to convert method ({}) - {}", method, err);
                Errno::Io
            })?;

            let client = reqwest::ClientBuilder::default().build().map_err(|err| {
                debug!("failed to build reqwest client - {}", err);
                Errno::Io
            })?;

            let mut builder = client.request(method, url.as_str());
            for (header, val) in headers {
                if let Ok(header) = reqwest::header::HeaderName::from_bytes(header.as_bytes()) {
                    builder = builder.header(header, val);
                } else {
                    debug!("failed to parse header - {}", header);
                }
            }

            if let Some(data) = data {
                builder = builder.body(reqwest::Body::from(data));
            }

            let request = builder.build().map_err(|err| {
                debug!("failed to convert request (url={}) - {}", url.as_str(), err);
                Errno::Io
            })?;

            let response = client.execute(request).await.map_err(|err| {
                debug!("failed to execute reqest - {}", err);
                Errno::Io
            })?;

            let status = response.status().as_u16();
            let status_text = response.status().as_str().to_string();
            let data = response.bytes().await.map_err(|err| {
                debug!("failed to read response bytes - {}", err);
                Errno::Io
            })?;
            let data = data.to_vec();

            Ok(HttpResponse {
                pos: 0usize,
                ok: true,
                status,
                status_text,
                redirected: false,
                data: Some(data),
                headers: Vec::new(),
            })
        })
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
    #[cfg(feature = "tracing")]
    fn log(&self, text: String) -> Pin<Box<dyn Future<Output = io::Result<()>>>> {
        Box::pin(async move {
            tracing::info!("{}", text);
            Ok(())
        })
    }

    /// Writes output to the log
    #[cfg(not(feature = "tracing"))]
    fn log(&self, text: String) -> Pin<Box<dyn Future<Output = io::Result<()>>>> {
        Box::pin(async move {
            let text = format!("{}\r\n", text);
            let mut handle = io::stderr();
            handle.write_all(text.as_bytes())
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

#[derive(Derivative)]
#[derivative(Debug)]
pub struct PluggableRuntimeImplementation {
    pub bus: Arc<dyn VirtualBus<WasiEnv> + Send + Sync + 'static>,
    pub networking: Arc<dyn VirtualNetworking + Send + Sync + 'static>,
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
        Self {
            #[cfg(not(feature = "host-vnet"))]
            networking: Arc::new(wasmer_vnet::UnsupportedVirtualNetworking::default()),
            #[cfg(feature = "host-vnet")]
            networking: Arc::new(wasmer_wasi_local_networking::LocalNetworking::default()),
            bus: Arc::new(DefaultVirtualBus::default()),
        }
    }
}

#[derive(Debug)]
pub struct DefaultTaskManager {
    /// This is the tokio runtime used for ASYNC operations that is
    /// used for non-javascript environments
    #[cfg(feature = "sys-thread")]
    runtime: std::sync::Arc<Runtime>,
}

impl Default for DefaultTaskManager {
    #[cfg(feature = "sys-thread")]
    fn default() -> Self {
        let runtime: std::sync::Arc<Runtime> =
            std::sync::Arc::new(Builder::new_current_thread().enable_all().build().unwrap());
        Self { runtime }
    }
    #[cfg(not(feature = "sys-thread"))]
    fn default() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(100);
        Self {
            periodic_wakers: Arc::new(Mutex::new((Vec::new(), tx))),
        }
    }
}

#[allow(unused_variables)]
#[cfg(not(feature = "sys-thread"))]
impl VirtualTaskManager for DefaultTaskManager {
    /// Invokes whenever a WASM thread goes idle. In some runtimes (like singlethreaded
    /// execution environments) they will need to do asynchronous work whenever the main
    /// thread goes idle and this is the place to hook for that.
    fn sleep_now(
        &self,
        id: WasiCallingId,
        ms: u128,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + Sync + 'static>> {
        if ms == 0 {
            std::thread::yield_now();
        } else {
            std::thread::sleep(std::time::Duration::from_millis(ms as u64));
        }
        Box::pin(async move {})
    }

    /// Starts an asynchronous task that will run on a shared worker pool
    /// This task must not block the execution or it could cause a deadlock
    fn task_shared(
        &self,
        task: Box<
            dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + 'static,
        >,
    ) -> Result<(), WasiThreadError> {
        Err(WasiThreadError::Unsupported)
    }

    /// Starts an asynchronous task on the local thread (by running it in a runtime)
    fn block_on(&self, task: Pin<Box<dyn Future<Output = ()>>>) {
        unimplemented!("asynchronous operations are not supported on this task manager");
    }

    /// Enters the task runtime
    fn enter(&self) -> Box<dyn std::any::Any> {
        unimplemented!("asynchronous operations are not supported on this task manager");
    }

    /// Starts an asynchronous task will will run on a dedicated thread
    /// pulled from the worker pool that has a stateful thread local variable
    /// It is ok for this task to block execution and any async futures within its scope
    fn task_wasm(
        &self,
        task: Box<dyn FnOnce(Store, Module, Option<VMMemory>) + Send + 'static>,
        store: Store,
        module: Module,
        spawn_type: SpawnType,
    ) -> Result<(), WasiThreadError> {
        Err(WasiThreadError::Unsupported)
    }

    /// Starts an asynchronous task will will run on a dedicated thread
    /// pulled from the worker pool. It is ok for this task to block execution
    /// and any async futures within its scope
    fn task_dedicated(
        &self,
        task: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), WasiThreadError> {
        Err(WasiThreadError::Unsupported)
    }

    /// Starts an asynchronous task will will run on a dedicated thread
    /// pulled from the worker pool. It is ok for this task to block execution
    /// and any async futures within its scope
    fn task_dedicated_async(
        &self,
        task: Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + 'static>> + Send + 'static>,
    ) -> Result<(), WasiThreadError> {
        Err(WasiThreadError::Unsupported)
    }

    /// Returns the amount of parallelism that is possible on this platform
    fn thread_parallelism(&self) -> Result<usize, WasiThreadError> {
        Err(WasiThreadError::Unsupported)
    }
}

#[cfg(feature = "sys-thread")]
impl VirtualTaskManager for DefaultTaskManager {
    /// See [`VirtualTaskManager::sleep_now`].
    fn sleep_now(
        &self,
        _id: WasiCallingId,
        ms: u128,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + Sync + 'static>> {
        Box::pin(async move {
            if ms == 0 {
                tokio::task::yield_now().await;
            } else {
                tokio::time::sleep(std::time::Duration::from_millis(ms as u64)).await;
            }
        })
    }

    /// Starts an asynchronous task will will run on a dedicated thread
    /// pulled from the worker pool that has a stateful thread local variable
    /// It is ok for this task to block execution and any async futures within its scope
    fn task_shared(
        &self,
        task: Box<
            dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + 'static,
        >,
    ) -> Result<(), WasiThreadError> {
        self.runtime.spawn(async move {
            let fut = task();
            fut.await
        });
        Ok(())
    }

    /// See [`VirtualTaskManager::block_on`].
    fn block_on<'a>(&self, task: Pin<Box<dyn Future<Output = ()> + 'a>>) {
        let _guard = self.runtime.enter();
        self.runtime.block_on(async move {
            task.await;
        });
    }

    /// See [`VirtualTaskManager::enter`].
    fn enter<'a>(&'a self) -> Box<dyn std::any::Any + 'a> {
        Box::new(self.runtime.enter())
    }

    /// See [`VirtualTaskManager::enter`].
    fn task_wasm(
        &self,
        task: Box<dyn FnOnce(Store, Module, Option<VMMemory>) + Send + 'static>,
        store: Store,
        module: Module,
        spawn_type: SpawnType,
    ) -> Result<(), WasiThreadError> {
        use wasmer::vm::VMSharedMemory;

        let memory: Option<VMMemory> = match spawn_type {
            SpawnType::CreateWithType(mem) => Some(
                VMSharedMemory::new(&mem.ty, &mem.style)
                    .map_err(|err| {
                        error!("failed to create memory - {}", err);
                    })
                    .unwrap()
                    .into(),
            ),
            SpawnType::NewThread(mem) => Some(mem),
            SpawnType::Create => None,
        };

        std::thread::spawn(move || {
            // Invoke the callback
            task(store, module, memory);
        });
        Ok(())
    }

    /// See [`VirtualTaskManager::task_dedicated`].
    fn task_dedicated(
        &self,
        task: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), WasiThreadError> {
        std::thread::spawn(move || {
            task();
        });
        Ok(())
    }

    /// See [`VirtualTaskManager::task_dedicated_async`].
    fn task_dedicated_async(
        &self,
        task: Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + 'static>> + Send + 'static>,
    ) -> Result<(), WasiThreadError> {
        let runtime = self.runtime.clone();
        std::thread::spawn(move || {
            let fut = task();
            runtime.block_on(fut);
        });
        Ok(())
    }

    /// See [`VirtualTaskManager::thread_parallelism`].
    fn thread_parallelism(&self) -> Result<usize, WasiThreadError> {
        Ok(std::thread::available_parallelism()
            .map(|a| usize::from(a))
            .unwrap_or(8))
    }
}

impl WasiRuntimeImplementation for PluggableRuntimeImplementation {
    fn bus<'a>(&'a self) -> Arc<dyn VirtualBus<WasiEnv> + Send + Sync + 'static> {
        self.bus.clone()
    }

    fn networking<'a>(&'a self) -> Arc<dyn VirtualNetworking + Send + Sync + 'static> {
        self.networking.clone()
    }
}
