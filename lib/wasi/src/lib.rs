#![deny(unused_mut)]
#![doc(html_favicon_url = "https://wasmer.io/images/icons/favicon-32x32.png")]
#![doc(html_logo_url = "https://github.com/wasmerio.png?size=200")]

//! Wasmer's WASI implementation
//!
//! Use `generate_import_object` to create an [`Imports`].  This [`Imports`]
//! can be combined with a module to create an `Instance` which can execute WASI
//! Wasm functions.
//!
//! See `state` for the experimental WASI FS API.  Also see the
//! [WASI plugin example](https://github.com/wasmerio/wasmer/blob/master/examples/plugin.rs)
//! for an example of how to extend WASI using the WASI FS API.

#[cfg(all(not(feature = "sys"), not(feature = "js")))]
compile_error!("At least the `sys` or the `js` feature must be enabled. Please, pick one.");

#[cfg(all(feature = "sys", feature = "js"))]
compile_error!(
    "Cannot have both `sys` and `js` features enabled at the same time. Please, pick one."
);

#[cfg(all(feature = "sys", target_arch = "wasm32"))]
compile_error!("The `sys` feature must be enabled only for non-`wasm32` target.");

#[cfg(all(feature = "js", not(target_arch = "wasm32")))]
compile_error!(
    "The `js` feature must be enabled only for the `wasm32` target (either `wasm32-unknown-unknown` or `wasm32-wasi`)."
);

#[cfg(all(feature = "host-fs", feature = "mem-fs"))]
compile_error!(
    "Cannot have both `host-fs` and `mem-fs` features enabled at the same time. Please, pick one."
);

#[macro_use]
mod macros;
mod runtime;
mod state;
mod syscalls;
mod utils;

/// Runners for WASI / Emscripten
#[cfg(feature = "webc_runner")]
pub mod runners;

use crate::syscalls::*;

pub use crate::state::{
    Fd, Pipe, Stderr, Stdin, Stdout, WasiFs, WasiInodes, WasiState, WasiStateBuilder,
    WasiStateCreationError, ALL_RIGHTS, VIRTUAL_ROOT_FD,
};
pub use crate::syscalls::types;
#[cfg(feature = "wasix")]
pub use crate::utils::is_wasix_module;
pub use crate::utils::wasi_import_shared_memory;
pub use crate::utils::{get_wasi_version, get_wasi_versions, is_wasi_module, WasiVersion};

pub use wasmer_vbus::{UnsupportedVirtualBus, VirtualBus};
#[deprecated(since = "2.1.0", note = "Please use `wasmer_vfs::FsError`")]
pub use wasmer_vfs::FsError as WasiFsError;
#[deprecated(since = "2.1.0", note = "Please use `wasmer_vfs::VirtualFile`")]
pub use wasmer_vfs::VirtualFile as WasiFile;
pub use wasmer_vfs::{FsError, VirtualFile};
pub use wasmer_vnet::{UnsupportedVirtualNetworking, VirtualNetworking};

use derivative::*;
use std::ops::Deref;
use thiserror::Error;
use tracing::trace;
use wasmer::{
    imports, namespace, AsStoreMut, AsStoreRef, ExportError, Exports, Function, FunctionEnv,
    Imports, Instance, Memory, Memory32, MemoryAccessError, MemorySize, MemoryView, Module,
    TypedFunction,
};
use wasmer_wasi_types::wasi::{BusErrno, Errno, Snapshot0Clockid};

pub use runtime::{
    PluggableRuntimeImplementation, WasiRuntimeImplementation, WasiThreadError, WasiTtyState,
};
use std::sync::{mpsc, Arc, Mutex, RwLockReadGuard, RwLockWriteGuard};
use std::time::Duration;

/// This is returned in `RuntimeError`.
/// Use `downcast` or `downcast_ref` to retrieve the `ExitCode`.
#[derive(Error, Debug)]
pub enum WasiError {
    #[error("WASI exited with code: {0}")]
    Exit(syscalls::types::__wasi_exitcode_t),
    #[error("The WASI version could not be determined")]
    UnknownWasiVersion,
}

/// Represents the ID of a WASI thread
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WasiThreadId(u32);

impl From<u32> for WasiThreadId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}
impl From<WasiThreadId> for u32 {
    fn from(t: WasiThreadId) -> u32 {
        t.0 as u32
    }
}

/// Represents the ID of a sub-process
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WasiBusProcessId(u32);

impl From<u32> for WasiBusProcessId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}
impl From<WasiBusProcessId> for u32 {
    fn from(id: WasiBusProcessId) -> u32 {
        id.0 as u32
    }
}

#[derive(Debug, Clone)]
pub struct WasiThread {
    /// ID of this thread
    #[allow(dead_code)]
    id: WasiThreadId,
    /// Signalers used to tell joiners that the thread has exited
    exit: Arc<Mutex<Option<mpsc::Sender<()>>>>,
    /// Event to wait on for the thread to join
    join: Arc<Mutex<mpsc::Receiver<()>>>,
}

impl WasiThread {
    /// Waits for the thread to exit (false = timeout)
    pub fn join(&self, timeout: Duration) -> bool {
        let guard = self.join.lock().unwrap();
        let timeout = guard.recv_timeout(timeout);
        match timeout {
            Ok(_) => true,
            Err(mpsc::RecvTimeoutError::Disconnected) => true,
            Err(mpsc::RecvTimeoutError::Timeout) => false,
        }
    }
}

pub struct WasiFunctionEnv {
    pub env: FunctionEnv<WasiEnv>,
}

impl WasiFunctionEnv {
    pub fn new(store: &mut impl AsStoreMut, env: WasiEnv) -> Self {
        Self {
            env: FunctionEnv::new(store, env),
        }
    }

