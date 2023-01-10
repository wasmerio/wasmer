//! WARNING: the API exposed here is unstable and very experimental.  Certain things are not ready
//! yet and may be broken in patch releases.  If you're using this and have any specific needs,
//! please [let us know here](https://github.com/wasmerio/wasmer/issues/583) or by filing an issue.
//!
//! Wasmer always has a virtual root directory located at `/` at which all pre-opened directories can
//! be found.  It's possible to traverse between preopened directories this way as well (for example
//! `preopen-dir1/../preopen-dir2`).
//!
//! A preopened directory is a directory or directory + name combination passed into the
//! `generate_import_object` function.  These are directories that the caller has given
//! the WASI module permission to access.
//!
//! You can implement `VirtualFile` for your own types to get custom behavior and extend WASI, see the
//! [WASI plugin example](https://github.com/wasmerio/wasmer/blob/master/examples/plugin.rs).

#![allow(clippy::cognitive_complexity, clippy::too_many_arguments)]

mod builder;
mod capabilities;
mod env;
mod func_env;
mod types;

use std::{
    cell::RefCell,
    collections::HashMap,
    path::Path,
    sync::{atomic::AtomicU32, Arc, Mutex, MutexGuard, RwLock},
    task::Waker,
    time::Duration,
};

use cooked_waker::{ViaRawPointer, Wake, WakeRef};
use derivative::Derivative;
pub use generational_arena::Index as Inode;
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use wasmer::Store;
use wasmer_vbus::{VirtualBusCalled, VirtualBusInvocation};
use wasmer_vfs::{FileOpener, FileSystem, FsError, OpenOptions, VirtualFile};
use wasmer_wasi_types::wasi::{Cid, Errno, Fd as WasiFd, Rights, Snapshot0Clockid};

pub use self::{
    builder::*,
    capabilities::Capabilities,
    env::{WasiEnv, WasiEnvInner},
    func_env::WasiFunctionEnv,
    types::*,
};
use crate::{
    fs::{fs_error_into_wasi_err, WasiFs, WasiFsRoot, WasiInodes, WasiStateFileGuard},
    os::task::process::WasiProcessId,
    syscalls::types::*,
    utils::WasiParkingLot,
    WasiCallingId, WasiRuntimeImplementation,
};

/// all the rights enabled
pub const ALL_RIGHTS: Rights = Rights::all();

// Implementations of direct to FS calls so that we can easily change their implementation
impl WasiState {
    pub(crate) fn fs_read_dir<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<wasmer_vfs::ReadDir, Errno> {
        self.fs
            .root_fs
            .read_dir(path.as_ref())
            .map_err(fs_error_into_wasi_err)
    }

    pub(crate) fn fs_create_dir<P: AsRef<Path>>(&self, path: P) -> Result<(), Errno> {
        self.fs
            .root_fs
            .create_dir(path.as_ref())
            .map_err(fs_error_into_wasi_err)
    }

    pub(crate) fn fs_remove_dir<P: AsRef<Path>>(&self, path: P) -> Result<(), Errno> {
        self.fs
            .root_fs
            .remove_dir(path.as_ref())
            .map_err(fs_error_into_wasi_err)
    }

    pub(crate) fn fs_rename<P: AsRef<Path>, Q: AsRef<Path>>(
        &self,
        from: P,
        to: Q,
    ) -> Result<(), Errno> {
        self.fs
            .root_fs
            .rename(from.as_ref(), to.as_ref())
            .map_err(fs_error_into_wasi_err)
    }

    pub(crate) fn fs_remove_file<P: AsRef<Path>>(&self, path: P) -> Result<(), Errno> {
        self.fs
            .root_fs
            .remove_file(path.as_ref())
            .map_err(fs_error_into_wasi_err)
    }

    pub(crate) fn fs_new_open_options(&self) -> OpenOptions {
        OpenOptions::new(Box::new(WasiStateOpener {
            root_fs: self.fs.root_fs.clone(),
        }))
    }
}

struct WasiStateOpener {
    root_fs: WasiFsRoot,
}

impl FileOpener for WasiStateOpener {
    fn open(
        &mut self,
        path: &Path,
        conf: &wasmer_vfs::OpenOptionsConfig,
    ) -> wasmer_vfs::Result<Box<dyn VirtualFile + Send + Sync + 'static>> {
        let mut new_options = self.root_fs.new_open_options();
        new_options.options(conf.clone());
        new_options.open(path)
    }
}

