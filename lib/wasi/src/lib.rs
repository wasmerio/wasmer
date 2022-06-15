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

use crate::syscalls::*;

pub use crate::state::{
    Fd, Pipe, Stderr, Stdin, Stdout, WasiFs, WasiInodes, WasiState, WasiStateBuilder,
    WasiStateCreationError, ALL_RIGHTS, VIRTUAL_ROOT_FD,
};
pub use crate::syscalls::types;
pub use crate::utils::{
    get_wasi_version, get_wasi_versions, is_wasi_module, is_wasix_module, WasiVersion,
};
pub use wasmer_vbus::{UnsupportedVirtualBus, VirtualBus};
#[deprecated(since = "2.1.0", note = "Please use `wasmer_vfs::FsError`")]
pub use wasmer_vfs::FsError as WasiFsError;
#[deprecated(since = "2.1.0", note = "Please use `wasmer_vfs::VirtualFile`")]
pub use wasmer_vfs::VirtualFile as WasiFile;
pub use wasmer_vfs::{FsError, VirtualFile};
pub use wasmer_vnet::{UnsupportedVirtualNetworking, VirtualNetworking};
use wasmer_wasi_types::__WASI_CLOCK_MONOTONIC;

use derivative::*;
use std::ops::Deref;
use thiserror::Error;
use wasmer::{
    imports, Function, Imports, LazyInit, Memory, Memory32, MemoryAccessError, MemorySize, Module,
    Store, TypedFunction, WasmerEnv,
};

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

#[cfg(target_family = "wasm")]
#[link(wasm_import_module = "__wbindgen_thread_xform__")]
extern "C" {
    fn __wbindgen_thread_id() -> u32;
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
        match guard.recv_timeout(timeout) {
            Ok(_) => true,
            Err(mpsc::RecvTimeoutError::Disconnected) => true,
            Err(mpsc::RecvTimeoutError::Timeout) => false,
        }
    }
}

/// The environment provided to the WASI imports.
#[derive(Derivative, Clone, WasmerEnv)]
#[derivative(Debug)]
pub struct WasiEnv {
    /// ID of this thread (zero is the main thread)
    id: WasiThreadId,
    /// Represents a reference to the memory
    #[wasmer(export)]
    memory: LazyInit<Memory>,
    /// If the module has it then map the thread start
    #[derivative(Debug = "ignore")]
    #[wasmer(export(optional = true, name = "_thread_start"))]
    thread_start: LazyInit<TypedFunction<u64, ()>>,
    #[derivative(Debug = "ignore")]
    #[wasmer(export(optional = true, name = "_reactor_work"))]
    reactor_work: LazyInit<TypedFunction<u64, ()>>,
    #[derivative(Debug = "ignore")]
    #[wasmer(export(optional = true, name = "_reactor_finish"))]
    reactor_finish: LazyInit<TypedFunction<u64, ()>>,
    #[derivative(Debug = "ignore")]
    #[wasmer(export(optional = true, name = "_malloc"))]
    malloc: LazyInit<TypedFunction<u64, u64>>,
    #[derivative(Debug = "ignore")]
    #[wasmer(export(optional = true, name = "_free"))]
    free: LazyInit<TypedFunction<(u64, u64), ()>>,
    /// Shared state of the WASI system. Manages all the data that the
    /// executing WASI program can see.
    pub state: Arc<WasiState>,
    /// Implementation of the WASI runtime.
    pub(crate) runtime: Arc<dyn WasiRuntimeImplementation + Send + Sync + 'static>,
}