    /// Get an `Imports` for a specific version of WASI detected in the module.
    pub fn import_object(
        &self,
        store: &mut impl AsStoreMut,
        module: &Module,
    ) -> Result<Imports, WasiError> {
        let wasi_version = get_wasi_version(module, false).ok_or(WasiError::UnknownWasiVersion)?;
        Ok(generate_import_object_from_env(
            store,
            &self.env,
            wasi_version,
        ))
    }

    pub fn data_mut<'a>(&'a self, store: &'a mut impl AsStoreMut) -> &'a mut WasiEnv {
        self.env.as_mut(store)
    }

    /// Initializes the WasiEnv using the instance exports
    /// (this must be executed before attempting to use it)
    /// (as the stores can not by themselves be passed between threads we can store the module
    ///  in a thread-local variables and use it later - for multithreading)
    pub fn initialize(
        &mut self,
        store: &mut impl AsStoreMut,
        instance: &Instance,
    ) -> Result<(), ExportError> {
        // List all the exports and imports
        for ns in instance.module().exports() {
            //trace!("module::export - {} ({:?})", ns.name(), ns.ty());
            trace!("module::export - {}", ns.name());
        }
        for ns in instance.module().imports() {
            trace!("module::import - {}::{}", ns.module(), ns.name());
        }

        // First we get the malloc function which if it exists will be used to
        // create the pthread_self structure
        let memory = instance.exports.get_memory("memory")?.clone();
        let env = self.data_mut(store);
        env.set_memory(memory);

        Ok(())
    }

    /// Like `import_object` but containing all the WASI versions detected in
    /// the module.
    pub fn import_object_for_all_wasi_versions(
        &self,
        store: &mut impl AsStoreMut,
        module: &Module,
    ) -> Result<Imports, WasiError> {
        let wasi_versions =
            get_wasi_versions(module, false).ok_or(WasiError::UnknownWasiVersion)?;

        let mut resolver = Imports::new();
        for version in wasi_versions.iter() {
            let new_import_object = generate_import_object_from_env(store, &self.env, *version);
            for ((n, m), e) in new_import_object.into_iter() {
                resolver.define(&n, &m, e);
            }
        }

        #[cfg(feature = "wasix")]
        if is_wasix_module(module) {
            self.data_mut(store)
                .state
                .fs
                .is_wasix
                .store(true, std::sync::atomic::Ordering::Release);
        }

        Ok(resolver)
    }
}

/// The environment provided to the WASI imports.
#[derive(Derivative, Clone)]
#[derivative(Debug)]
#[allow(dead_code)]
pub struct WasiEnv {
    /// ID of this thread (zero is the main thread)
    id: WasiThreadId,
    /// Represents a reference to the memory
    memory: Option<Memory>,
    /// If the module has it then map the thread start
    #[derivative(Debug = "ignore")]
    thread_start: Option<TypedFunction<u64, ()>>,
    #[derivative(Debug = "ignore")]
    reactor_work: Option<TypedFunction<u64, ()>>,
    #[derivative(Debug = "ignore")]
    reactor_finish: Option<TypedFunction<u64, ()>>,
    #[derivative(Debug = "ignore")]
    malloc: Option<TypedFunction<u64, u64>>,
    #[derivative(Debug = "ignore")]
    free: Option<TypedFunction<(u64, u64), ()>>,
    /// Shared state of the WASI system. Manages all the data that the
    /// executing WASI program can see.
    pub state: Arc<WasiState>,
    /// Implementation of the WASI runtime.
    pub(crate) runtime: Arc<dyn WasiRuntimeImplementation + Send + Sync + 'static>,
}

impl WasiEnv {
    /// Create a new WasiEnv from a WasiState (memory will be set to None)
    pub fn new(state: WasiState) -> Self {
        Self {
            id: 0u32.into(),
            state: Arc::new(state),
            memory: None,
            thread_start: None,
            reactor_work: None,
            reactor_finish: None,
            malloc: None,
            free: None,
            runtime: Arc::new(PluggableRuntimeImplementation::default()),
        }
    }

    /// Returns a copy of the current runtime implementation for this environment
    pub fn runtime(&self) -> &(dyn WasiRuntimeImplementation) {
        self.runtime.deref()
    }

    /// Overrides the runtime implementation for this environment
    pub fn set_runtime<R>(&mut self, runtime: R)
    where
        R: WasiRuntimeImplementation + Send + Sync + 'static,
    {
        self.runtime = Arc::new(runtime);
    }

    /// Returns the current thread ID
    pub fn current_thread_id(&self) -> WasiThreadId {
        self.id
    }

    /// Creates a new thread only this wasi environment
    pub fn new_thread(&self) -> WasiThread {
        let (tx, rx) = mpsc::channel();

        let mut guard = self.state.threading.lock().unwrap();

        guard.thread_seed += 1;
        let next_id: WasiThreadId = guard.thread_seed.into();

        let thread = WasiThread {
            id: next_id,
            exit: Arc::new(Mutex::new(Some(tx))),
            join: Arc::new(Mutex::new(rx)),
        };

        guard.threads.insert(thread.id, thread.clone());
        thread
    }

    /// Copy the lazy reference so that when it's initialized during the
    /// export phase, all the other references get a copy of it
    pub fn memory_clone(&self) -> Option<Memory> {
        self.memory.clone()
    }

    // Yields execution
    pub fn yield_now(&self) -> Result<(), WasiError> {
        self.runtime.yield_now(self.id)?;
        Ok(())
    }

    // Sleeps for a period of time
    pub fn sleep(&self, duration: Duration) -> Result<(), WasiError> {
        let duration = duration.as_nanos();
        let start =
            platform_clock_time_get(Snapshot0Clockid::Monotonic, 1_000_000).unwrap() as u128;
        self.yield_now()?;
        loop {
            let now =
                platform_clock_time_get(Snapshot0Clockid::Monotonic, 1_000_000).unwrap() as u128;
            let delta = match now.checked_sub(start) {
                Some(a) => a,
                None => {
                    break;
                }
            };
            if delta >= duration {
                break;
            }
            let remaining = match duration.checked_sub(delta) {
                Some(a) => Duration::from_nanos(a as u64),
                None => {
                    break;
                }
            };
            std::thread::sleep(remaining.min(Duration::from_millis(10)));
            self.yield_now()?;
        }
        Ok(())
    }

