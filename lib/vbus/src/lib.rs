use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};
use thiserror::Error;

pub use wasmer_vfs::FileDescriptor;
pub use wasmer_vfs::StdioMode;

pub type Result<T> = std::result::Result<T, BusError>;

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
    fn new_spawn(&self) -> SpawnOptions;

    /// Creates a listener thats used to receive BUS commands
    fn listen(&self) -> Result<Box<dyn VirtualBusListener + Sync>>;
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
    working_dir: String,
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

    pub fn working_dir(&self) -> &str {
        self.working_dir.as_str()
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
                working_dir: "/".to_string(),
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
        self.conf.working_dir = working_dir;
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
    /// Reference to the spawned instance
    pub inst: Box<dyn VirtualBusProcess + Sync>,
}

pub trait VirtualBusScope: fmt::Debug + Send + Sync + 'static {
    //// Returns true if the invokable target has finished
    fn poll_finished(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()>;
}

pub trait VirtualBusInvokable: fmt::Debug + Send + Sync + 'static {
    /// Invokes a service within this instance
    fn invoke(
        &self,
        topic: String,
        format: BusDataFormat,
        buf: &[u8],
    ) -> Result<Box<dyn VirtualBusInvocation + Sync>>;
}

pub trait VirtualBusProcess:
    VirtualBusScope + VirtualBusInvokable + fmt::Debug + Send + Sync + 'static
{
    /// Returns the exit code if the instance has finished
    fn exit_code(&self) -> Option<u32>;

    /// Returns a file descriptor used to read the STDIN
    fn stdin_fd(&self) -> Option<FileDescriptor>;

    /// Returns a file descriptor used to write to STDOUT
    fn stdout_fd(&self) -> Option<FileDescriptor>;

    /// Returns a file descriptor used to write to STDERR
    fn stderr_fd(&self) -> Option<FileDescriptor>;
}

pub trait VirtualBusInvocation:
    VirtualBusScope + VirtualBusInvokable + fmt::Debug + Send + Sync + 'static
{
    /// Polls for new listen events related to this context
    fn poll_event(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<BusInvocationEvent>;
}

#[derive(Debug)]
pub enum BusInvocationEvent {
    /// The server has sent some out-of-band data to you
    Callback {
        /// Topic that this call relates to
        topic: String,
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
}

pub trait VirtualBusListener: fmt::Debug + Send + Sync + 'static {
    /// Polls for new calls to this service
    fn poll_call(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<BusCallEvent>;
}

#[derive(Debug)]
pub struct BusCallEvent {
    /// Topic that this call relates to
    pub topic: String,
    /// Reference to the call itself
    pub called: Box<dyn VirtualBusCalled + Sync>,
    /// Format of the data we received
    pub format: BusDataFormat,
    /// Data passed in the call
    pub data: Vec<u8>,
}

pub trait VirtualBusCalled: VirtualBusListener + fmt::Debug + Send + Sync + 'static {
    /// Sends an out-of-band message back to the caller
    fn callback(&self, topic: String, format: BusDataFormat, buf: &[u8]) -> Result<()>;

    /// Informs the caller that their call has failed
    fn fault(self, fault: BusError) -> Result<()>;

    /// Finishes the call and returns a particular response
    fn reply(self, format: BusDataFormat, buf: &[u8]) -> Result<()>;
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
    fn new_spawn(&self) -> SpawnOptions {
        SpawnOptions::new(Box::new(UnsupportedVirtualBusSpawner::default()))
    }

    fn listen(&self) -> Result<Box<dyn VirtualBusListener + Sync>> {
        Err(BusError::Unsupported)
    }
}

#[derive(Debug, Default)]
pub struct UnsupportedVirtualBusSpawner {}

impl VirtualBusSpawner for UnsupportedVirtualBusSpawner {
    fn spawn(&mut self, _name: &str, _config: &SpawnOptionsConfig) -> Result<BusSpawnedProcess> {
        Err(BusError::Unsupported)
    }
}

#[derive(Error, Copy, Clone, Debug, PartialEq, Eq)]
pub enum BusError {
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
