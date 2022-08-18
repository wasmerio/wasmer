use std::io::Write;
use std::sync::{Arc, Mutex};
use std::task::Waker;
use std::{fmt, io};
use std::future::Future;
use std::pin::Pin;
use thiserror::Error;
use wasmer::{Module, Store, MemoryType};
use wasmer::vm::VMMemory;
#[cfg(feature = "sys")]
use wasmer_types::MemoryStyle;
use wasmer_vbus::{DefaultVirtualBus, VirtualBus};
use wasmer_vnet::VirtualNetworking;
use derivative::Derivative;
use tracing::*;

use crate::{WasiCallingId, WasiEnv};

use super::types::*;

mod ws;
pub use ws::*;

mod stdio;
pub use stdio::*;

#[cfg(feature = "termios")]
pub mod term;
#[cfg(feature = "termios")]
pub use term::*;

use tokio::runtime::{
    Builder, Runtime
};

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

impl From<WasiThreadError> for __wasi_errno_t {
    fn from(a: WasiThreadError) -> __wasi_errno_t {
        match a {
            WasiThreadError::Unsupported => __WASI_ENOTSUP,
            WasiThreadError::MethodNotFound => __WASI_EINVAL,
            WasiThreadError::MemoryCreateFailed => __WASI_EFAULT,
            WasiThreadError::InvalidWasmContext => __WASI_ENOEXEC,
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
    pub line_feeds: bool,
}

impl Default
for WasiTtyState {
    fn default() -> Self {
        Self {
            rows: 80,
            cols: 25,
            width: 800,
            height: 600,
            stdin_tty: true,
            stdout_tty: true,
            stderr_tty: true,
            echo: false,
            line_buffered: false,
            line_feeds: true,
        }
    }
}

#[derive(Debug)]
pub struct SpawnedMemory
{
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

#[derive(Debug, Default)]
pub struct ReqwestOptions {
    pub gzip: bool,
    pub cors_proxy: Option<String>,
}

pub struct ReqwestResponse {
    pub pos: usize,
    pub data: Option<Vec<u8>>,
    pub ok: bool,
    pub redirected: bool,
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
}

/// An implementation of task management
#[allow(unused_variables)]
pub trait VirtualTaskManager: fmt::Debug + Send + Sync + 'static
{
    /// Invokes whenever a WASM thread goes idle. In some runtimes (like singlethreaded
    /// execution environments) they will need to do asynchronous work whenever the main
    /// thread goes idle and this is the place to hook for that.
    fn sleep_now(&self, _id: WasiCallingId, ms: u128) -> Pin<Box<dyn Future<Output=()> + Send + Sync + 'static>>;

    /// Starts an asynchronous task that will run on a shared worker pool
    /// This task must not block the execution or it could cause a deadlock
    fn task_shared(
        &self,
        task: Box<
            dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + 'static,
        >,
    ) -> Result<(), WasiThreadError>;

    /// Starts an asynchronous task on the local thread (by running it in a runtime)
    fn block_on(
        &self,
        task: Pin<Box<dyn Future<Output = ()>>>,
    );

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

    /// Returns a list of periodic wakers that need to be woken on a regular basis
    fn periodic_wakers(&self) -> Arc<Mutex<(Vec<Waker>, tokio::sync::broadcast::Sender<()>)>>;

    /// Gets a function that will register a root periodic waker
    fn register_root_waker(&self) -> Arc<dyn Fn(Waker) + Send + Sync + 'static> {
        let periodic_wakers = self.periodic_wakers();
        Arc::new(move |waker: Waker| {
            let mut guard = periodic_wakers.lock().unwrap();
            guard.0.push(waker);
            let _ = guard.1.send(());
        })
    }

    /// Wakes all the root wakers
    fn wake_root_wakers(&self) {
        let wakers = {
            let periodic_wakers = self.periodic_wakers();
            let mut guard = periodic_wakers.lock().unwrap();
            guard.0.drain(..).collect::<Vec<_>>()
        };
        for waker in wakers {
            waker.wake();
        }
    }

    /// Waits for a periodic period (if there is anyone waiting on it)
    fn wait_for_root_waker(&self) -> Pin<Box<dyn Future<Output=()> + Send + Sync + 'static>> {
        let (has_wakers, mut new_wakers) = {
            let periodic_wakers = self.periodic_wakers();
            let guard = periodic_wakers.lock().unwrap();
            let has_wakers = guard.0.is_empty() == false;
            let new_wakers = guard.1.subscribe();
            (has_wakers, new_wakers)
        };
        let sleep_now = self.sleep_now(crate::current_caller_id(), 5);
        Box::pin(async move {
            if has_wakers {
                tokio::select! {
                    _ = sleep_now => { },
                    _ = new_wakers.recv() => { },
                }
            } else {
                let _ = new_wakers.recv().await;
            }
        })
    }
}

/// Represents an implementation of the WASI runtime - by default everything is
/// unimplemented.
#[allow(unused_variables)]
pub trait WasiRuntimeImplementation
where Self: fmt::Debug + Sync,
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
    fn tty_set(&self, _tty_state: WasiTtyState) {
    }

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
        options: ReqwestOptions,
        headers: Vec<(String, String)>,
        data: Option<Vec<u8>>,
    ) -> Result<ReqwestResponse, u32> {
        Err(__WASI_ENOTSUP as u32)
    }

    /// Performs a HTTP or HTTPS request to a destination URL
    #[cfg(feature = "host-reqwest")]
    fn reqwest(
        &self,
        tasks: &dyn VirtualTaskManager,
        url: &str,
        method: &str,
        _options: ReqwestOptions,
        headers: Vec<(String, String)>,
        data: Option<Vec<u8>>,
    ) -> Result<ReqwestResponse, u32> {
        use std::convert::TryFrom;

        let work = {
            let url = url.to_string();
            let method = method.to_string();
            async move {
                let method = reqwest::Method::try_from(method.as_str()).map_err(|err| {
                    debug!("failed to convert method ({}) - {}", method, err);
                    __WASI_EIO as u32
                })?;
        
                let client = reqwest::ClientBuilder::default().build().map_err(|err| {
                    debug!("failed to build reqwest client - {}", err);
                    __WASI_EIO as u32
                })?;
        
                let mut builder = client.request(method, url.as_str());
                for (header, val) in headers {
                    if let Ok(header) =
                        reqwest::header::HeaderName::from_bytes(header.as_bytes())
                    {
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
                    __WASI_EIO as u32
                })?;
        
                let response = client.execute(request)
                    .await
                    .map_err(|err|
                {
                    debug!("failed to execute reqest - {}", err);
                    __WASI_EIO as u32
                })?;
        
                let status = response.status().as_u16();
                let status_text = response.status().as_str().to_string();
                let data = response.bytes().await.map_err(|err| {
                    debug!("failed to read response bytes - {}", err);
                    __WASI_EIO as u32
                })?;
                let data = data.to_vec();
        
                Ok(ReqwestResponse {
                    pos: 0usize,
                    ok: true,
                    status,
                    status_text,
                    redirected: false,
                    data: Some(data),
                    headers: Vec::new(),
                })
            }
        };

        let (tx, rx) = std::sync::mpsc::channel();
        tasks
            .block_on(Box::pin(async move {
                let ret = work.await;
                let _ = tx.send(ret);
            }));
        rx.try_recv().map_err(|_| __WASI_EIO)?
    }

    /// Make a web socket connection to a particular URL
    #[cfg(feature = "os")]
    #[cfg(not(feature = "host-ws"))]
    fn web_socket(&self, url: &str) -> Result<Box<dyn WebSocketAbi>, String> {
        Err("not supported".to_string())
    }

    /// Make a web socket connection to a particular URL
    #[cfg(feature = "os")]
    #[cfg(feature = "host-ws")]
    fn web_socket(&self, url: &str) -> Result<Box<dyn WebSocketAbi>, String> {
        let url = url.to_string();
        let (tx_done, rx_done) = mpsc::unbounded_channel();
        self.task_shared(Box::new(move ||
            Box::pin(async move {
                let ret = move || async move {
                    Box::new(TerminalWebSocket::new(url.as_str())).await
                };
                let ret = ret().await;
                let _ = tx_done.send(ret);
            })
        ));
        tokio::task::block_in_place(move || {
            rx_done.blocking_recv()
                .ok_or("failed to create web socket".to_string())
        })
    }

    /// Writes output to the console
    fn stdout(&self, data: &[u8]) -> io::Result<()> {
        let mut handle = io::stdout();
        handle.write_all(data)
    }

    /// Writes output to the console
    fn stderr(&self, data: &[u8]) -> io::Result<()> {
        let mut handle = io::stderr();
        handle.write_all(data)
    }

    /// Flushes the output to the console
    fn flush(&self) -> io::Result<()> {
        io::stdout().flush()?;
        io::stderr().flush()?;
        Ok(())
    }

    /// Writes output to the log
    #[cfg(feature = "tracing")]
    fn log(&self, text: String) -> io::Result<()> {
        tracing::info!("{}", text);
        Ok(())
    }

    /// Writes output to the log
    #[cfg(not(feature = "tracing"))]
    fn log(&self, text: String) -> io::Result<()> {
        let text = format!("{}\r\n", text);
        self.stderr(text.as_bytes())
    }

    /// Clears the terminal
    fn cls(&self) -> io::Result<()> {
        self.stdout("\x1B[H\x1B[2J".as_bytes())
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct PluggableRuntimeImplementation
{
    pub bus: Arc<dyn VirtualBus<WasiEnv> + Send + Sync + 'static>,
    pub networking: Arc<dyn VirtualNetworking + Send + Sync + 'static>,
}

impl PluggableRuntimeImplementation
{
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

impl Default
for PluggableRuntimeImplementation
{
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
    runtime: std::sync::Arc<Runtime>,
    /// List of periodic wakers to wake (this is used by IO subsystems)
    /// that do not support async operations
    periodic_wakers: Arc<Mutex<(Vec<Waker>, tokio::sync::broadcast::Sender<()>)>>
}

impl Default
for DefaultTaskManager {
    fn default() -> Self {
        let runtime: std::sync::Arc<Runtime>
            = std::sync::Arc::new(Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
            );
        let (tx, _) = tokio::sync::broadcast::channel(100);
        Self {
            runtime,
            periodic_wakers: Arc::new(Mutex::new((Vec::new(), tx)))
        }
    }
}

#[allow(unused_variables)]
#[cfg(not(feature = "sys-thread"))]
impl VirtualTaskManager
for DefaultTaskManager
{
    /// Invokes whenever a WASM thread goes idle. In some runtimes (like singlethreaded
    /// execution environments) they will need to do asynchronous work whenever the main
    /// thread goes idle and this is the place to hook for that.
    fn sleep_now(&self, id: WasiCallingId, ms: u128) -> Pin<Box<dyn Future<Output=()> + Send + Sync + 'static>> {
        if ms == 0 {
            std::thread::yield_now();
        } else {
            std::thread::sleep(std::time::Duration::from_millis(ms as u64));
        }
        Box::pin(async move {
        })
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
    fn block_on(
        &self,
        task: Pin<Box<dyn Future<Output = ()>>>,
    )
    {
        let _guard = self.runtime.enter();
        self.runtime.block_on(async move {
            task.await;
        });
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

    /// Returns a reference to the periodic wakers used by this task manager
    fn periodic_wakers(&self) -> Arc<Mutex<(Vec<Waker>, tokio::sync::broadcast::Sender<()>)>> {
        self.periodic_wakers.clone()
    }
}

#[cfg(feature = "sys-thread")]
impl VirtualTaskManager
for DefaultTaskManager
{
    /// Invokes whenever a WASM thread goes idle. In some runtimes (like singlethreaded
    /// execution environments) they will need to do asynchronous work whenever the main
    /// thread goes idle and this is the place to hook for that.
    fn sleep_now(&self, _id: WasiCallingId, ms: u128) -> Pin<Box<dyn Future<Output=()> + Send + Sync + 'static>> {
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

    /// Starts an asynchronous task on the local thread (by running it in a runtime)
    fn block_on(
        &self,
        task: Pin<Box<dyn Future<Output = ()>>>,
    )
    {
        let _guard = self.runtime.enter();
        self.runtime.block_on(async move {
            task.await;
        });
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
        use wasmer::vm::VMSharedMemory;

        let memory: Option<VMMemory> = match spawn_type {
            SpawnType::CreateWithType(mem) => {
                Some(
                    VMSharedMemory::new(&mem.ty, &mem.style)
                        .map_err(|err| {
                            error!("failed to create memory - {}", err);
                        })
                        .unwrap()
                        .into()
                )
            },
            SpawnType::NewThread(mem) => Some(mem),
            SpawnType::Create => None,
        };
        
        std::thread::spawn(move || {
            // Invoke the callback
            task(store, module, memory);
        });
        Ok(())
    }

    /// Starts an asynchronous task will will run on a dedicated thread
    /// pulled from the worker pool. It is ok for this task to block execution
    /// and any async futures within its scope
    fn task_dedicated(
        &self,
        task: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), WasiThreadError> {
        std::thread::spawn(move || {
            task();
        });
        Ok(())
    }

    /// Starts an asynchronous task will will run on a dedicated thread
    /// pulled from the worker pool. It is ok for this task to block execution
    /// and any async futures within its scope
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

    /// Number of concurrent threads supported on this machine
    /// in a stable way (ideally we should aim for this number
    /// of background threads)
    fn thread_parallelism(&self) -> Result<usize, WasiThreadError> {
        Ok(
            std::thread::available_parallelism()
                .map(|a| usize::from(a))
                .unwrap_or(8)
        )
    }

    /// Returns a reference to the periodic wakers used by this task manager
    fn periodic_wakers(&self) -> Arc<Mutex<(Vec<Waker>, tokio::sync::broadcast::Sender<()>)>> {
        self.periodic_wakers.clone()
    }
}

impl WasiRuntimeImplementation
for PluggableRuntimeImplementation
{
    fn bus<'a>(&'a self) -> Arc<dyn VirtualBus<WasiEnv> + Send + Sync + 'static> {
        self.bus.clone()
    }

    fn networking<'a>(&'a self) -> Arc<dyn VirtualNetworking + Send + Sync + 'static> {
        self.networking.clone()
    }
}