    /// Accesses the virtual networking implementation
    pub fn net(&self) -> &(dyn VirtualNetworking) {
        self.runtime.networking()
    }

    /// Accesses the virtual bus implementation
    pub fn bus(&self) -> &(dyn VirtualBus) {
        self.runtime.bus()
    }

    /// Set the memory of the WasiEnv (can only be done once)
    pub fn set_memory(&mut self, memory: Memory) {
        if self.memory.is_some() {
            panic!("Memory of a WasiEnv can only be set once!");
        }
        self.memory = Some(memory);
    }

    /// Providers safe access to the memory
    /// (it must be initialized before it can be used)
    pub fn memory_view<'a>(&'a self, store: &'a impl AsStoreRef) -> MemoryView<'a> {
        self.memory().view(store)
    }

    /// Get memory, that needs to have been set fist
    pub fn memory(&self) -> &Memory {
        self.memory.as_ref().unwrap()
    }

    /// Get the WASI state
    pub fn state(&self) -> &WasiState {
        &self.state
    }

    pub(crate) fn get_memory_and_wasi_state<'a>(
        &'a self,
        store: &'a impl AsStoreRef,
        _mem_index: u32,
    ) -> (MemoryView<'a>, &WasiState) {
        let memory = self.memory_view(store);
        let state = self.state.deref();
        (memory, state)
    }

    pub(crate) fn get_memory_and_wasi_state_and_inodes<'a>(
        &'a self,
        store: &'a impl AsStoreRef,
        _mem_index: u32,
    ) -> (MemoryView<'a>, &WasiState, RwLockReadGuard<WasiInodes>) {
        let memory = self.memory_view(store);
        let state = self.state.deref();
        let inodes = state.inodes.read().unwrap();
        (memory, state, inodes)
    }

    pub(crate) fn get_memory_and_wasi_state_and_inodes_mut<'a>(
        &'a self,
        store: &'a impl AsStoreRef,
        _mem_index: u32,
    ) -> (MemoryView<'a>, &WasiState, RwLockWriteGuard<WasiInodes>) {
        let memory = self.memory_view(store);
        let state = self.state.deref();
        let inodes = state.inodes.write().unwrap();
        (memory, state, inodes)
    }
}

/// Create an [`Imports`]  from a [`Context`]
pub fn generate_import_object_from_env(
    store: &mut impl AsStoreMut,
    env: &FunctionEnv<WasiEnv>,
    version: WasiVersion,
) -> Imports {
    match version {
        WasiVersion::Snapshot0 => generate_import_object_snapshot0(store, env),
        WasiVersion::Snapshot1 | WasiVersion::Latest => {
            generate_import_object_snapshot1(store, env)
        }
        #[cfg(feature = "wasix")]
        WasiVersion::Wasix32v1 => generate_import_object_wasix32_v1(store, env),
        #[cfg(feature = "wasix")]
        WasiVersion::Wasix64v1 => generate_import_object_wasix64_v1(store, env),
        #[cfg(not(feature = "wasix"))]
        _ => unimplemented!(),
    }
}

fn wasi_unstable_exports(mut store: &mut impl AsStoreMut, env: &FunctionEnv<WasiEnv>) -> Exports {
    let namespace = namespace! {
        "args_get" => Function::new_typed_with_env(&mut store, env, args_get::<Memory32>),
        "args_sizes_get" => Function::new_typed_with_env(&mut store, env, args_sizes_get::<Memory32>),
        "clock_res_get" => Function::new_typed_with_env(&mut store, env, clock_res_get::<Memory32>),
        "clock_time_get" => Function::new_typed_with_env(&mut store, env, clock_time_get::<Memory32>),
        "environ_get" => Function::new_typed_with_env(&mut store, env, environ_get::<Memory32>),
        "environ_sizes_get" => Function::new_typed_with_env(&mut store, env, environ_sizes_get::<Memory32>),
        "fd_advise" => Function::new_typed_with_env(&mut store, env, fd_advise),
        "fd_allocate" => Function::new_typed_with_env(&mut store, env, fd_allocate),
        "fd_close" => Function::new_typed_with_env(&mut store, env, fd_close),
        "fd_datasync" => Function::new_typed_with_env(&mut store, env, fd_datasync),
        "fd_fdstat_get" => Function::new_typed_with_env(&mut store, env, fd_fdstat_get::<Memory32>),
        "fd_fdstat_set_flags" => Function::new_typed_with_env(&mut store, env, fd_fdstat_set_flags),
        "fd_fdstat_set_rights" => Function::new_typed_with_env(&mut store, env, fd_fdstat_set_rights),
        "fd_filestat_get" => Function::new_typed_with_env(&mut store, env, legacy::snapshot0::fd_filestat_get),
        "fd_filestat_set_size" => Function::new_typed_with_env(&mut store, env, fd_filestat_set_size),
        "fd_filestat_set_times" => Function::new_typed_with_env(&mut store, env, fd_filestat_set_times),
        "fd_pread" => Function::new_typed_with_env(&mut store, env, fd_pread::<Memory32>),
        "fd_prestat_get" => Function::new_typed_with_env(&mut store, env, fd_prestat_get::<Memory32>),
        "fd_prestat_dir_name" => Function::new_typed_with_env(&mut store, env, fd_prestat_dir_name::<Memory32>),
        "fd_pwrite" => Function::new_typed_with_env(&mut store, env, fd_pwrite::<Memory32>),
        "fd_read" => Function::new_typed_with_env(&mut store, env, fd_read::<Memory32>),
        "fd_readdir" => Function::new_typed_with_env(&mut store, env, fd_readdir::<Memory32>),
        "fd_renumber" => Function::new_typed_with_env(&mut store, env, fd_renumber),
        "fd_seek" => Function::new_typed_with_env(&mut store, env, legacy::snapshot0::fd_seek),
        "fd_sync" => Function::new_typed_with_env(&mut store, env, fd_sync),
        "fd_tell" => Function::new_typed_with_env(&mut store, env, fd_tell::<Memory32>),
        "fd_write" => Function::new_typed_with_env(&mut store, env, fd_write::<Memory32>),
        "path_create_directory" => Function::new_typed_with_env(&mut store, env, path_create_directory::<Memory32>),
        "path_filestat_get" => Function::new_typed_with_env(&mut store, env, legacy::snapshot0::path_filestat_get),
        "path_filestat_set_times" => Function::new_typed_with_env(&mut store, env, path_filestat_set_times::<Memory32>),
        "path_link" => Function::new_typed_with_env(&mut store, env, path_link::<Memory32>),
        "path_open" => Function::new_typed_with_env(&mut store, env, path_open::<Memory32>),
        "path_readlink" => Function::new_typed_with_env(&mut store, env, path_readlink::<Memory32>),
        "path_remove_directory" => Function::new_typed_with_env(&mut store, env, path_remove_directory::<Memory32>),
        "path_rename" => Function::new_typed_with_env(&mut store, env, path_rename::<Memory32>),
        "path_symlink" => Function::new_typed_with_env(&mut store, env, path_symlink::<Memory32>),
        "path_unlink_file" => Function::new_typed_with_env(&mut store, env, path_unlink_file::<Memory32>),
        "poll_oneoff" => Function::new_typed_with_env(&mut store, env, legacy::snapshot0::poll_oneoff),
        "proc_exit" => Function::new_typed_with_env(&mut store, env, proc_exit),
        "proc_raise" => Function::new_typed_with_env(&mut store, env, proc_raise),
        "random_get" => Function::new_typed_with_env(&mut store, env, random_get::<Memory32>),
        "sched_yield" => Function::new_typed_with_env(&mut store, env, sched_yield),
        "sock_recv" => Function::new_typed_with_env(&mut store, env, sock_recv::<Memory32>),
        "sock_send" => Function::new_typed_with_env(&mut store, env, sock_send::<Memory32>),
        "sock_shutdown" => Function::new_typed_with_env(&mut store, env, sock_shutdown),
    };
    namespace
}