// TODO: review allow...
#[allow(dead_code)]
pub(crate) struct WasiThreadContext {
    pub ctx: WasiFunctionEnv,
    pub store: RefCell<Store>,
}

/// The code itself makes safe use of the struct so multiple threads don't access
/// it (without this the JS code prevents the reference to the module from being stored
/// which is needed for the multithreading mode)
unsafe impl Send for WasiThreadContext {}
unsafe impl Sync for WasiThreadContext {}

/// Structures used for the threading and sub-processes
///
/// These internal implementation details are hidden away from the
/// consumer who should instead implement the vbus trait on the runtime
#[derive(Derivative, Default)]
// TODO: review allow...
#[allow(dead_code)]
#[derivative(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub(crate) struct WasiStateThreading {
    #[derivative(Debug = "ignore")]
    pub thread_ctx: HashMap<WasiCallingId, Arc<WasiThreadContext>>,
}

/// Represents a futex which will make threads wait for completion in a more
/// CPU efficient manner
#[derive(Debug, Clone)]
pub struct WasiFutex {
    pub(crate) refcnt: Arc<AtomicU32>,
    pub(crate) inner: Arc<Mutex<tokio::sync::broadcast::Sender<()>>>,
}

#[derive(Debug)]
pub struct WasiBusCall {
    pub bid: WasiProcessId,
    pub invocation: Box<dyn VirtualBusInvocation + Sync>,
}

/// Protected area of the BUS state
#[derive(Debug, Default)]
pub struct WasiBusProtectedState {
    pub call_seed: u64,
    pub called: HashMap<Cid, Box<dyn VirtualBusCalled + Sync + Unpin>>,
    pub calls: HashMap<Cid, WasiBusCall>,
}

/// Structure that holds the state of BUS calls to this process and from
/// this process. BUS calls are the equivalent of RPC's with support
/// for all the major serializers
#[derive(Debug, Default)]
pub struct WasiBusState {
    protected: Mutex<WasiBusProtectedState>,
    poll_waker: WasiParkingLot,
}

impl WasiBusState {
    /// Gets a reference to the waker that can be used for
    /// asynchronous calls
    // TODO: review allow...
    #[allow(dead_code)]
    pub fn get_poll_waker(&self) -> Waker {
        self.poll_waker.get_waker()
    }

    /// Wakes one of the reactors thats currently waiting
    // TODO: review allow...
    #[allow(dead_code)]
    pub fn poll_wake(&self) {
        self.poll_waker.wake()
    }

    /// Will wait until either the reactor is triggered
    /// or the timeout occurs
    // TODO: review allow...
    #[allow(dead_code)]
    pub fn poll_wait(&self, timeout: Duration) -> bool {
        self.poll_waker.wait(timeout)
    }

    /// Locks the protected area of the BUS and returns a guard that
    /// can be used to access it
    pub fn protected<'a>(&'a self) -> MutexGuard<'a, WasiBusProtectedState> {
        self.protected.lock().unwrap()
    }
}

/// Top level data type containing all* the state with which WASI can
/// interact.
///
/// * The contents of files are not stored and may be modified by
/// other, concurrently running programs.  Data such as the contents
/// of directories are lazily loaded.
///
/// Usage:
///
/// ```no_run
/// # use wasmer_wasi::{WasiState, WasiStateCreationError};
/// # fn main() -> Result<(), WasiStateCreationError> {
/// WasiState::new("program_name")
///    .env(b"HOME", "/home/home".to_string())
///    .arg("--help")
///    .envs({
///        let mut hm = std::collections::HashMap::new();
///        hm.insert("COLOR_OUTPUT", "TRUE");
///        hm.insert("PATH", "/usr/bin");
///        hm
///    })
///    .args(&["--verbose", "list"])
///    .preopen(|p| p.directory("src").read(true).write(true).create(true))?
///    .preopen(|p| p.directory(".").alias("dot").read(true))?
///    .build()?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct WasiState {
    pub fs: WasiFs,
    pub secret: [u8; 32],
    pub inodes: Arc<RwLock<WasiInodes>>,
    // TODO: review allow...
    #[allow(dead_code)]
    pub(crate) threading: RwLock<WasiStateThreading>,
    pub(crate) futexs: Mutex<HashMap<u64, WasiFutex>>,
    pub(crate) clock_offset: Mutex<HashMap<Snapshot0Clockid, i64>>,
    pub(crate) bus: WasiBusState,
    pub args: Vec<String>,
    pub envs: Vec<Vec<u8>>,
    pub preopen: Vec<String>,
    pub(crate) runtime: Arc<dyn WasiRuntimeImplementation + Send + Sync>,
}