impl WasiEnv {
    pub fn new(state: WasiState) -> Self {
        Self {
            id: 0u32.into(),
            state: Arc::new(state),
            memory: LazyInit::new(),
            thread_start: LazyInit::new(),
            reactor_work: LazyInit::new(),
            reactor_finish: LazyInit::new(),
            malloc: LazyInit::new(),
            free: LazyInit::new(),
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

    /// Get the WASI state
    ///
    /// Be careful when using this in host functions that call into Wasm:
    /// if the lock is held and the Wasm calls into a host function that tries
    /// to lock this mutex, the program will deadlock.
    pub fn state(&self) -> &WasiState {
        self.state.deref()
    }

    /// Get a reference to the memory
    pub fn memory(&self) -> &Memory {
        self.memory
            .get_ref()
            .expect("Memory should be set on `WasiEnv` first")
    }

    /// Copy the lazy reference so that when it's initialized during the
    /// export phase, all the other references get a copy of it
    pub fn memory_clone(&self) -> LazyInit<Memory> {
        self.memory.clone()
    }

    /// Get an `Imports` for a specific version of WASI detected in the module.
    pub fn import_object(&mut self, module: &Module) -> Result<Imports, WasiError> {
        let wasi_version = get_wasi_version(module, false).ok_or(WasiError::UnknownWasiVersion)?;
        Ok(generate_import_object_from_env(
            module.store(),
            self.clone(),
            wasi_version,
        ))
    }

    /// Like `import_object` but containing all the WASI versions detected in
    /// the module.
    pub fn import_object_for_all_wasi_versions(
        &mut self,
        module: &Module,
    ) -> Result<Imports, WasiError> {
        let wasi_versions =
            get_wasi_versions(module, false).ok_or(WasiError::UnknownWasiVersion)?;

        let mut resolver = Imports::new();
        for version in wasi_versions.iter() {
            let new_import_object =
                generate_import_object_from_env(module.store(), self.clone(), *version);
            for ((n, m), e) in new_import_object.into_iter() {
                resolver.define(&n, &m, e);
            }
        }

        if is_wasix_module(module) {
            self.state
                .fs
                .is_wasix
                .store(true, std::sync::atomic::Ordering::Release);
        }

        Ok(resolver)
    }

    // Yields execution
    pub fn yield_now(&self) -> Result<(), WasiError> {
        self.runtime.yield_now(self.id)?;
        Ok(())
    }

    // Sleeps for a period of time
    pub fn sleep(&self, duration: Duration) -> Result<(), WasiError> {
        let duration = duration.as_nanos();
        let start = platform_clock_time_get(__WASI_CLOCK_MONOTONIC, 1_000_000).unwrap() as u128;
        self.yield_now()?;
        loop {
            let now = platform_clock_time_get(__WASI_CLOCK_MONOTONIC, 1_000_000).unwrap() as u128;
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
    pub(crate) fn get_memory_and_wasi_state(&self, _mem_index: u32) -> (&Memory, &WasiState) {
        let memory = self.memory();
        let state = self.state.deref();
        (memory, state)
    }

    pub(crate) fn get_memory_and_wasi_state_and_inodes(
        &self,
        _mem_index: u32,
    ) -> (&Memory, &WasiState, RwLockReadGuard<WasiInodes>) {
        let memory = self.memory();
        let state = self.state.deref();
        let inodes = state.inodes.read().unwrap();
        (memory, state, inodes)
    }

    pub(crate) fn get_memory_and_wasi_state_and_inodes_mut(
        &self,
        _mem_index: u32,
    ) -> (&Memory, &WasiState, RwLockWriteGuard<WasiInodes>) {
        let memory = self.memory();
        let state = self.state.deref();
        let inodes = state.inodes.write().unwrap();
        (memory, state, inodes)
    }
}

/// Create an [`Imports`] with an existing [`WasiEnv`]. `WasiEnv`
/// needs a [`WasiState`], that can be constructed from a
/// [`WasiStateBuilder`](state::WasiStateBuilder).
pub fn generate_import_object_from_env(
    store: &Store,
    env: WasiEnv,
    version: WasiVersion,
) -> Imports {
    match version {
        WasiVersion::Snapshot0 => generate_import_object_snapshot0(store, env),
        WasiVersion::Wasix32v1 => generate_import_object_wasix32_v1(store, env),
        WasiVersion::Wasix64v1 => generate_import_object_wasix64_v1(store, env),
        WasiVersion::Snapshot1 | WasiVersion::Latest => {
            generate_import_object_snapshot1(store, env)
        }
    }
}

/// Combines a state generating function with the import list for legacy WASI
fn generate_import_object_snapshot0(store: &Store, env: WasiEnv) -> Imports {
    use self::wasi::*;
    imports! {
        "wasi_unstable" => {
            "args_get" => Function::new_native_with_env(store, env.clone(), args_get),
            "args_sizes_get" => Function::new_native_with_env(store, env.clone(), args_sizes_get),
            "clock_res_get" => Function::new_native_with_env(store, env.clone(), clock_res_get),
            "clock_time_get" => Function::new_native_with_env(store, env.clone(), clock_time_get),
            "environ_get" => Function::new_native_with_env(store, env.clone(), environ_get),
            "environ_sizes_get" => Function::new_native_with_env(store, env.clone(), environ_sizes_get),
            "fd_advise" => Function::new_native_with_env(store, env.clone(), fd_advise),
            "fd_allocate" => Function::new_native_with_env(store, env.clone(), fd_allocate),
            "fd_close" => Function::new_native_with_env(store, env.clone(), fd_close),
            "fd_datasync" => Function::new_native_with_env(store, env.clone(), fd_datasync),
            "fd_fdstat_get" => Function::new_native_with_env(store, env.clone(), fd_fdstat_get),
            "fd_fdstat_set_flags" => Function::new_native_with_env(store, env.clone(), fd_fdstat_set_flags),
            "fd_fdstat_set_rights" => Function::new_native_with_env(store, env.clone(), fd_fdstat_set_rights),
            "fd_filestat_get" => Function::new_native_with_env(store, env.clone(), legacy::snapshot0::fd_filestat_get),
            "fd_filestat_set_size" => Function::new_native_with_env(store, env.clone(), fd_filestat_set_size),
            "fd_filestat_set_times" => Function::new_native_with_env(store, env.clone(), fd_filestat_set_times),
            "fd_pread" => Function::new_native_with_env(store, env.clone(), fd_pread),
            "fd_prestat_get" => Function::new_native_with_env(store, env.clone(), fd_prestat_get),
            "fd_prestat_dir_name" => Function::new_native_with_env(store, env.clone(), fd_prestat_dir_name),
            "fd_pwrite" => Function::new_native_with_env(store, env.clone(), fd_pwrite),
            "fd_read" => Function::new_native_with_env(store, env.clone(), fd_read),
            "fd_readdir" => Function::new_native_with_env(store, env.clone(), fd_readdir),
            "fd_renumber" => Function::new_native_with_env(store, env.clone(), fd_renumber),
            "fd_seek" => Function::new_native_with_env(store, env.clone(), legacy::snapshot0::fd_seek),
            "fd_sync" => Function::new_native_with_env(store, env.clone(), fd_sync),
            "fd_tell" => Function::new_native_with_env(store, env.clone(), fd_tell),
            "fd_write" => Function::new_native_with_env(store, env.clone(), fd_write),
            "path_create_directory" => Function::new_native_with_env(store, env.clone(), path_create_directory),
            "path_filestat_get" => Function::new_native_with_env(store, env.clone(), legacy::snapshot0::path_filestat_get),
            "path_filestat_set_times" => Function::new_native_with_env(store, env.clone(), path_filestat_set_times),
            "path_link" => Function::new_native_with_env(store, env.clone(), path_link),
            "path_open" => Function::new_native_with_env(store, env.clone(), path_open),
            "path_readlink" => Function::new_native_with_env(store, env.clone(), path_readlink),
            "path_remove_directory" => Function::new_native_with_env(store, env.clone(), path_remove_directory),
            "path_rename" => Function::new_native_with_env(store, env.clone(), path_rename),
            "path_symlink" => Function::new_native_with_env(store, env.clone(), path_symlink),
            "path_unlink_file" => Function::new_native_with_env(store, env.clone(), path_unlink_file),
            "poll_oneoff" => Function::new_native_with_env(store, env.clone(), legacy::snapshot0::poll_oneoff),
            "proc_exit" => Function::new_native_with_env(store, env.clone(), proc_exit),
            "proc_raise" => Function::new_native_with_env(store, env.clone(), proc_raise),
            "random_get" => Function::new_native_with_env(store, env.clone(), random_get),
            "sched_yield" => Function::new_native_with_env(store, env.clone(), sched_yield),
            "sock_recv" => Function::new_native_with_env(store, env.clone(), sock_recv),
            "sock_send" => Function::new_native_with_env(store, env.clone(), sock_send),
            "sock_shutdown" => Function::new_native_with_env(store, env, sock_shutdown),
        },
    }
}

/// Combines a state generating function with the import list for snapshot 1
fn generate_import_object_snapshot1(store: &Store, env: WasiEnv) -> Imports {
    use self::wasi::*;
    imports! {
        "wasi_snapshot_preview1" => {
            "args_get" => Function::new_native_with_env(store, env.clone(), args_get),
            "args_sizes_get" => Function::new_native_with_env(store, env.clone(), args_sizes_get),
            "clock_res_get" => Function::new_native_with_env(store, env.clone(), clock_res_get),
            "clock_time_get" => Function::new_native_with_env(store, env.clone(), clock_time_get),
            "environ_get" => Function::new_native_with_env(store, env.clone(), environ_get),
            "environ_sizes_get" => Function::new_native_with_env(store, env.clone(), environ_sizes_get),
            "fd_advise" => Function::new_native_with_env(store, env.clone(), fd_advise),
            "fd_allocate" => Function::new_native_with_env(store, env.clone(), fd_allocate),
            "fd_close" => Function::new_native_with_env(store, env.clone(), fd_close),
            "fd_datasync" => Function::new_native_with_env(store, env.clone(), fd_datasync),
            "fd_fdstat_get" => Function::new_native_with_env(store, env.clone(), fd_fdstat_get),
            "fd_fdstat_set_flags" => Function::new_native_with_env(store, env.clone(), fd_fdstat_set_flags),
            "fd_fdstat_set_rights" => Function::new_native_with_env(store, env.clone(), fd_fdstat_set_rights),
            "fd_filestat_get" => Function::new_native_with_env(store, env.clone(), fd_filestat_get),
            "fd_filestat_set_size" => Function::new_native_with_env(store, env.clone(), fd_filestat_set_size),
            "fd_filestat_set_times" => Function::new_native_with_env(store, env.clone(), fd_filestat_set_times),
            "fd_pread" => Function::new_native_with_env(store, env.clone(), fd_pread),
            "fd_prestat_get" => Function::new_native_with_env(store, env.clone(), fd_prestat_get),
            "fd_prestat_dir_name" => Function::new_native_with_env(store, env.clone(), fd_prestat_dir_name),
            "fd_pwrite" => Function::new_native_with_env(store, env.clone(), fd_pwrite),
            "fd_read" => Function::new_native_with_env(store, env.clone(), fd_read),
            "fd_readdir" => Function::new_native_with_env(store, env.clone(), fd_readdir),
            "fd_renumber" => Function::new_native_with_env(store, env.clone(), fd_renumber),
            "fd_seek" => Function::new_native_with_env(store, env.clone(), fd_seek),
            "fd_sync" => Function::new_native_with_env(store, env.clone(), fd_sync),
            "fd_tell" => Function::new_native_with_env(store, env.clone(), fd_tell),
            "fd_write" => Function::new_native_with_env(store, env.clone(), fd_write),
            "path_create_directory" => Function::new_native_with_env(store, env.clone(), path_create_directory),
            "path_filestat_get" => Function::new_native_with_env(store, env.clone(), path_filestat_get),
            "path_filestat_set_times" => Function::new_native_with_env(store, env.clone(), path_filestat_set_times),
            "path_link" => Function::new_native_with_env(store, env.clone(), path_link),
            "path_open" => Function::new_native_with_env(store, env.clone(), path_open),
            "path_readlink" => Function::new_native_with_env(store, env.clone(), path_readlink),
            "path_remove_directory" => Function::new_native_with_env(store, env.clone(), path_remove_directory),
            "path_rename" => Function::new_native_with_env(store, env.clone(), path_rename),
            "path_symlink" => Function::new_native_with_env(store, env.clone(), path_symlink),
            "path_unlink_file" => Function::new_native_with_env(store, env.clone(), path_unlink_file),
            "poll_oneoff" => Function::new_native_with_env(store, env.clone(), poll_oneoff),
            "proc_exit" => Function::new_native_with_env(store, env.clone(), proc_exit),
            "proc_raise" => Function::new_native_with_env(store, env.clone(), proc_raise),
            "random_get" => Function::new_native_with_env(store, env.clone(), random_get),
            "sched_yield" => Function::new_native_with_env(store, env.clone(), sched_yield),
            "sock_recv" => Function::new_native_with_env(store, env.clone(), sock_recv),
            "sock_send" => Function::new_native_with_env(store, env.clone(), sock_send),
            "sock_shutdown" => Function::new_native_with_env(store, env, sock_shutdown),
        }
    }
}

/// Combines a state generating function with the import list for snapshot 1
fn generate_import_object_wasix32_v1(store: &Store, env: WasiEnv) -> Imports {
    use self::wasix32::*;
    imports! {
        "wasix_32v1" => {
            "args_get" => Function::new_native_with_env(store, env.clone(), args_get),
            "args_sizes_get" => Function::new_native_with_env(store, env.clone(), args_sizes_get),
            "clock_res_get" => Function::new_native_with_env(store, env.clone(), clock_res_get),
            "clock_time_get" => Function::new_native_with_env(store, env.clone(), clock_time_get),
            "environ_get" => Function::new_native_with_env(store, env.clone(), environ_get),
            "environ_sizes_get" => Function::new_native_with_env(store, env.clone(), environ_sizes_get),
            "fd_advise" => Function::new_native_with_env(store, env.clone(), fd_advise),
            "fd_allocate" => Function::new_native_with_env(store, env.clone(), fd_allocate),
            "fd_close" => Function::new_native_with_env(store, env.clone(), fd_close),
            "fd_datasync" => Function::new_native_with_env(store, env.clone(), fd_datasync),
            "fd_fdstat_get" => Function::new_native_with_env(store, env.clone(), fd_fdstat_get),
            "fd_fdstat_set_flags" => Function::new_native_with_env(store, env.clone(), fd_fdstat_set_flags),
            "fd_fdstat_set_rights" => Function::new_native_with_env(store, env.clone(), fd_fdstat_set_rights),
            "fd_filestat_get" => Function::new_native_with_env(store, env.clone(), fd_filestat_get),
            "fd_filestat_set_size" => Function::new_native_with_env(store, env.clone(), fd_filestat_set_size),
            "fd_filestat_set_times" => Function::new_native_with_env(store, env.clone(), fd_filestat_set_times),
            "fd_pread" => Function::new_native_with_env(store, env.clone(), fd_pread),
            "fd_prestat_get" => Function::new_native_with_env(store, env.clone(), fd_prestat_get),
            "fd_prestat_dir_name" => Function::new_native_with_env(store, env.clone(), fd_prestat_dir_name),
            "fd_pwrite" => Function::new_native_with_env(store, env.clone(), fd_pwrite),
            "fd_read" => Function::new_native_with_env(store, env.clone(), fd_read),
            "fd_readdir" => Function::new_native_with_env(store, env.clone(), fd_readdir),
            "fd_renumber" => Function::new_native_with_env(store, env.clone(), fd_renumber),
            "fd_dup" => Function::new_native_with_env(store, env.clone(), fd_dup),
            "fd_event" => Function::new_native_with_env(store, env.clone(), fd_event),
            "fd_seek" => Function::new_native_with_env(store, env.clone(), fd_seek),
            "fd_sync" => Function::new_native_with_env(store, env.clone(), fd_sync),
            "fd_tell" => Function::new_native_with_env(store, env.clone(), fd_tell),
            "fd_write" => Function::new_native_with_env(store, env.clone(), fd_write),
            "fd_pipe" => Function::new_native_with_env(store, env.clone(), fd_pipe),
            "path_create_directory" => Function::new_native_with_env(store, env.clone(), path_create_directory),
            "path_filestat_get" => Function::new_native_with_env(store, env.clone(), path_filestat_get),
            "path_filestat_set_times" => Function::new_native_with_env(store, env.clone(), path_filestat_set_times),
            "path_link" => Function::new_native_with_env(store, env.clone(), path_link),
            "path_open" => Function::new_native_with_env(store, env.clone(), path_open),
            "path_readlink" => Function::new_native_with_env(store, env.clone(), path_readlink),
            "path_remove_directory" => Function::new_native_with_env(store, env.clone(), path_remove_directory),
            "path_rename" => Function::new_native_with_env(store, env.clone(), path_rename),
            "path_symlink" => Function::new_native_with_env(store, env.clone(), path_symlink),
            "path_unlink_file" => Function::new_native_with_env(store, env.clone(), path_unlink_file),
            "poll_oneoff" => Function::new_native_with_env(store, env.clone(), poll_oneoff),
            "proc_exit" => Function::new_native_with_env(store, env.clone(), proc_exit),
            "proc_raise" => Function::new_native_with_env(store, env.clone(), proc_raise),
            "random_get" => Function::new_native_with_env(store, env.clone(), random_get),
            "tty_get" => Function::new_native_with_env(store, env.clone(), tty_get),
            "tty_set" => Function::new_native_with_env(store, env.clone(), tty_set),
            "getcwd" => Function::new_native_with_env(store, env.clone(), getcwd),
            "chdir" => Function::new_native_with_env(store, env.clone(), chdir),
            "thread_spawn" => Function::new_native_with_env(store, env.clone(), thread_spawn),
            "thread_sleep" => Function::new_native_with_env(store, env.clone(), thread_sleep),
            "thread_id" => Function::new_native_with_env(store, env.clone(), thread_id),
            "thread_join" => Function::new_native_with_env(store, env.clone(), thread_join),
            "thread_parallelism" => Function::new_native_with_env(store, env.clone(), thread_parallelism),
            "thread_exit" => Function::new_native_with_env(store, env.clone(), thread_exit),
            "sched_yield" => Function::new_native_with_env(store, env.clone(), sched_yield),
            "getpid" => Function::new_native_with_env(store, env.clone(), getpid),
            "process_spawn" => Function::new_native_with_env(store, env.clone(), process_spawn),
            "bus_open_local" => Function::new_native_with_env(store, env.clone(), bus_open_local),
            "bus_open_remote" => Function::new_native_with_env(store, env.clone(), bus_open_remote),
            "bus_close" => Function::new_native_with_env(store, env.clone(), bus_close),
            "bus_call" => Function::new_native_with_env(store, env.clone(), bus_call),
            "bus_subcall" => Function::new_native_with_env(store, env.clone(), bus_subcall),
            "bus_poll" => Function::new_native_with_env(store, env.clone(), bus_poll),
            "call_reply" => Function::new_native_with_env(store, env.clone(), call_reply),
            "call_fault" => Function::new_native_with_env(store, env.clone(), call_fault),
            "call_close" => Function::new_native_with_env(store, env.clone(), call_close),
            "ws_connect" => Function::new_native_with_env(store, env.clone(), ws_connect),
            "http_request" => Function::new_native_with_env(store, env.clone(), http_request),
            "http_status" => Function::new_native_with_env(store, env.clone(), http_status),
            "port_bridge" => Function::new_native_with_env(store, env.clone(), port_bridge),
            "port_unbridge" => Function::new_native_with_env(store, env.clone(), port_unbridge),
            "port_dhcp_acquire" => Function::new_native_with_env(store, env.clone(), port_dhcp_acquire),
            "port_addr_add" => Function::new_native_with_env(store, env.clone(), port_addr_add),
            "port_addr_remove" => Function::new_native_with_env(store, env.clone(), port_addr_remove),
            "port_addr_clear" => Function::new_native_with_env(store, env.clone(), port_addr_clear),
            "port_addr_list" => Function::new_native_with_env(store, env.clone(), port_addr_list),
            "port_mac" => Function::new_native_with_env(store, env.clone(), port_mac),
            "port_gateway_set" => Function::new_native_with_env(store, env.clone(), port_gateway_set),
            "port_route_add" => Function::new_native_with_env(store, env.clone(), port_route_add),
            "port_route_remove" => Function::new_native_with_env(store, env.clone(), port_route_remove),
            "port_route_clear" => Function::new_native_with_env(store, env.clone(), port_route_clear),
            "port_route_list" => Function::new_native_with_env(store, env.clone(), port_route_list),
            "sock_status" => Function::new_native_with_env(store, env.clone(), sock_status),
            "sock_addr_local" => Function::new_native_with_env(store, env.clone(), sock_addr_local),
            "sock_addr_peer" => Function::new_native_with_env(store, env.clone(), sock_addr_peer),
            "sock_open" => Function::new_native_with_env(store, env.clone(), sock_open),
            "sock_set_opt_flag" => Function::new_native_with_env(store, env.clone(), sock_set_opt_flag),
            "sock_get_opt_flag" => Function::new_native_with_env(store, env.clone(), sock_get_opt_flag),
            "sock_set_opt_time" => Function::new_native_with_env(store, env.clone(), sock_set_opt_time),
            "sock_get_opt_time" => Function::new_native_with_env(store, env.clone(), sock_get_opt_time),
            "sock_set_opt_size" => Function::new_native_with_env(store, env.clone(), sock_set_opt_size),
            "sock_get_opt_size" => Function::new_native_with_env(store, env.clone(), sock_get_opt_size),
            "sock_join_multicast_v4" => Function::new_native_with_env(store, env.clone(), sock_join_multicast_v4),
            "sock_leave_multicast_v4" => Function::new_native_with_env(store, env.clone(), sock_leave_multicast_v4),
            "sock_join_multicast_v6" => Function::new_native_with_env(store, env.clone(), sock_join_multicast_v6),
            "sock_leave_multicast_v6" => Function::new_native_with_env(store, env.clone(), sock_leave_multicast_v6),
            "sock_bind" => Function::new_native_with_env(store, env.clone(), sock_bind),
            "sock_listen" => Function::new_native_with_env(store, env.clone(), sock_listen),
            "sock_accept" => Function::new_native_with_env(store, env.clone(), sock_accept),
            "sock_connect" => Function::new_native_with_env(store, env.clone(), sock_connect),
            "sock_recv" => Function::new_native_with_env(store, env.clone(), sock_recv),
            "sock_recv_from" => Function::new_native_with_env(store, env.clone(), sock_recv_from),
            "sock_send" => Function::new_native_with_env(store, env.clone(), sock_send),
            "sock_send_to" => Function::new_native_with_env(store, env.clone(), sock_send_to),
            "sock_send_file" => Function::new_native_with_env(store, env.clone(), sock_send_file),
            "sock_shutdown" => Function::new_native_with_env(store, env.clone(), sock_shutdown),
            "resolve" => Function::new_native_with_env(store, env, resolve),
        }
    }
}

fn generate_import_object_wasix64_v1(store: &Store, env: WasiEnv) -> Imports {
    use self::wasix64::*;
    imports! {
        "wasix_64v1" => {
            "args_get" => Function::new_native_with_env(store, env.clone(), args_get),
            "args_sizes_get" => Function::new_native_with_env(store, env.clone(), args_sizes_get),
            "clock_res_get" => Function::new_native_with_env(store, env.clone(), clock_res_get),
            "clock_time_get" => Function::new_native_with_env(store, env.clone(), clock_time_get),
            "environ_get" => Function::new_native_with_env(store, env.clone(), environ_get),
            "environ_sizes_get" => Function::new_native_with_env(store, env.clone(), environ_sizes_get),
            "fd_advise" => Function::new_native_with_env(store, env.clone(), fd_advise),
            "fd_allocate" => Function::new_native_with_env(store, env.clone(), fd_allocate),
            "fd_close" => Function::new_native_with_env(store, env.clone(), fd_close),
            "fd_datasync" => Function::new_native_with_env(store, env.clone(), fd_datasync),
            "fd_fdstat_get" => Function::new_native_with_env(store, env.clone(), fd_fdstat_get),
            "fd_fdstat_set_flags" => Function::new_native_with_env(store, env.clone(), fd_fdstat_set_flags),
            "fd_fdstat_set_rights" => Function::new_native_with_env(store, env.clone(), fd_fdstat_set_rights),
            "fd_filestat_get" => Function::new_native_with_env(store, env.clone(), fd_filestat_get),
            "fd_filestat_set_size" => Function::new_native_with_env(store, env.clone(), fd_filestat_set_size),
            "fd_filestat_set_times" => Function::new_native_with_env(store, env.clone(), fd_filestat_set_times),
            "fd_pread" => Function::new_native_with_env(store, env.clone(), fd_pread),
            "fd_prestat_get" => Function::new_native_with_env(store, env.clone(), fd_prestat_get),
            "fd_prestat_dir_name" => Function::new_native_with_env(store, env.clone(), fd_prestat_dir_name),
            "fd_pwrite" => Function::new_native_with_env(store, env.clone(), fd_pwrite),
            "fd_read" => Function::new_native_with_env(store, env.clone(), fd_read),
            "fd_readdir" => Function::new_native_with_env(store, env.clone(), fd_readdir),
            "fd_renumber" => Function::new_native_with_env(store, env.clone(), fd_renumber),
            "fd_dup" => Function::new_native_with_env(store, env.clone(), fd_dup),
            "fd_event" => Function::new_native_with_env(store, env.clone(), fd_event),
            "fd_seek" => Function::new_native_with_env(store, env.clone(), fd_seek),
            "fd_sync" => Function::new_native_with_env(store, env.clone(), fd_sync),
            "fd_tell" => Function::new_native_with_env(store, env.clone(), fd_tell),
            "fd_write" => Function::new_native_with_env(store, env.clone(), fd_write),
            "fd_pipe" => Function::new_native_with_env(store, env.clone(), fd_pipe),
            "path_create_directory" => Function::new_native_with_env(store, env.clone(), path_create_directory),
            "path_filestat_get" => Function::new_native_with_env(store, env.clone(), path_filestat_get),
            "path_filestat_set_times" => Function::new_native_with_env(store, env.clone(), path_filestat_set_times),
            "path_link" => Function::new_native_with_env(store, env.clone(), path_link),
            "path_open" => Function::new_native_with_env(store, env.clone(), path_open),
            "path_readlink" => Function::new_native_with_env(store, env.clone(), path_readlink),
            "path_remove_directory" => Function::new_native_with_env(store, env.clone(), path_remove_directory),
            "path_rename" => Function::new_native_with_env(store, env.clone(), path_rename),
            "path_symlink" => Function::new_native_with_env(store, env.clone(), path_symlink),
            "path_unlink_file" => Function::new_native_with_env(store, env.clone(), path_unlink_file),
            "poll_oneoff" => Function::new_native_with_env(store, env.clone(), poll_oneoff),
            "proc_exit" => Function::new_native_with_env(store, env.clone(), proc_exit),
            "proc_raise" => Function::new_native_with_env(store, env.clone(), proc_raise),
            "random_get" => Function::new_native_with_env(store, env.clone(), random_get),
            "tty_get" => Function::new_native_with_env(store, env.clone(), tty_get),
            "tty_set" => Function::new_native_with_env(store, env.clone(), tty_set),
            "getcwd" => Function::new_native_with_env(store, env.clone(), getcwd),
            "chdir" => Function::new_native_with_env(store, env.clone(), chdir),
            "thread_spawn" => Function::new_native_with_env(store, env.clone(), thread_spawn),
            "thread_sleep" => Function::new_native_with_env(store, env.clone(), thread_sleep),
            "thread_id" => Function::new_native_with_env(store, env.clone(), thread_id),
            "thread_join" => Function::new_native_with_env(store, env.clone(), thread_join),
            "thread_parallelism" => Function::new_native_with_env(store, env.clone(), thread_parallelism),
            "thread_exit" => Function::new_native_with_env(store, env.clone(), thread_exit),
            "sched_yield" => Function::new_native_with_env(store, env.clone(), sched_yield),
            "getpid" => Function::new_native_with_env(store, env.clone(), getpid),
            "process_spawn" => Function::new_native_with_env(store, env.clone(), process_spawn),
            "bus_open_local" => Function::new_native_with_env(store, env.clone(), bus_open_local),
            "bus_open_remote" => Function::new_native_with_env(store, env.clone(), bus_open_remote),
            "bus_close" => Function::new_native_with_env(store, env.clone(), bus_close),
            "bus_call" => Function::new_native_with_env(store, env.clone(), bus_call),
            "bus_subcall" => Function::new_native_with_env(store, env.clone(), bus_subcall),
            "bus_poll" => Function::new_native_with_env(store, env.clone(), bus_poll),
            "call_reply" => Function::new_native_with_env(store, env.clone(), call_reply),
            "call_fault" => Function::new_native_with_env(store, env.clone(), call_fault),
            "call_close" => Function::new_native_with_env(store, env.clone(), call_close),
            "ws_connect" => Function::new_native_with_env(store, env.clone(), ws_connect),
            "http_request" => Function::new_native_with_env(store, env.clone(), http_request),
            "http_status" => Function::new_native_with_env(store, env.clone(), http_status),
            "port_bridge" => Function::new_native_with_env(store, env.clone(), port_bridge),
            "port_unbridge" => Function::new_native_with_env(store, env.clone(), port_unbridge),
            "port_dhcp_acquire" => Function::new_native_with_env(store, env.clone(), port_dhcp_acquire),
            "port_addr_add" => Function::new_native_with_env(store, env.clone(), port_addr_add),
            "port_addr_remove" => Function::new_native_with_env(store, env.clone(), port_addr_remove),
            "port_addr_clear" => Function::new_native_with_env(store, env.clone(), port_addr_clear),
            "port_addr_list" => Function::new_native_with_env(store, env.clone(), port_addr_list),
            "port_mac" => Function::new_native_with_env(store, env.clone(), port_mac),
            "port_gateway_set" => Function::new_native_with_env(store, env.clone(), port_gateway_set),
            "port_route_add" => Function::new_native_with_env(store, env.clone(), port_route_add),
            "port_route_remove" => Function::new_native_with_env(store, env.clone(), port_route_remove),
            "port_route_clear" => Function::new_native_with_env(store, env.clone(), port_route_clear),
            "port_route_list" => Function::new_native_with_env(store, env.clone(), port_route_list),
            "sock_status" => Function::new_native_with_env(store, env.clone(), sock_status),
            "sock_addr_local" => Function::new_native_with_env(store, env.clone(), sock_addr_local),
            "sock_addr_peer" => Function::new_native_with_env(store, env.clone(), sock_addr_peer),
            "sock_open" => Function::new_native_with_env(store, env.clone(), sock_open),
            "sock_set_opt_flag" => Function::new_native_with_env(store, env.clone(), sock_set_opt_flag),
            "sock_get_opt_flag" => Function::new_native_with_env(store, env.clone(), sock_get_opt_flag),
            "sock_set_opt_time" => Function::new_native_with_env(store, env.clone(), sock_set_opt_time),
            "sock_get_opt_time" => Function::new_native_with_env(store, env.clone(), sock_get_opt_time),
            "sock_set_opt_size" => Function::new_native_with_env(store, env.clone(), sock_set_opt_size),
            "sock_get_opt_size" => Function::new_native_with_env(store, env.clone(), sock_get_opt_size),
            "sock_join_multicast_v4" => Function::new_native_with_env(store, env.clone(), sock_join_multicast_v4),
            "sock_leave_multicast_v4" => Function::new_native_with_env(store, env.clone(), sock_leave_multicast_v4),
            "sock_join_multicast_v6" => Function::new_native_with_env(store, env.clone(), sock_join_multicast_v6),
            "sock_leave_multicast_v6" => Function::new_native_with_env(store, env.clone(), sock_leave_multicast_v6),
            "sock_bind" => Function::new_native_with_env(store, env.clone(), sock_bind),
            "sock_listen" => Function::new_native_with_env(store, env.clone(), sock_listen),
            "sock_accept" => Function::new_native_with_env(store, env.clone(), sock_accept),
            "sock_connect" => Function::new_native_with_env(store, env.clone(), sock_connect),
            "sock_recv" => Function::new_native_with_env(store, env.clone(), sock_recv),
            "sock_recv_from" => Function::new_native_with_env(store, env.clone(), sock_recv_from),
            "sock_send" => Function::new_native_with_env(store, env.clone(), sock_send),
            "sock_send_to" => Function::new_native_with_env(store, env.clone(), sock_send_to),
            "sock_send_file" => Function::new_native_with_env(store, env.clone(), sock_send_file),
            "sock_shutdown" => Function::new_native_with_env(store, env.clone(), sock_shutdown),
            "resolve" => Function::new_native_with_env(store, env, resolve),
        }
    }
}

fn mem_error_to_wasi(err: MemoryAccessError) -> types::__wasi_errno_t {
    match err {
        MemoryAccessError::HeapOutOfBounds => types::__WASI_EFAULT,
        MemoryAccessError::Overflow => types::__WASI_EOVERFLOW,
        MemoryAccessError::NonUtf8String => types::__WASI_EINVAL,
        _ => types::__WASI_EINVAL,
    }
}

fn mem_error_to_bus(err: MemoryAccessError) -> types::__bus_errno_t {
    match err {
        MemoryAccessError::HeapOutOfBounds => types::__BUS_EMEMVIOLATION,
        MemoryAccessError::Overflow => types::__BUS_EMEMVIOLATION,
        MemoryAccessError::NonUtf8String => types::__BUS_EBADREQUEST,
        _ => types::__BUS_EUNKNOWN,
    }
}