fn wasi_snapshot_preview1_exports(
    mut store: &mut impl AsStoreMut,
    env: &FunctionEnv<WasiEnv>,
) -> Exports {
    let namespace = namespace! {
        "args_get" => Function::new_typed_with_env(&mut store, env, args_get::<Memory32>),
        "args_sizes_get" => Function::new_typed_with_env(&mut store, env, args_sizes_get::<Memory32>),
        "clock_res_get" => Function::new_typed_with_env(&mut store, env, clock_res_get::<Memory32>),
        "clock_time_get" => Function::new_typed_with_env(&mut store, env, clock_time_get::<Memory32>),
        "environ_get" => Function::new_typed_with_env(&mut store, env, environ_get::<Memory32>),
        "environ_sizes_get" => Function::new_typed_with_env(&mut store, env, environ_sizes_get::<Memory32>),
        "fd_advise" => Function::new_typed_with_env(&mut store, env, fd_advise),
        "fd_allocate" => Function::new_typed_with_env(&mut store, env, fd_allocate),
        "fd_close" => Function::new_typed_with_env(&mut store, env, fd_close),
        "fd_datasync" => Function::new_typed_with_env(&mut store, env, fd_datasync),
        "fd_fdstat_get" => Function::new_typed_with_env(&mut store, env, fd_fdstat_get::<Memory32>),
        "fd_fdstat_set_flags" => Function::new_typed_with_env(&mut store, env, fd_fdstat_set_flags),
        "fd_fdstat_set_rights" => Function::new_typed_with_env(&mut store, env, fd_fdstat_set_rights),
        "fd_filestat_get" => Function::new_typed_with_env(&mut store, env, fd_filestat_get::<Memory32>),
        "fd_filestat_set_size" => Function::new_typed_with_env(&mut store, env, fd_filestat_set_size),
        "fd_filestat_set_times" => Function::new_typed_with_env(&mut store, env, fd_filestat_set_times),
        "fd_pread" => Function::new_typed_with_env(&mut store, env, fd_pread::<Memory32>),
        "fd_prestat_get" => Function::new_typed_with_env(&mut store, env, fd_prestat_get::<Memory32>),
        "fd_prestat_dir_name" => Function::new_typed_with_env(&mut store, env, fd_prestat_dir_name::<Memory32>),
        "fd_pwrite" => Function::new_typed_with_env(&mut store, env, fd_pwrite::<Memory32>),
        "fd_read" => Function::new_typed_with_env(&mut store, env, fd_read::<Memory32>),
        "fd_readdir" => Function::new_typed_with_env(&mut store, env, fd_readdir::<Memory32>),
        "fd_renumber" => Function::new_typed_with_env(&mut store, env, fd_renumber),
        "fd_seek" => Function::new_typed_with_env(&mut store, env, fd_seek::<Memory32>),
        "fd_sync" => Function::new_typed_with_env(&mut store, env, fd_sync),
        "fd_tell" => Function::new_typed_with_env(&mut store, env, fd_tell::<Memory32>),
        "fd_write" => Function::new_typed_with_env(&mut store, env, fd_write::<Memory32>),
        "path_create_directory" => Function::new_typed_with_env(&mut store, env, path_create_directory::<Memory32>),
        "path_filestat_get" => Function::new_typed_with_env(&mut store, env, path_filestat_get::<Memory32>),
        "path_filestat_set_times" => Function::new_typed_with_env(&mut store, env, path_filestat_set_times::<Memory32>),
        "path_link" => Function::new_typed_with_env(&mut store, env, path_link::<Memory32>),
        "path_open" => Function::new_typed_with_env(&mut store, env, path_open::<Memory32>),
        "path_readlink" => Function::new_typed_with_env(&mut store, env, path_readlink::<Memory32>),
        "path_remove_directory" => Function::new_typed_with_env(&mut store, env, path_remove_directory::<Memory32>),
        "path_rename" => Function::new_typed_with_env(&mut store, env, path_rename::<Memory32>),
        "path_symlink" => Function::new_typed_with_env(&mut store, env, path_symlink::<Memory32>),
        "path_unlink_file" => Function::new_typed_with_env(&mut store, env, path_unlink_file::<Memory32>),
        "poll_oneoff" => Function::new_typed_with_env(&mut store, env, poll_oneoff::<Memory32>),
        "proc_exit" => Function::new_typed_with_env(&mut store, env, proc_exit),
        "proc_raise" => Function::new_typed_with_env(&mut store, env, proc_raise),
        "random_get" => Function::new_typed_with_env(&mut store, env, random_get::<Memory32>),
        "sched_yield" => Function::new_typed_with_env(&mut store, env, sched_yield),
        "sock_recv" => Function::new_typed_with_env(&mut store, env, sock_recv::<Memory32>),
        "sock_send" => Function::new_typed_with_env(&mut store, env, sock_send::<Memory32>),
        "sock_shutdown" => Function::new_typed_with_env(&mut store, env, sock_shutdown),
    };
    namespace
}
pub fn import_object_for_all_wasi_versions(
    store: &mut impl AsStoreMut,
    env: &FunctionEnv<WasiEnv>,
) -> Imports {
    let wasi_unstable_exports = wasi_unstable_exports(store, env);
    let wasi_snapshot_preview1_exports = wasi_snapshot_preview1_exports(store, env);
    imports! {
        "wasi_unstable" => wasi_unstable_exports,
        "wasi_snapshot_preview1" => wasi_snapshot_preview1_exports,
    }
}

