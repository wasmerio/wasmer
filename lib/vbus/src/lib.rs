use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};
use thiserror::Error;

pub use wasmer_vfs::FileDescriptor;
pub use wasmer_vfs::StdioMode;
use wasmer_vfs::VirtualFile;

pub type Result<T> = std::result::Result<T, VirtualBusError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct CallDescriptor(u32);

impl CallDescriptor {
    pub fn raw(&self) -> u32 {
        self.0
    }
}

impl From<u32> for CallDescriptor {
    fn from(a: u32) -> Self {
        Self(a)
    }
}

pub trait VirtualBus: fmt::Debug + Send + Sync + 'static {
    /// Starts a new WAPM sub process
    fn new_spawn(&self) -> SpawnOptions {
        SpawnOptions::new(Box::new(UnsupportedVirtualBusSpawner::default()))
    }

    /// Creates a listener thats used to receive BUS commands
    fn listen<'a>(&'a self) -> Result<&'a dyn VirtualBusListener> {
        Err(VirtualBusError::Unsupported)
    }
}

pub trait VirtualBusSpawner {
    /// Spawns a new WAPM process by its name
    fn spawn(&mut self, name: &str, config: &SpawnOptionsConfig) -> Result<BusSpawnedProcess>;
}

#[derive(Debug, Clone)]
pub struct SpawnOptionsConfig {
    reuse: bool,
    chroot: bool,
    args: Vec<String>,
    preopen: Vec<String>,
    stdin_mode: StdioMode,
    stdout_mode: StdioMode,
    stderr_mode: StdioMode,
    working_dir: Option<String>,
    remote_instance: Option<String>,
    access_token: Option<String>,
}

impl SpawnOptionsConfig {
    pub const fn reuse(&self) -> bool {
        self.reuse
    }

    pub const fn chroot(&self) -> bool {
        self.chroot
    }

    pub const fn args(&self) -> &Vec<String> {
        &self.args
    }

    pub const fn preopen(&self) -> &Vec<String> {
        &self.preopen
    }

    pub const fn stdin_mode(&self) -> StdioMode {
        self.stdin_mode
    }

    pub const fn stdout_mode(&self) -> StdioMode {
        self.stdout_mode
    }

    pub const fn stderr_mode(&self) -> StdioMode {
        self.stderr_mode
    }

    pub fn working_dir(&self) -> Option<&str> {
        self.working_dir.as_ref().map(|a| a.as_str())
    }

    pub fn remote_instance(&self) -> Option<&str> {
        self.remote_instance.as_deref()
    }

    pub fn access_token(&self) -> Option<&str> {
        self.access_token.as_deref()
    }
}

pub struct SpawnOptions {
    spawner: Box<dyn VirtualBusSpawner>,
    conf: SpawnOptionsConfig,
}

impl SpawnOptions {
    pub fn new(spawner: Box<dyn VirtualBusSpawner>) -> Self {
        Self {
            spawner,
            conf: SpawnOptionsConfig {
                reuse: false,
                chroot: false,
                args: Vec::new(),
                preopen: Vec::new(),
                stdin_mode: StdioMode::Null,
                stdout_mode: StdioMode::Null,
                stderr_mode: StdioMode::Null,
                working_dir: None,
                remote_instance: None,
                access_token: None,
            },
        }
    }
    pub fn options(&mut self, options: SpawnOptionsConfig) -> &mut Self {
        self.conf = options;
        self
    }

    pub fn reuse(&mut self, reuse: bool) -> &mut Self {
        self.conf.reuse = reuse;
        self
    }

    pub fn chroot(&mut self, chroot: bool) -> &mut Self {
        self.conf.chroot = chroot;
        self
    }

    pub fn args(&mut self, args: Vec<String>) -> &mut Self {
        self.conf.args = args;
        self
    }

    pub fn preopen(&mut self, preopen: Vec<String>) -> &mut Self {
        self.conf.preopen = preopen;
        self
    }

    pub fn stdin_mode(&mut self, stdin_mode: StdioMode) -> &mut Self {
        self.conf.stdin_mode = stdin_mode;
        self
    }

    pub fn stdout_mode(&mut self, stdout_mode: StdioMode) -> &mut Self {
        self.conf.stdout_mode = stdout_mode;
        self
    }

    pub fn stderr_mode(&mut self, stderr_mode: StdioMode) -> &mut Self {
        self.conf.stderr_mode = stderr_mode;
        self
    }