impl WasiState {
    /// Create a [`WasiStateBuilder`] to construct a validated instance of
    /// [`WasiState`].
    #[allow(clippy::new_ret_no_self)]
    #[deprecated = "Use WasiState::builder()"]
    pub fn new(program_name: impl AsRef<str>) -> WasiStateBuilder {
        WasiState::builder(program_name)
    }

    /// Create a [`WasiStateBuilder`] to construct a validated instance of
    /// [`WasiState`].
    pub fn builder(program_name: impl AsRef<str>) -> WasiStateBuilder {
        create_wasi_state(program_name.as_ref())
    }

    /// Turn the WasiState into bytes
    #[cfg(feature = "enable-serde")]
    pub fn freeze(&self) -> Option<Vec<u8>> {
        bincode::serialize(self).ok()
    }

    /// Get a WasiState from bytes
    #[cfg(feature = "enable-serde")]
    pub fn unfreeze(bytes: &[u8]) -> Option<Self> {
        bincode::deserialize(bytes).ok()
    }

    /// Get the `VirtualFile` object at stdout
    pub fn stdout(&self) -> Result<Option<Box<dyn VirtualFile + Send + Sync + 'static>>, FsError> {
        self.std_dev_get(__WASI_STDOUT_FILENO)
    }

    #[deprecated(
        since = "3.0.0",
        note = "stdout_mut() is no longer needed - just use stdout() instead"
    )]
    pub fn stdout_mut(
        &self,
    ) -> Result<Option<Box<dyn VirtualFile + Send + Sync + 'static>>, FsError> {
        self.stdout()
    }

    /// Get the `VirtualFile` object at stderr
    pub fn stderr(&self) -> Result<Option<Box<dyn VirtualFile + Send + Sync + 'static>>, FsError> {
        self.std_dev_get(__WASI_STDERR_FILENO)
    }

    #[deprecated(
        since = "3.0.0",
        note = "stderr_mut() is no longer needed - just use stderr() instead"
    )]
    pub fn stderr_mut(
        &self,
    ) -> Result<Option<Box<dyn VirtualFile + Send + Sync + 'static>>, FsError> {
        self.stderr()
    }

    /// Get the `VirtualFile` object at stdin
    pub fn stdin(&self) -> Result<Option<Box<dyn VirtualFile + Send + Sync + 'static>>, FsError> {
        self.std_dev_get(__WASI_STDIN_FILENO)
    }

    #[deprecated(
        since = "3.0.0",
        note = "stdin_mut() is no longer needed - just use stdin() instead"
    )]
    pub fn stdin_mut(
        &self,
    ) -> Result<Option<Box<dyn VirtualFile + Send + Sync + 'static>>, FsError> {
        self.stdin()
    }

    /// Internal helper function to get a standard device handle.
    /// Expects one of `__WASI_STDIN_FILENO`, `__WASI_STDOUT_FILENO`, `__WASI_STDERR_FILENO`.
    fn std_dev_get(
        &self,
        fd: WasiFd,
    ) -> Result<Option<Box<dyn VirtualFile + Send + Sync + 'static>>, FsError> {
        let ret = WasiStateFileGuard::new(self, fd)?.map(|a| {
            let ret = Box::new(a);
            let ret: Box<dyn VirtualFile + Send + Sync + 'static> = ret;
            ret
        });
        Ok(ret)
    }

    /// Forking the WasiState is used when either fork or vfork is called
    pub fn fork(&self, inc_refs: bool) -> Self {
        WasiState {
            fs: self.fs.fork(inc_refs),
            secret: self.secret.clone(),
            inodes: self.inodes.clone(),
            threading: Default::default(),
            futexs: Default::default(),
            clock_offset: Mutex::new(self.clock_offset.lock().unwrap().clone()),
            bus: Default::default(),
            args: self.args.clone(),
            envs: self.envs.clone(),
            preopen: self.preopen.clone(),
            runtime: self.runtime.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct WasiDummyWaker;

impl WakeRef for WasiDummyWaker {
    fn wake_by_ref(&self) {}
}

impl Wake for WasiDummyWaker {
    fn wake(self) {}
}

unsafe impl ViaRawPointer for WasiDummyWaker {
    type Target = ();
    fn into_raw(self) -> *mut () {
        std::mem::forget(self);
        std::ptr::null_mut()
    }
    unsafe fn from_raw(_ptr: *mut ()) -> Self {
        WasiDummyWaker
    }
}