/// Combines a state generating function with the import list for legacy WASI
fn generate_import_object_snapshot0(
    store: &mut impl AsStoreMut,
    env: &FunctionEnv<WasiEnv>,
) -> Imports {
    let wasi_unstable_exports = wasi_unstable_exports(store, env);
    imports! {
        "wasi_unstable" => wasi_unstable_exports
    }
}

fn generate_import_object_snapshot1(
    store: &mut impl AsStoreMut,
    env: &FunctionEnv<WasiEnv>,
) -> Imports {
    let wasi_snapshot_preview1_exports = wasi_snapshot_preview1_exports(store, env);
    imports! {
        "wasi_snapshot_preview1" => wasi_snapshot_preview1_exports
    }
}

/// Combines a state generating function with the import list for snapshot 1
#[cfg(feature = "wasix")]
fn generate_import_object_wasix32_v1(
    mut store: &mut impl AsStoreMut,
    env: &FunctionEnv<WasiEnv>,
) -> Imports {
    use self::wasix32::*;
    imports! {
        "wasix_32v1" => {
            "args_get" => Function::new_typed_with_env(&mut store, env, args_get),
            "args_sizes_get" => Function::new_typed_with_env(&mut store, env, args_sizes_get),
            "clock_res_get" => Function::new_typed_with_env(&mut store, env, clock_res_get),
            "clock_time_get" => Function::new_typed_with_env(&mut store, env, clock_time_get),
            "environ_get" => Function::new_typed_with_env(&mut store, env, environ_get),
            "environ_sizes_get" => Function::new_typed_with_env(&mut store, env, environ_sizes_get),
            "fd_advise" => Function::new_typed_with_env(&mut store, env, fd_advise),
            "fd_allocate" => Function::new_typed_with_env(&mut store, env, fd_allocate),
            "fd_close" => Function::new_typed_with_env(&mut store, env, fd_close),
            "fd_datasync" => Function::new_typed_with_env(&mut store, env, fd_datasync),
            "fd_fdstat_get" => Function::new_typed_with_env(&mut store, env, fd_fdstat_get),
            "fd_fdstat_set_flags" => Function::new_typed_with_env(&mut store, env, fd_fdstat_set_flags),
            "fd_fdstat_set_rights" => Function::new_typed_with_env(&mut store, env, fd_fdstat_set_rights),
            "fd_filestat_get" => Function::new_typed_with_env(&mut store, env, fd_filestat_get),
            "fd_filestat_set_size" => Function::new_typed_with_env(&mut store, env, fd_filestat_set_size),
            "fd_filestat_set_times" => Function::new_typed_with_env(&mut store, env, fd_filestat_set_times),
            "fd_pread" => Function::new_typed_with_env(&mut store, env, fd_pread),
            "fd_prestat_get" => Function::new_typed_with_env(&mut store, env, fd_prestat_get),
            "fd_prestat_dir_name" => Function::new_typed_with_env(&mut store, env, fd_prestat_dir_name),
            "fd_pwrite" => Function::new_typed_with_env(&mut store, env, fd_pwrite),
            "fd_read" => Function::new_typed_with_env(&mut store, env, fd_read),
            "fd_readdir" => Function::new_typed_with_env(&mut store, env, fd_readdir),
            "fd_renumber" => Function::new_typed_with_env(&mut store, env, fd_renumber),
            "fd_dup" => Function::new_typed_with_env(&mut store, env, fd_dup),
            "fd_event" => Function::new_typed_with_env(&mut store, env, fd_event),
            "fd_seek" => Function::new_typed_with_env(&mut store, env, fd_seek),
            "fd_sync" => Function::new_typed_with_env(&mut store, env, fd_sync),
            "fd_tell" => Function::new_typed_with_env(&mut store, env, fd_tell),
            "fd_write" => Function::new_typed_with_env(&mut store, env, fd_write),
            "fd_pipe" => Function::new_typed_with_env(&mut store, env, fd_pipe),
            "path_create_directory" => Function::new_typed_with_env(&mut store, env, path_create_directory),
            "path_filestat_get" => Function::new_typed_with_env(&mut store, env, path_filestat_get),
            "path_filestat_set_times" => Function::new_typed_with_env(&mut store, env, path_filestat_set_times),
            "path_link" => Function::new_typed_with_env(&mut store, env, path_link),
            "path_open" => Function::new_typed_with_env(&mut store, env, path_open),
            "path_readlink" => Function::new_typed_with_env(&mut store, env, path_readlink),
            "path_remove_directory" => Function::new_typed_with_env(&mut store, env, path_remove_directory),
            "path_rename" => Function::new_typed_with_env(&mut store, env, path_rename),
            "path_symlink" => Function::new_typed_with_env(&mut store, env, path_symlink),
            "path_unlink_file" => Function::new_typed_with_env(&mut store, env, path_unlink_file),
            "poll_oneoff" => Function::new_typed_with_env(&mut store, env, poll_oneoff),
            "proc_exit" => Function::new_typed_with_env(&mut store, env, proc_exit),
            "proc_raise" => Function::new_typed_with_env(&mut store, env, proc_raise),
            "random_get" => Function::new_typed_with_env(&mut store, env, random_get),
            "tty_get" => Function::new_typed_with_env(&mut store, env, tty_get),
            "tty_set" => Function::new_typed_with_env(&mut store, env, tty_set),
            "getcwd" => Function::new_typed_with_env(&mut store, env, getcwd),
            "chdir" => Function::new_typed_with_env(&mut store, env, chdir),
            "thread_spawn" => Function::new_typed_with_env(&mut store, env, thread_spawn),
            "thread_sleep" => Function::new_typed_with_env(&mut store, env, thread_sleep),
            "thread_id" => Function::new_typed_with_env(&mut store, env, thread_id),
            "thread_join" => Function::new_typed_with_env(&mut store, env, thread_join),
            "thread_parallelism" => Function::new_typed_with_env(&mut store, env, thread_parallelism),
            "thread_exit" => Function::new_typed_with_env(&mut store, env, thread_exit),
            "sched_yield" => Function::new_typed_with_env(&mut store, env, sched_yield),
            "getpid" => Function::new_typed_with_env(&mut store, env, getpid),
            "process_spawn" => Function::new_typed_with_env(&mut store, env, process_spawn),
            "bus_open_local" => Function::new_typed_with_env(&mut store, env, bus_open_local),
            "bus_open_remote" => Function::new_typed_with_env(&mut store, env, bus_open_remote),
            "bus_close" => Function::new_typed_with_env(&mut store, env, bus_close),
            "bus_call" => Function::new_typed_with_env(&mut store, env, bus_call),
            "bus_subcall" => Function::new_typed_with_env(&mut store, env, bus_subcall),
            "bus_poll" => Function::new_typed_with_env(&mut store, env, bus_poll),
            "call_reply" => Function::new_typed_with_env(&mut store, env, call_reply),
            "call_fault" => Function::new_typed_with_env(&mut store, env, call_fault),
            "call_close" => Function::new_typed_with_env(&mut store, env, call_close),
            "ws_connect" => Function::new_typed_with_env(&mut store, env, ws_connect),
            "http_request" => Function::new_typed_with_env(&mut store, env, http_request),
            "http_status" => Function::new_typed_with_env(&mut store, env, http_status),
            "port_bridge" => Function::new_typed_with_env(&mut store, env, port_bridge),
            "port_unbridge" => Function::new_typed_with_env(&mut store, env, port_unbridge),
            "port_dhcp_acquire" => Function::new_typed_with_env(&mut store, env, port_dhcp_acquire),
            "port_addr_add" => Function::new_typed_with_env(&mut store, env, port_addr_add),
            "port_addr_remove" => Function::new_typed_with_env(&mut store, env, port_addr_remove),
            "port_addr_clear" => Function::new_typed_with_env(&mut store, env, port_addr_clear),
            "port_addr_list" => Function::new_typed_with_env(&mut store, env, port_addr_list),
            "port_mac" => Function::new_typed_with_env(&mut store, env, port_mac),
            "port_gateway_set" => Function::new_typed_with_env(&mut store, env, port_gateway_set),
            "port_route_add" => Function::new_typed_with_env(&mut store, env, port_route_add),
            "port_route_remove" => Function::new_typed_with_env(&mut store, env, port_route_remove),
            "port_route_clear" => Function::new_typed_with_env(&mut store, env, port_route_clear),
            "port_route_list" => Function::new_typed_with_env(&mut store, env, port_route_list),
            "sock_status" => Function::new_typed_with_env(&mut store, env, sock_status),
            "sock_addr_local" => Function::new_typed_with_env(&mut store, env, sock_addr_local),
            "sock_addr_peer" => Function::new_typed_with_env(&mut store, env, sock_addr_peer),
            "sock_open" => Function::new_typed_with_env(&mut store, env, sock_open),
            "sock_set_opt_flag" => Function::new_typed_with_env(&mut store, env, sock_set_opt_flag),
            "sock_get_opt_flag" => Function::new_typed_with_env(&mut store, env, sock_get_opt_flag),
            "sock_set_opt_time" => Function::new_typed_with_env(&mut store, env, sock_set_opt_time),
            "sock_get_opt_time" => Function::new_typed_with_env(&mut store, env, sock_get_opt_time),
            "sock_set_opt_size" => Function::new_typed_with_env(&mut store, env, sock_set_opt_size),
            "sock_get_opt_size" => Function::new_typed_with_env(&mut store, env, sock_get_opt_size),
            "sock_join_multicast_v4" => Function::new_typed_with_env(&mut store, env, sock_join_multicast_v4),
            "sock_leave_multicast_v4" => Function::new_typed_with_env(&mut store, env, sock_leave_multicast_v4),
            "sock_join_multicast_v6" => Function::new_typed_with_env(&mut store, env, sock_join_multicast_v6),
            "sock_leave_multicast_v6" => Function::new_typed_with_env(&mut store, env, sock_leave_multicast_v6),
            "sock_bind" => Function::new_typed_with_env(&mut store, env, sock_bind),
            "sock_listen" => Function::new_typed_with_env(&mut store, env, sock_listen),
            "sock_accept" => Function::new_typed_with_env(&mut store, env, sock_accept),
            "sock_connect" => Function::new_typed_with_env(&mut store, env, sock_connect),
            "sock_recv" => Function::new_typed_with_env(&mut store, env, sock_recv),
            "sock_recv_from" => Function::new_typed_with_env(&mut store, env, sock_recv_from),
            "sock_send" => Function::new_typed_with_env(&mut store, env, sock_send),
            "sock_send_to" => Function::new_typed_with_env(&mut store, env, sock_send_to),
            "sock_send_file" => Function::new_typed_with_env(&mut store, env, sock_send_file),
            "sock_shutdown" => Function::new_typed_with_env(&mut store, env, sock_shutdown),
            "resolve" => Function::new_typed_with_env(&mut store, env, resolve),
        }
    }
}