    pub fn working_dir(&mut self, working_dir: String) -> &mut Self {
        self.conf.working_dir = Some(working_dir);
        self
    }

    pub fn remote_instance(&mut self, remote_instance: String) -> &mut Self {
        self.conf.remote_instance = Some(remote_instance);
        self
    }

    pub fn access_token(&mut self, access_token: String) -> &mut Self {
        self.conf.access_token = Some(access_token);
        self
    }

    /// Spawns a new bus instance by its reference name
    pub fn spawn(&mut self, name: &str) -> Result<BusSpawnedProcess> {
        self.spawner.spawn(name, &self.conf)
    }
}

#[derive(Debug)]
pub struct BusSpawnedProcess {
    /// Name of the spawned process
    pub name: String,
    /// Configuration applied to this spawned thread
    pub config: SpawnOptionsConfig,
    /// Reference to the spawned instance
    pub inst: Box<dyn VirtualBusProcess + Sync + Unpin>,
    /// Virtual file used for stdin
    pub stdin: Option<Box<dyn VirtualFile + Send + Sync + 'static>>,
    /// Virtual file used for stdout
    pub stdout: Option<Box<dyn VirtualFile + Send + Sync + 'static>>,
    /// Virtual file used for stderr
    pub stderr: Option<Box<dyn VirtualFile + Send + Sync + 'static>>,
}

pub trait VirtualBusScope: fmt::Debug + Send + Sync + 'static {
    //// Returns true if the invokable target has finished
    fn poll_finished(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()>;
}

pub trait VirtualBusInvokable: fmt::Debug + Send + Sync + 'static {
    /// Invokes a service within this instance
    fn invoke(
        &self,
        topic_hash: u128,
        format: BusDataFormat,
        buf: Vec<u8>,
    ) -> Box<dyn VirtualBusInvoked>;
}

pub trait VirtualBusInvoked: fmt::Debug + Unpin + 'static {
    //// Returns once the bus has been invoked (or failed)
    fn poll_invoked(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<Box<dyn VirtualBusInvocation + Sync>>>;
}

pub trait VirtualBusProcess:
    VirtualBusScope + VirtualBusInvokable + fmt::Debug + Send + Sync + 'static
{
    /// Returns the exit code if the instance has finished
    fn exit_code(&self) -> Option<u32>;

    /// Polls to check if the process is ready yet to receive commands
    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()>;
}