#[cfg(feature = "wasix")]
fn generate_import_object_wasix64_v1(
    mut store: &mut impl AsStoreMut,
    env: &FunctionEnv<WasiEnv>,
) -> Imports {
    use self::wasix64::*;
    imports! {
        "wasix_64v1" => {
            "args_get" => Function::new_typed_with_env(&mut store, env, args_get),
            "args_sizes_get" => Function::new_typed_with_env(&mut store, env, args_sizes_get),
            "clock_res_get" => Function::new_typed_with_env(&mut store, env, clock_res_get),
            "clock_time_get" => Function::new_typed_with_env(&mut store, env, clock_time_get),
            "environ_get" => Function::new_typed_with_env(&mut store, env, environ_get),
            "environ_sizes_get" => Function::new_typed_with_env(&mut store, env, environ_sizes_get),
            "fd_advise" => Function::new_typed_with_env(&mut store, env, fd_advise),
            "fd_allocate" => Function::new_typed_with_env(&mut store, env, fd_allocate),
            "fd_close" => Function::new_typed_with_env(&mut store, env, fd_close),
            "fd_datasync" => Function::new_typed_with_env(&mut store, env, fd_datasync),
            "fd_fdstat_get" => Function::new_typed_with_env(&mut store, env, fd_fdstat_get),
            "fd_fdstat_set_flags" => Function::new_typed_with_env(&mut store, env, fd_fdstat_set_flags),
            "fd_fdstat_set_rights" => Function::new_typed_with_env(&mut store, env, fd_fdstat_set_rights),
            "fd_filestat_get" => Function::new_typed_with_env(&mut store, env, fd_filestat_get),
            "fd_filestat_set_size" => Function::new_typed_with_env(&mut store, env, fd_filestat_set_size),
            "fd_filestat_set_times" => Function::new_typed_with_env(&mut store, env, fd_filestat_set_times),
            "fd_pread" => Function::new_typed_with_env(&mut store, env, fd_pread),
            "fd_prestat_get" => Function::new_typed_with_env(&mut store, env, fd_prestat_get),
            "fd_prestat_dir_name" => Function::new_typed_with_env(&mut store, env, fd_prestat_dir_name),
            "fd_pwrite" => Function::new_typed_with_env(&mut store, env, fd_pwrite),
            "fd_read" => Function::new_typed_with_env(&mut store, env, fd_read),
            "fd_readdir" => Function::new_typed_with_env(&mut store, env, fd_readdir),
            "fd_renumber" => Function::new_typed_with_env(&mut store, env, fd_renumber),
            "fd_dup" => Function::new_typed_with_env(&mut store, env, fd_dup),
            "fd_event" => Function::new_typed_with_env(&mut store, env, fd_event),
            "fd_seek" => Function::new_typed_with_env(&mut store, env, fd_seek),
            "fd_sync" => Function::new_typed_with_env(&mut store, env, fd_sync),
            "fd_tell" => Function::new_typed_with_env(&mut store, env, fd_tell),
            "fd_write" => Function::new_typed_with_env(&mut store, env, fd_write),
            "fd_pipe" => Function::new_typed_with_env(&mut store, env, fd_pipe),
            "path_create_directory" => Function::new_typed_with_env(&mut store, env, path_create_directory),
            "path_filestat_get" => Function::new_typed_with_env(&mut store, env, path_filestat_get),
            "path_filestat_set_times" => Function::new_typed_with_env(&mut store, env, path_filestat_set_times),
            "path_link" => Function::new_typed_with_env(&mut store, env, path_link),
            "path_open" => Function::new_typed_with_env(&mut store, env, path_open),
            "path_readlink" => Function::new_typed_with_env(&mut store, env, path_readlink),
            "path_remove_directory" => Function::new_typed_with_env(&mut store, env, path_remove_directory),
            "path_rename" => Function::new_typed_with_env(&mut store, env, path_rename),
            "path_symlink" => Function::new_typed_with_env(&mut store, env, path_symlink),
            "path_unlink_file" => Function::new_typed_with_env(&mut store, env, path_unlink_file),
            "poll_oneoff" => Function::new_typed_with_env(&mut store, env, poll_oneoff),
            "proc_exit" => Function::new_typed_with_env(&mut store, env, proc_exit),
            "proc_raise" => Function::new_typed_with_env(&mut store, env, proc_raise),
            "random_get" => Function::new_typed_with_env(&mut store, env, random_get),
            "tty_get" => Function::new_typed_with_env(&mut store, env, tty_get),
            "tty_set" => Function::new_typed_with_env(&mut store, env, tty_set),
            "getcwd" => Function::new_typed_with_env(&mut store, env, getcwd),
            "chdir" => Function::new_typed_with_env(&mut store, env, chdir),
            "thread_spawn" => Function::new_typed_with_env(&mut store, env, thread_spawn),
            "thread_sleep" => Function::new_typed_with_env(&mut store, env, thread_sleep),
            "thread_id" => Function::new_typed_with_env(&mut store, env, thread_id),
            "thread_join" => Function::new_typed_with_env(&mut store, env, thread_join),
            "thread_parallelism" => Function::new_typed_with_env(&mut store, env, thread_parallelism),
            "thread_exit" => Function::new_typed_with_env(&mut store, env, thread_exit),
            "sched_yield" => Function::new_typed_with_env(&mut store, env, sched_yield),
            "getpid" => Function::new_typed_with_env(&mut store, env, getpid),
            "process_spawn" => Function::new_typed_with_env(&mut store, env, process_spawn),
            "bus_open_local" => Function::new_typed_with_env(&mut store, env, bus_open_local),
            "bus_open_remote" => Function::new_typed_with_env(&mut store, env, bus_open_remote),
            "bus_close" => Function::new_typed_with_env(&mut store, env, bus_close),
            "bus_call" => Function::new_typed_with_env(&mut store, env, bus_call),
            "bus_subcall" => Function::new_typed_with_env(&mut store, env, bus_subcall),
            "bus_poll" => Function::new_typed_with_env(&mut store, env, bus_poll),
            "call_reply" => Function::new_typed_with_env(&mut store, env, call_reply),
            "call_fault" => Function::new_typed_with_env(&mut store, env, call_fault),
            "call_close" => Function::new_typed_with_env(&mut store, env, call_close),
            "ws_connect" => Function::new_typed_with_env(&mut store, env, ws_connect),
            "http_request" => Function::new_typed_with_env(&mut store, env, http_request),
            "http_status" => Function::new_typed_with_env(&mut store, env, http_status),
            "port_bridge" => Function::new_typed_with_env(&mut store, env, port_bridge),
            "port_unbridge" => Function::new_typed_with_env(&mut store, env, port_unbridge),
            "port_dhcp_acquire" => Function::new_typed_with_env(&mut store, env, port_dhcp_acquire),
            "port_addr_add" => Function::new_typed_with_env(&mut store, env, port_addr_add),
            "port_addr_remove" => Function::new_typed_with_env(&mut store, env, port_addr_remove),
            "port_addr_clear" => Function::new_typed_with_env(&mut store, env, port_addr_clear),
            "port_addr_list" => Function::new_typed_with_env(&mut store, env, port_addr_list),
            "port_mac" => Function::new_typed_with_env(&mut store, env, port_mac),
            "port_gateway_set" => Function::new_typed_with_env(&mut store, env, port_gateway_set),
            "port_route_add" => Function::new_typed_with_env(&mut store, env, port_route_add),
            "port_route_remove" => Function::new_typed_with_env(&mut store, env, port_route_remove),
            "port_route_clear" => Function::new_typed_with_env(&mut store, env, port_route_clear),
            "port_route_list" => Function::new_typed_with_env(&mut store, env, port_route_list),
            "sock_status" => Function::new_typed_with_env(&mut store, env, sock_status),
            "sock_addr_local" => Function::new_typed_with_env(&mut store, env, sock_addr_local),
            "sock_addr_peer" => Function::new_typed_with_env(&mut store, env, sock_addr_peer),
            "sock_open" => Function::new_typed_with_env(&mut store, env, sock_open),
            "sock_set_opt_flag" => Function::new_typed_with_env(&mut store, env, sock_set_opt_flag),
            "sock_get_opt_flag" => Function::new_typed_with_env(&mut store, env, sock_get_opt_flag),
            "sock_set_opt_time" => Function::new_typed_with_env(&mut store, env, sock_set_opt_time),
            "sock_get_opt_time" => Function::new_typed_with_env(&mut store, env, sock_get_opt_time),
            "sock_set_opt_size" => Function::new_typed_with_env(&mut store, env, sock_set_opt_size),
            "sock_get_opt_size" => Function::new_typed_with_env(&mut store, env, sock_get_opt_size),
            "sock_join_multicast_v4" => Function::new_typed_with_env(&mut store, env, sock_join_multicast_v4),
            "sock_leave_multicast_v4" => Function::new_typed_with_env(&mut store, env, sock_leave_multicast_v4),
            "sock_join_multicast_v6" => Function::new_typed_with_env(&mut store, env, sock_join_multicast_v6),
            "sock_leave_multicast_v6" => Function::new_typed_with_env(&mut store, env, sock_leave_multicast_v6),
            "sock_bind" => Function::new_typed_with_env(&mut store, env, sock_bind),
            "sock_listen" => Function::new_typed_with_env(&mut store, env, sock_listen),
            "sock_accept" => Function::new_typed_with_env(&mut store, env, sock_accept),
            "sock_connect" => Function::new_typed_with_env(&mut store, env, sock_connect),
            "sock_recv" => Function::new_typed_with_env(&mut store, env, sock_recv),
            "sock_recv_from" => Function::new_typed_with_env(&mut store, env, sock_recv_from),
            "sock_send" => Function::new_typed_with_env(&mut store, env, sock_send),
            "sock_send_to" => Function::new_typed_with_env(&mut store, env, sock_send_to),
            "sock_send_file" => Function::new_typed_with_env(&mut store, env, sock_send_file),
            "sock_shutdown" => Function::new_typed_with_env(&mut store, env, sock_shutdown),
            "resolve" => Function::new_typed_with_env(&mut store, env, resolve),
        }
    }
}

fn mem_error_to_wasi(err: MemoryAccessError) -> Errno {
    match err {
        MemoryAccessError::HeapOutOfBounds => Errno::Fault,
        MemoryAccessError::Overflow => Errno::Overflow,
        MemoryAccessError::NonUtf8String => Errno::Inval,
        _ => Errno::Inval,
    }
}

fn mem_error_to_bus(err: MemoryAccessError) -> BusErrno {
    match err {
        MemoryAccessError::HeapOutOfBounds => BusErrno::Memviolation,
        MemoryAccessError::Overflow => BusErrno::Memviolation,
        MemoryAccessError::NonUtf8String => BusErrno::Badrequest,
        _ => BusErrno::Unknown,
    }
}