pub trait VirtualBusInvocation:
    VirtualBusInvokable + fmt::Debug + Send + Sync + Unpin + 'static
{
    /// Polls for new listen events related to this context
    fn poll_event(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<BusInvocationEvent>;
}

#[derive(Debug)]
pub struct InstantInvocation
{
    val: Option<BusInvocationEvent>,
    err: Option<VirtualBusError>,
    call: Option<Box<dyn VirtualBusInvocation + Sync>>,
}

impl InstantInvocation
{
    pub fn response(format: BusDataFormat, data: Vec<u8>) -> Self {
        Self {
            val: Some(BusInvocationEvent::Response { format, data }),
            err: None,
            call: None
        }
    }

    pub fn fault(err: VirtualBusError) -> Self {
        Self {
            val: None,
            err: Some(err),
            call: None
        }
    }

    pub fn call(val: Box<dyn VirtualBusInvocation + Sync>) -> Self {
        Self {
            val: None,
            err: None,
            call: Some(val)
        }
    }
}

impl VirtualBusInvoked
for InstantInvocation
{
    fn poll_invoked(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<Box<dyn VirtualBusInvocation + Sync>>> {
        if let Some(err) = self.err.take() {
            return Poll::Ready(Err(err));
        }
        if let Some(val) = self.val.take() {
            return Poll::Ready(Ok(Box::new(InstantInvocation {
                val: Some(val),
                err: None,
                call: None,
            })));
        }
        match self.call.take() {
            Some(val) => {
                Poll::Ready(Ok(val))
            },
            None => {
                Poll::Ready(Err(VirtualBusError::AlreadyConsumed))
            }
        }
    }
}

impl VirtualBusInvocation
for InstantInvocation
{
    fn poll_event(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<BusInvocationEvent> {
        match self.val.take() {
            Some(val) => {
                Poll::Ready(val)
            },
            None => {
                Poll::Ready(BusInvocationEvent::Fault { fault: VirtualBusError::AlreadyConsumed })
            }
        }
    }
}

impl VirtualBusInvokable
for InstantInvocation
{
    fn invoke(
        &self,
        _topic_hash: u128,
        _format: BusDataFormat,
        _buf: Vec<u8>,
    ) -> Box<dyn VirtualBusInvoked> {
        Box::new(
            InstantInvocation {
                val: None,
                err: Some(VirtualBusError::InvalidTopic),
                call: None
            }
        )
    }
}

#[derive(Debug)]
pub enum BusInvocationEvent {
    /// The server has sent some out-of-band data to you
    Callback {
        /// Topic that this call relates to
        topic_hash: u128,
        /// Format of the data we received
        format: BusDataFormat,
        /// Data passed in the call
        data: Vec<u8>,
    },
    /// The service has a responded to your call
    Response {
        /// Format of the data we received
        format: BusDataFormat,
        /// Data returned by the call
        data: Vec<u8>,
    },
    /// The service has responded with a fault
    Fault {
        /// Fault code that was raised
        fault: VirtualBusError
    }
}

pub trait VirtualBusListener: fmt::Debug + Send + Sync + Unpin + 'static {
    /// Polls for new calls to this service
    fn poll(self: Pin<&Self>, cx: &mut Context<'_>) -> Poll<BusCallEvent>;
}

#[derive(Debug)]
pub struct BusCallEvent {
    /// Topic hash that this call relates to
    pub topic_hash: u128,
    /// Reference to the call itself
    pub called: Box<dyn VirtualBusCalled + Sync + Unpin>,
    /// Format of the data we received
    pub format: BusDataFormat,
    /// Data passed in the call
    pub data: Vec<u8>,
}

pub trait VirtualBusCalled: fmt::Debug + Send + Sync + 'static
{
    /// Polls for new calls to this service
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<BusCallEvent>;

    /// Sends an out-of-band message back to the caller
    fn callback(&self, topic_hash: u128, format: BusDataFormat, buf: Vec<u8>);

    /// Informs the caller that their call has failed
    fn fault(self: Box<Self>, fault: VirtualBusError);

    /// Finishes the call and returns a particular response
    fn reply(&self, format: BusDataFormat, buf: Vec<u8>);
}

/// Format that the supplied data is in
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum BusDataFormat {
    Raw,
    Bincode,
    MessagePack,
    Json,
    Yaml,
    Xml,
}

#[derive(Debug, Default)]
pub struct UnsupportedVirtualBus {}

impl VirtualBus for UnsupportedVirtualBus {
}

#[derive(Debug, Default)]
pub struct UnsupportedVirtualBusSpawner {}

impl VirtualBusSpawner for UnsupportedVirtualBusSpawner {
    fn spawn(&mut self, _name: &str, _config: &SpawnOptionsConfig) -> Result<BusSpawnedProcess> {
        Err(VirtualBusError::Unsupported)
    }
}

#[derive(Error, Copy, Clone, Debug, PartialEq, Eq)]
pub enum VirtualBusError {
    /// Failed during serialization
    #[error("serialization failed")]
    Serialization,
    /// Failed during deserialization
    #[error("deserialization failed")]
    Deserialization,
    /// Invalid WAPM process
    #[error("invalid wapm")]
    InvalidWapm,
    /// Failed to fetch the WAPM process
    #[error("fetch failed")]
    FetchFailed,
    /// Failed to compile the WAPM process
    #[error("compile error")]
    CompileError,
    /// Invalid ABI
    #[error("WAPM process has an invalid ABI")]
    InvalidABI,
    /// Call was aborted
    #[error("call aborted")]
    Aborted,
    /// Bad handle
    #[error("bad handle")]
    BadHandle,
    /// Invalid topic
    #[error("invalid topic")]
    InvalidTopic,
    /// Invalid callback
    #[error("invalid callback")]
    BadCallback,
    /// Call is unsupported
    #[error("unsupported")]
    Unsupported,
    /// Bad request
    #[error("bad request")]
    BadRequest,
    /// Access denied
    #[error("access denied")]
    AccessDenied,
    /// Internal error has occured
    #[error("internal error")]
    InternalError,
    /// Memory allocation failed
    #[error("memory allocation failed")]
    MemoryAllocationFailed,
    /// Invocation has failed
    #[error("invocation has failed")]
    InvokeFailed,
    /// Already consumed
    #[error("already consumed")]
    AlreadyConsumed,
    /// Memory access violation
    #[error("memory access violation")]
    MemoryAccessViolation,
    /// Some other unhandled error. If you see this, it's probably a bug.
    #[error("unknown error found")]
    UnknownError,
}
