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

pub use crate::state::{
    Fd, Pipe, Stderr, Stdin, Stdout, WasiFs, WasiInodes, WasiState, WasiStateBuilder,
    WasiStateCreationError, ALL_RIGHTS, VIRTUAL_ROOT_FD,
};
pub use crate::syscalls::types;
pub use crate::utils::{
    get_wasi_version, get_wasi_versions, is_wasi_module, is_wasix_module, WasiVersion,
};
use bytes::Bytes;
use derivative::Derivative;
pub use wasmer_vbus::{UnsupportedVirtualBus, VirtualBus};
#[deprecated(since = "2.1.0", note = "Please use `wasmer_vfs::FsError`")]
pub use wasmer_vfs::FsError as WasiFsError;
#[deprecated(since = "2.1.0", note = "Please use `wasmer_vfs::VirtualFile`")]
pub use wasmer_vfs::VirtualFile as WasiFile;
pub use wasmer_vfs::{FsError, VirtualFile};
pub use wasmer_vnet::{UnsupportedVirtualNetworking, VirtualNetworking};
use wasmer_wasi_types::__WASI_CLOCK_MONOTONIC;

use std::cell::RefCell;
use std::ops::Deref;
use thiserror::Error;
use wasmer::{
    imports, namespace, AsStoreMut, Exports, Function, FunctionEnv, Imports, Memory, Memory32,
    MemoryAccessError, MemorySize, Module, TypedFunction, Reactors, Memory64, MemoryView, AsStoreRef, Instance, ExportError
};

pub use runtime::{
    PluggableRuntimeImplementation, WasiRuntimeImplementation, WasiThreadError, WasiTtyState
};
use std::sync::{Arc, RwLockReadGuard, RwLockWriteGuard};
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
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WasiThreadId(u32);

impl WasiThreadId {
    pub fn raw(&self) -> u32 {
        self.0
    }

    pub fn inc(&mut self) -> WasiThreadId {
        self.0 += 1;
        self.clone()
    }
}

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
impl Into<u32> for WasiBusProcessId {
    fn into(self) -> u32 {
        self.0 as u32
    }
}

/// The protected environment attributes that are set after the WasiEnv is initialized
#[derive(Clone)]
pub struct WasiEnvInner
{
    /// Represents a reference to the memory
    memory: Memory,
    /// Represents the reactors used to sleep and wake
    reactors: Reactors,
    /// Compiled bytes for the module so it can be recreated
    module_bytes: Bytes,
    /// Represents the callback for spawning a thread (name = "_start_thread")
    thread_spawn: Option<TypedFunction<i64, ()>>,
    /// Represents the callback for spawning a reactor (name = "_react")
    react: Option<TypedFunction<i64, ()>>,
    /// Represents the callback for destroying a local thread variable (name = "_thread_local_destroy")
    thread_local_destroy: Option<TypedFunction<(i64, i64), ()>>,
    /// Represents the callback for allocating memory (name = "_malloc")
    _malloc: Option<TypedFunction<i64, i64>>,
    /// Represents the callback for deallocating memory (name = "_free")
    _free: Option<TypedFunction<(i64, i64), ()>>,
}

/// The environment provided to the WASI imports.
#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct WasiEnv {
    /// ID of this thread (zero is the main thread)
    id: WasiThreadId,
    /// Shared state of the WASI system. Manages all the data that the
    /// executing WASI program can see.
    pub state: Arc<WasiState>,
    /// Inner functions and references that are loaded before the environment starts
    #[derivative(Debug = "ignore")]
    pub inner: Option<WasiEnvInner>,
    /// Implementation of the WASI runtime.
    pub(crate) runtime: Arc<dyn WasiRuntimeImplementation + Send + Sync + 'static>,
}

// Represents the current thread ID for the executing method
thread_local!(pub(crate) static THREAD_ID: RefCell<u32> = RefCell::new(0));

impl WasiEnv {
    pub fn new(state: WasiState) -> Self {
        let state = Arc::new(state);
        Self::new_ext(state)
    }

    fn new_ext(state: Arc<WasiState>) -> Self {
        let ret = Self {
            id: 0u32.into(),
            state,
            inner: None,
            runtime: Arc::new(PluggableRuntimeImplementation::default()),
        };
        ret
    }
    
    /// Returns a copy of the current runtime implementation for this environment
    pub fn runtime<'a>(&'a self) -> &'a (dyn WasiRuntimeImplementation) {
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
        THREAD_ID.with(|f| {
            let thread_id = f.borrow();
            *thread_id
        }).into()
    }

    /// Returns the number of active threads
    pub fn active_threads(&self) -> u32 {
        let guard = self.state.threading.read().unwrap();
        guard.active_threads()
    }

    // Yields execution
    pub fn yield_now(&self) -> Result<(), WasiError> {
        self.runtime.yield_now(self.current_thread_id())?;
        Ok(())
    }

    // Sleeps for a period of time
    pub fn sleep(&self, duration: Duration) -> Result<(), WasiError> {
        let duration = duration.as_nanos();
        let start = syscalls::platform_clock_time_get(__WASI_CLOCK_MONOTONIC, 1_000_000).unwrap() as u128;
        self.yield_now()?;
        loop {
            let now = syscalls::platform_clock_time_get(__WASI_CLOCK_MONOTONIC, 1_000_000).unwrap() as u128;
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
    pub fn net<'a>(&'a self) -> &'a (dyn VirtualNetworking) {
        self.runtime.networking()
    }

    /// Accesses the virtual bus implementation
    pub fn bus<'a>(&'a self) -> &'a (dyn VirtualBus) {
        self.runtime.bus()
    }

    /// Providers safe access to the initialized part of WasiEnv
    /// (it must be initialized before it can be used)
    pub fn inner(&self) -> &WasiEnvInner {
        self.inner.as_ref()
            .expect("You must initialize the WasiEnv before using it")
    }
    
    /// Providers safe access to the memory
    /// (it must be initialized before it can be used)
    pub fn memory_view<'a>(&'a self, store: &'a impl AsStoreRef) -> MemoryView<'a> {
        self.memory().view(store)
    }

    /// Providers safe access to the memory
    /// (it must be initialized before it can be used)
    pub fn memory(&self) -> &Memory {
        &self.inner().memory
    }

    /// Copy the lazy reference so that when it's initialized during the
    /// export phase, all the other references get a copy of it
    pub fn memory_clone(&self) -> Memory {
        self.memory().clone()
    }

    /// Get the WASI state
    pub fn state(&self) -> &WasiState {
        &self.state
    }
    
    pub(crate) fn get_memory_and_wasi_state<'a>(&'a self, store: &'a impl AsStoreRef, _mem_index: u32) -> (MemoryView<'a>, &WasiState) {
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

    /// Gets a reference to the WasiEnvironment
    pub fn data<'a>(&'a self, store: &'a impl AsStoreRef) -> &'a WasiEnv {
        self.env.as_ref(store)
    }

    /// Gets a mutable- reference to the host state in this context.
    /// (this will only return a mutable reference as long as the environment
    ///  has not been cloned - environments are cloned during multithreading)
    pub fn data_mut<'a>(&'a mut self, store: &'a mut impl AsStoreMut) -> &'a mut WasiEnv {
        self.env
            .as_mut(store)
            .expect("The WasiEnv can not be mutated after its been cloned")
    }

    /// Initializes the WasiEnv using the instance exports
    /// (this must be executed before attempting to use it)
    pub fn initialize(&mut self, store: &mut impl AsStoreMut, instance: &Instance) -> Result<(), ExportError> {
        let memory = instance.exports.get_memory("memory")?.clone();
        let new_inner = WasiEnvInner {
            reactors: Default::default(),
            module_bytes: Bytes::from(instance.module().serialize().unwrap()),
            memory,
            thread_spawn: instance.exports.get_typed_function(store, "_start_thread").ok(),
            react: instance.exports.get_typed_function(store, "_react").ok(),
            thread_local_destroy: instance.exports.get_typed_function(store, "_thread_local_destroy").ok(),
            _malloc: instance.exports.get_typed_function(store, "_malloc").ok(),
            _free: instance.exports.get_typed_function(store, "_free").ok()
        };

        let env = self.data_mut(store);
        env.inner.replace(new_inner);

        env.state.fs.is_wasix.store(
            is_wasix_module(instance.module()),
            std::sync::atomic::Ordering::Release,
        );

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

        Ok(resolver)
    }
}

/// Create an [`Imports`] with an existing [`WasiEnv`]. `WasiEnv`
/// needs a [`WasiState`], that can be constructed from a
/// [`WasiStateBuilder`](state::WasiStateBuilder).
pub fn generate_import_object_from_env(
    store: &mut impl AsStoreMut,
    ctx: &FunctionEnv<WasiEnv>,
    version: WasiVersion,
) -> Imports {
    match version {
        WasiVersion::Snapshot0 => generate_import_object_snapshot0(store, ctx),
        WasiVersion::Snapshot1 | WasiVersion::Latest => {
            generate_import_object_snapshot1(store, ctx)
        }
        WasiVersion::Wasix32v1 => generate_import_object_wasix32_v1(store, ctx),
        WasiVersion::Wasix64v1 => generate_import_object_wasix64_v1(store, ctx),
    }
}

fn wasi_unstable_exports(mut store: &mut impl AsStoreMut, ctx: &FunctionEnv<WasiEnv>) -> Exports {
    use syscalls::*;
    let namespace = namespace! {
        "args_get" => Function::new_native(&mut store, ctx, args_get::<Memory32>),
        "args_sizes_get" => Function::new_native(&mut store, ctx, args_sizes_get::<Memory32>),
        "clock_res_get" => Function::new_native(&mut store, ctx, clock_res_get::<Memory32>),
        "clock_time_get" => Function::new_native(&mut store, ctx, clock_time_get::<Memory32>),
        "environ_get" => Function::new_native(&mut store, ctx, environ_get::<Memory32>),
        "environ_sizes_get" => Function::new_native(&mut store, ctx, environ_sizes_get::<Memory32>),
        "fd_advise" => Function::new_native(&mut store, ctx, fd_advise),
        "fd_allocate" => Function::new_native(&mut store, ctx, fd_allocate),
        "fd_close" => Function::new_native(&mut store, ctx, fd_close),
        "fd_datasync" => Function::new_native(&mut store, ctx, fd_datasync),
        "fd_fdstat_get" => Function::new_native(&mut store, ctx, fd_fdstat_get::<Memory32>),
        "fd_fdstat_set_flags" => Function::new_native(&mut store, ctx, fd_fdstat_set_flags),
        "fd_fdstat_set_rights" => Function::new_native(&mut store, ctx, fd_fdstat_set_rights),
        "fd_filestat_get" => Function::new_native(&mut store, ctx, legacy::snapshot0::fd_filestat_get),
        "fd_filestat_set_size" => Function::new_native(&mut store, ctx, fd_filestat_set_size),
        "fd_filestat_set_times" => Function::new_native(&mut store, ctx, fd_filestat_set_times),
        "fd_pread" => Function::new_native(&mut store, ctx, fd_pread::<Memory32>),
        "fd_prestat_get" => Function::new_native(&mut store, ctx, fd_prestat_get::<Memory32>),
        "fd_prestat_dir_name" => Function::new_native(&mut store, ctx, fd_prestat_dir_name::<Memory32>),
        "fd_pwrite" => Function::new_native(&mut store, ctx, fd_pwrite::<Memory32>),
        "fd_read" => Function::new_native(&mut store, ctx, fd_read::<Memory32>),
        "fd_readdir" => Function::new_native(&mut store, ctx, fd_readdir::<Memory32>),
        "fd_renumber" => Function::new_native(&mut store, ctx, fd_renumber),
        "fd_seek" => Function::new_native(&mut store, ctx, legacy::snapshot0::fd_seek),
        "fd_sync" => Function::new_native(&mut store, ctx, fd_sync),
        "fd_tell" => Function::new_native(&mut store, ctx, fd_tell::<Memory32>),
        "fd_write" => Function::new_native(&mut store, ctx, fd_write::<Memory32>),
        "path_create_directory" => Function::new_native(&mut store, ctx, path_create_directory::<Memory32>),
        "path_filestat_get" => Function::new_native(&mut store, ctx, legacy::snapshot0::path_filestat_get),
        "path_filestat_set_times" => Function::new_native(&mut store, ctx, path_filestat_set_times::<Memory32>),
        "path_link" => Function::new_native(&mut store, ctx, path_link::<Memory32>),
        "path_open" => Function::new_native(&mut store, ctx, path_open::<Memory32>),
        "path_readlink" => Function::new_native(&mut store, ctx, path_readlink::<Memory32>),
        "path_remove_directory" => Function::new_native(&mut store, ctx, path_remove_directory::<Memory32>),
        "path_rename" => Function::new_native(&mut store, ctx, path_rename::<Memory32>),
        "path_symlink" => Function::new_native(&mut store, ctx, path_symlink::<Memory32>),
        "path_unlink_file" => Function::new_native(&mut store, ctx, path_unlink_file::<Memory32>),
        "poll_oneoff" => Function::new_native(&mut store, ctx, legacy::snapshot0::poll_oneoff),
        "proc_exit" => Function::new_native(&mut store, ctx, proc_exit),
        "proc_raise" => Function::new_native(&mut store, ctx, proc_raise),
        "random_get" => Function::new_native(&mut store, ctx, random_get::<Memory32>),
        "sched_yield" => Function::new_native(&mut store, ctx, sched_yield),
        "sock_recv" => Function::new_native(&mut store, ctx, sock_recv::<Memory32>),
        "sock_send" => Function::new_native(&mut store, ctx, sock_send::<Memory32>),
        "sock_shutdown" => Function::new_native(&mut store, ctx, sock_shutdown),
    };
    namespace
}

fn wasi_snapshot_preview1_exports(
    mut store: &mut impl AsStoreMut,
    ctx: &FunctionEnv<WasiEnv>,
) -> Exports {
    use syscalls::*;
    let namespace = namespace! {
        "args_get" => Function::new_native(&mut store, ctx, args_get::<Memory32>),
        "args_sizes_get" => Function::new_native(&mut store, ctx, args_sizes_get::<Memory32>),
        "clock_res_get" => Function::new_native(&mut store, ctx, clock_res_get::<Memory32>),
        "clock_time_get" => Function::new_native(&mut store, ctx, clock_time_get::<Memory32>),
        "environ_get" => Function::new_native(&mut store, ctx, environ_get::<Memory32>),
        "environ_sizes_get" => Function::new_native(&mut store, ctx, environ_sizes_get::<Memory32>),
        "fd_advise" => Function::new_native(&mut store, ctx, fd_advise),
        "fd_allocate" => Function::new_native(&mut store, ctx, fd_allocate),
        "fd_close" => Function::new_native(&mut store, ctx, fd_close),
        "fd_datasync" => Function::new_native(&mut store, ctx, fd_datasync),
        "fd_fdstat_get" => Function::new_native(&mut store, ctx, fd_fdstat_get::<Memory32>),
        "fd_fdstat_set_flags" => Function::new_native(&mut store, ctx, fd_fdstat_set_flags),
        "fd_fdstat_set_rights" => Function::new_native(&mut store, ctx, fd_fdstat_set_rights),
        "fd_filestat_get" => Function::new_native(&mut store, ctx, fd_filestat_get::<Memory32>),
        "fd_filestat_set_size" => Function::new_native(&mut store, ctx, fd_filestat_set_size),
        "fd_filestat_set_times" => Function::new_native(&mut store, ctx, fd_filestat_set_times),
        "fd_pread" => Function::new_native(&mut store, ctx, fd_pread::<Memory32>),
        "fd_prestat_get" => Function::new_native(&mut store, ctx, fd_prestat_get::<Memory32>),
        "fd_prestat_dir_name" => Function::new_native(&mut store, ctx, fd_prestat_dir_name::<Memory32>),
        "fd_pwrite" => Function::new_native(&mut store, ctx, fd_pwrite::<Memory32>),
        "fd_read" => Function::new_native(&mut store, ctx, fd_read::<Memory32>),
        "fd_readdir" => Function::new_native(&mut store, ctx, fd_readdir::<Memory32>),
        "fd_renumber" => Function::new_native(&mut store, ctx, fd_renumber),
        "fd_seek" => Function::new_native(&mut store, ctx, fd_seek::<Memory32>),
        "fd_sync" => Function::new_native(&mut store, ctx, fd_sync),
        "fd_tell" => Function::new_native(&mut store, ctx, fd_tell::<Memory32>),
        "fd_write" => Function::new_native(&mut store, ctx, fd_write::<Memory32>),
        "path_create_directory" => Function::new_native(&mut store, ctx, path_create_directory::<Memory32>),
        "path_filestat_get" => Function::new_native(&mut store, ctx, path_filestat_get::<Memory32>),
        "path_filestat_set_times" => Function::new_native(&mut store, ctx, path_filestat_set_times::<Memory32>),
        "path_link" => Function::new_native(&mut store, ctx, path_link::<Memory32>),
        "path_open" => Function::new_native(&mut store, ctx, path_open::<Memory32>),
        "path_readlink" => Function::new_native(&mut store, ctx, path_readlink::<Memory32>),
        "path_remove_directory" => Function::new_native(&mut store, ctx, path_remove_directory::<Memory32>),
        "path_rename" => Function::new_native(&mut store, ctx, path_rename::<Memory32>),
        "path_symlink" => Function::new_native(&mut store, ctx, path_symlink::<Memory32>),
        "path_unlink_file" => Function::new_native(&mut store, ctx, path_unlink_file::<Memory32>),
        "poll_oneoff" => Function::new_native(&mut store, ctx, poll_oneoff::<Memory32>),
        "proc_exit" => Function::new_native(&mut store, ctx, proc_exit),
        "proc_raise" => Function::new_native(&mut store, ctx, proc_raise),
        "random_get" => Function::new_native(&mut store, ctx, random_get::<Memory32>),
        "sched_yield" => Function::new_native(&mut store, ctx, sched_yield),
        "sock_recv" => Function::new_native(&mut store, ctx, sock_recv::<Memory32>),
        "sock_send" => Function::new_native(&mut store, ctx, sock_send::<Memory32>),
        "sock_shutdown" => Function::new_native(&mut store, ctx, sock_shutdown),
    };
    namespace
}

fn wasix_exports_32(
    mut store: &mut impl AsStoreMut,
    ctx: &FunctionEnv<WasiEnv>,
) -> Exports
{
    use syscalls::*;
    let namespace = namespace! {
        "args_get" => Function::new_native(&mut store, ctx, args_get::<Memory32>),
        "args_sizes_get" => Function::new_native(&mut store, ctx, args_sizes_get::<Memory32>),
        "clock_res_get" => Function::new_native(&mut store, ctx, clock_res_get::<Memory32>),
        "clock_time_get" => Function::new_native(&mut store, ctx, clock_time_get::<Memory32>),
        "environ_get" => Function::new_native(&mut store, ctx, environ_get::<Memory32>),
        "environ_sizes_get" => Function::new_native(&mut store, ctx, environ_sizes_get::<Memory32>),
        "fd_advise" => Function::new_native(&mut store, ctx, fd_advise),
        "fd_allocate" => Function::new_native(&mut store, ctx, fd_allocate),
        "fd_close" => Function::new_native(&mut store, ctx, fd_close),
        "fd_datasync" => Function::new_native(&mut store, ctx, fd_datasync),
        "fd_fdstat_get" => Function::new_native(&mut store, ctx, fd_fdstat_get::<Memory32>),
        "fd_fdstat_set_flags" => Function::new_native(&mut store, ctx, fd_fdstat_set_flags),
        "fd_fdstat_set_rights" => Function::new_native(&mut store, ctx, fd_fdstat_set_rights),
        "fd_filestat_get" => Function::new_native(&mut store, ctx, fd_filestat_get::<Memory32>),
        "fd_filestat_set_size" => Function::new_native(&mut store, ctx, fd_filestat_set_size),
        "fd_filestat_set_times" => Function::new_native(&mut store, ctx, fd_filestat_set_times),
        "fd_pread" => Function::new_native(&mut store, ctx, fd_pread::<Memory32>),
        "fd_prestat_get" => Function::new_native(&mut store, ctx, fd_prestat_get::<Memory32>),
        "fd_prestat_dir_name" => Function::new_native(&mut store, ctx, fd_prestat_dir_name::<Memory32>),
        "fd_pwrite" => Function::new_native(&mut store, ctx, fd_pwrite::<Memory32>),
        "fd_read" => Function::new_native(&mut store, ctx, fd_read::<Memory32>),
        "fd_readdir" => Function::new_native(&mut store, ctx, fd_readdir::<Memory32>),
        "fd_renumber" => Function::new_native(&mut store, ctx, fd_renumber),
        "fd_dup" => Function::new_native(&mut store, ctx, fd_dup::<Memory32>),
        "fd_event" => Function::new_native(&mut store, ctx, fd_event::<Memory32>),
        "fd_seek" => Function::new_native(&mut store, ctx, fd_seek::<Memory32>),
        "fd_sync" => Function::new_native(&mut store, ctx, fd_sync),
        "fd_tell" => Function::new_native(&mut store, ctx, fd_tell::<Memory32>),
        "fd_write" => Function::new_native(&mut store, ctx, fd_write::<Memory32>),
        "fd_pipe" => Function::new_native(&mut store, ctx, fd_pipe::<Memory32>),
        "path_create_directory" => Function::new_native(&mut store, ctx, path_create_directory::<Memory32>),
        "path_filestat_get" => Function::new_native(&mut store, ctx, path_filestat_get::<Memory32>),
        "path_filestat_set_times" => Function::new_native(&mut store, ctx, path_filestat_set_times::<Memory32>),
        "path_link" => Function::new_native(&mut store, ctx, path_link::<Memory32>),
        "path_open" => Function::new_native(&mut store, ctx, path_open::<Memory32>),
        "path_readlink" => Function::new_native(&mut store, ctx, path_readlink::<Memory32>),
        "path_remove_directory" => Function::new_native(&mut store, ctx, path_remove_directory::<Memory32>),
        "path_rename" => Function::new_native(&mut store, ctx, path_rename::<Memory32>),
        "path_symlink" => Function::new_native(&mut store, ctx, path_symlink::<Memory32>),
        "path_unlink_file" => Function::new_native(&mut store, ctx, path_unlink_file::<Memory32>),
        "poll_oneoff" => Function::new_native(&mut store, ctx, poll_oneoff::<Memory32>),
        "proc_exit" => Function::new_native(&mut store, ctx, proc_exit),
        "proc_raise" => Function::new_native(&mut store, ctx, proc_raise),
        "random_get" => Function::new_native(&mut store, ctx, random_get::<Memory32>),
        "tty_get" => Function::new_native(&mut store, ctx, tty_get::<Memory32>),
        "tty_set" => Function::new_native(&mut store, ctx, tty_set::<Memory32>),
        "getcwd" => Function::new_native(&mut store, ctx, getcwd::<Memory32>),
        "chdir" => Function::new_native(&mut store, ctx, chdir::<Memory32>),
        "thread_spawn" => Function::new_native(&mut store, ctx, thread_spawn::<Memory32>),
        "thread_local_create" => Function::new_native(&mut store, ctx, thread_local_create::<Memory32>),
        "thread_local_destroy" => Function::new_native(&mut store, ctx, thread_local_destroy),
        "thread_local_set" => Function::new_native(&mut store, ctx, thread_local_set),
        "thread_local_get" => Function::new_native(&mut store, ctx, thread_local_get::<Memory32>),
        "thread_sleep" => Function::new_native(&mut store, ctx, thread_sleep),
        "thread_id" => Function::new_native(&mut store, ctx, thread_id::<Memory32>),
        "thread_join" => Function::new_native(&mut store, ctx, thread_join),
        "thread_parallelism" => Function::new_native(&mut store, ctx, thread_parallelism::<Memory32>),
        "thread_exit" => Function::new_native(&mut store, ctx, thread_exit),
        "sched_yield" => Function::new_native(&mut store, ctx, sched_yield),
        "futex_wait" => Function::new_native(&mut store, ctx, futex_wait::<Memory32>),
        "futex_wake" => Function::new_native(&mut store, ctx, futex_wake::<Memory32>),
        "futex_wake_all" => Function::new_native(&mut store, ctx, futex_wake_all::<Memory32>),
        "getpid" => Function::new_native(&mut store, ctx, getpid::<Memory32>),
        "process_spawn" => Function::new_native(&mut store, ctx, process_spawn::<Memory32>),
        "bus_open_local" => Function::new_native(&mut store, ctx, bus_open_local::<Memory32>),
        "bus_open_remote" => Function::new_native(&mut store, ctx, bus_open_remote::<Memory32>),
        "bus_close" => Function::new_native(&mut store, ctx, bus_close),
        "bus_call" => Function::new_native(&mut store, ctx, bus_call::<Memory32>),
        "bus_subcall" => Function::new_native(&mut store, ctx, bus_subcall::<Memory32>),
        "bus_poll" => Function::new_native(&mut store, ctx, bus_poll::<Memory32>),
        "call_reply" => Function::new_native(&mut store, ctx, call_reply::<Memory32>),
        "call_fault" => Function::new_native(&mut store, ctx, call_fault),
        "call_close" => Function::new_native(&mut store, ctx, call_close),
        "ws_connect" => Function::new_native(&mut store, ctx, ws_connect::<Memory32>),
        "http_request" => Function::new_native(&mut store, ctx, http_request::<Memory32>),
        "http_status" => Function::new_native(&mut store, ctx, http_status::<Memory32>),
        "port_bridge" => Function::new_native(&mut store, ctx, port_bridge::<Memory32>),
        "port_unbridge" => Function::new_native(&mut store, ctx, port_unbridge),
        "port_dhcp_acquire" => Function::new_native(&mut store, ctx, port_dhcp_acquire),
        "port_addr_add" => Function::new_native(&mut store, ctx, port_addr_add::<Memory32>),
        "port_addr_remove" => Function::new_native(&mut store, ctx, port_addr_remove::<Memory32>),
        "port_addr_clear" => Function::new_native(&mut store, ctx, port_addr_clear),
        "port_addr_list" => Function::new_native(&mut store, ctx, port_addr_list::<Memory32>),
        "port_mac" => Function::new_native(&mut store, ctx, port_mac::<Memory32>),
        "port_gateway_set" => Function::new_native(&mut store, ctx, port_gateway_set::<Memory32>),
        "port_route_add" => Function::new_native(&mut store, ctx, port_route_add::<Memory32>),
        "port_route_remove" => Function::new_native(&mut store, ctx, port_route_remove::<Memory32>),
        "port_route_clear" => Function::new_native(&mut store, ctx, port_route_clear),
        "port_route_list" => Function::new_native(&mut store, ctx, port_route_list::<Memory32>),
        "sock_status" => Function::new_native(&mut store, ctx, sock_status::<Memory32>),
        "sock_addr_local" => Function::new_native(&mut store, ctx, sock_addr_local::<Memory32>),
        "sock_addr_peer" => Function::new_native(&mut store, ctx, sock_addr_peer::<Memory32>),
        "sock_open" => Function::new_native(&mut store, ctx, sock_open::<Memory32>),
        "sock_set_opt_flag" => Function::new_native(&mut store, ctx, sock_set_opt_flag),
        "sock_get_opt_flag" => Function::new_native(&mut store, ctx, sock_get_opt_flag::<Memory32>),
        "sock_set_opt_time" => Function::new_native(&mut store, ctx, sock_set_opt_time::<Memory32>),
        "sock_get_opt_time" => Function::new_native(&mut store, ctx, sock_get_opt_time::<Memory32>),
        "sock_set_opt_size" => Function::new_native(&mut store, ctx, sock_set_opt_size),
        "sock_get_opt_size" => Function::new_native(&mut store, ctx, sock_get_opt_size::<Memory32>),
        "sock_join_multicast_v4" => Function::new_native(&mut store, ctx, sock_join_multicast_v4::<Memory32>),
        "sock_leave_multicast_v4" => Function::new_native(&mut store, ctx, sock_leave_multicast_v4::<Memory32>),
        "sock_join_multicast_v6" => Function::new_native(&mut store, ctx, sock_join_multicast_v6::<Memory32>),
        "sock_leave_multicast_v6" => Function::new_native(&mut store, ctx, sock_leave_multicast_v6::<Memory32>),
        "sock_bind" => Function::new_native(&mut store, ctx, sock_bind::<Memory32>),
        "sock_listen" => Function::new_native(&mut store, ctx, sock_listen::<Memory32>),
        "sock_accept" => Function::new_native(&mut store, ctx, sock_accept::<Memory32>),
        "sock_connect" => Function::new_native(&mut store, ctx, sock_connect::<Memory32>),
        "sock_recv" => Function::new_native(&mut store, ctx, sock_recv::<Memory32>),
        "sock_recv_from" => Function::new_native(&mut store, ctx, sock_recv_from::<Memory32>),
        "sock_send" => Function::new_native(&mut store, ctx, sock_send::<Memory32>),
        "sock_send_to" => Function::new_native(&mut store, ctx, sock_send_to::<Memory32>),
        "sock_send_file" => Function::new_native(&mut store, ctx, sock_send_file::<Memory32>),
        "sock_shutdown" => Function::new_native(&mut store, ctx, sock_shutdown),
        "resolve" => Function::new_native(&mut store, ctx, resolve::<Memory32>),
    };
    namespace
}

fn wasix_exports_64(
    mut store: &mut impl AsStoreMut,
    ctx: &FunctionEnv<WasiEnv>,
) -> Exports
{
    use syscalls::*;
    let namespace = namespace! {
        "args_get" => Function::new_native(&mut store, ctx, args_get::<Memory64>),
        "args_sizes_get" => Function::new_native(&mut store, ctx, args_sizes_get::<Memory64>),
        "clock_res_get" => Function::new_native(&mut store, ctx, clock_res_get::<Memory64>),
        "clock_time_get" => Function::new_native(&mut store, ctx, clock_time_get::<Memory64>),
        "environ_get" => Function::new_native(&mut store, ctx, environ_get::<Memory64>),
        "environ_sizes_get" => Function::new_native(&mut store, ctx, environ_sizes_get::<Memory64>),
        "fd_advise" => Function::new_native(&mut store, ctx, fd_advise),
        "fd_allocate" => Function::new_native(&mut store, ctx, fd_allocate),
        "fd_close" => Function::new_native(&mut store, ctx, fd_close),
        "fd_datasync" => Function::new_native(&mut store, ctx, fd_datasync),
        "fd_fdstat_get" => Function::new_native(&mut store, ctx, fd_fdstat_get::<Memory64>),
        "fd_fdstat_set_flags" => Function::new_native(&mut store, ctx, fd_fdstat_set_flags),
        "fd_fdstat_set_rights" => Function::new_native(&mut store, ctx, fd_fdstat_set_rights),
        "fd_filestat_get" => Function::new_native(&mut store, ctx, fd_filestat_get::<Memory64>),
        "fd_filestat_set_size" => Function::new_native(&mut store, ctx, fd_filestat_set_size),
        "fd_filestat_set_times" => Function::new_native(&mut store, ctx, fd_filestat_set_times),
        "fd_pread" => Function::new_native(&mut store, ctx, fd_pread::<Memory64>),
        "fd_prestat_get" => Function::new_native(&mut store, ctx, fd_prestat_get::<Memory64>),
        "fd_prestat_dir_name" => Function::new_native(&mut store, ctx, fd_prestat_dir_name::<Memory64>),
        "fd_pwrite" => Function::new_native(&mut store, ctx, fd_pwrite::<Memory64>),
        "fd_read" => Function::new_native(&mut store, ctx, fd_read::<Memory64>),
        "fd_readdir" => Function::new_native(&mut store, ctx, fd_readdir::<Memory64>),
        "fd_renumber" => Function::new_native(&mut store, ctx, fd_renumber),
        "fd_dup" => Function::new_native(&mut store, ctx, fd_dup::<Memory64>),
        "fd_event" => Function::new_native(&mut store, ctx, fd_event::<Memory64>),
        "fd_seek" => Function::new_native(&mut store, ctx, fd_seek::<Memory64>),
        "fd_sync" => Function::new_native(&mut store, ctx, fd_sync),
        "fd_tell" => Function::new_native(&mut store, ctx, fd_tell::<Memory64>),
        "fd_write" => Function::new_native(&mut store, ctx, fd_write::<Memory64>),
        "fd_pipe" => Function::new_native(&mut store, ctx, fd_pipe::<Memory64>),
        "path_create_directory" => Function::new_native(&mut store, ctx, path_create_directory::<Memory64>),
        "path_filestat_get" => Function::new_native(&mut store, ctx, path_filestat_get::<Memory64>),
        "path_filestat_set_times" => Function::new_native(&mut store, ctx, path_filestat_set_times::<Memory64>),
        "path_link" => Function::new_native(&mut store, ctx, path_link::<Memory64>),
        "path_open" => Function::new_native(&mut store, ctx, path_open::<Memory64>),
        "path_readlink" => Function::new_native(&mut store, ctx, path_readlink::<Memory64>),
        "path_remove_directory" => Function::new_native(&mut store, ctx, path_remove_directory::<Memory64>),
        "path_rename" => Function::new_native(&mut store, ctx, path_rename::<Memory64>),
        "path_symlink" => Function::new_native(&mut store, ctx, path_symlink::<Memory64>),
        "path_unlink_file" => Function::new_native(&mut store, ctx, path_unlink_file::<Memory64>),
        "poll_oneoff" => Function::new_native(&mut store, ctx, poll_oneoff::<Memory64>),
        "proc_exit" => Function::new_native(&mut store, ctx, proc_exit),
        "proc_raise" => Function::new_native(&mut store, ctx, proc_raise),
        "random_get" => Function::new_native(&mut store, ctx, random_get::<Memory64>),
        "tty_get" => Function::new_native(&mut store, ctx, tty_get::<Memory64>),
        "tty_set" => Function::new_native(&mut store, ctx, tty_set::<Memory64>),
        "getcwd" => Function::new_native(&mut store, ctx, getcwd::<Memory64>),
        "chdir" => Function::new_native(&mut store, ctx, chdir::<Memory64>),
        "thread_spawn" => Function::new_native(&mut store, ctx, thread_spawn::<Memory64>),
        "thread_local_create" => Function::new_native(&mut store, ctx, thread_local_create::<Memory64>),
        "thread_local_destroy" => Function::new_native(&mut store, ctx, thread_local_destroy),
        "thread_local_set" => Function::new_native(&mut store, ctx, thread_local_set),
        "thread_local_get" => Function::new_native(&mut store, ctx, thread_local_get::<Memory64>),
        "thread_sleep" => Function::new_native(&mut store, ctx, thread_sleep),
        "thread_id" => Function::new_native(&mut store, ctx, thread_id::<Memory64>),
        "thread_join" => Function::new_native(&mut store, ctx, thread_join),
        "thread_parallelism" => Function::new_native(&mut store, ctx, thread_parallelism::<Memory64>),
        "thread_exit" => Function::new_native(&mut store, ctx, thread_exit),
        "sched_yield" => Function::new_native(&mut store, ctx, sched_yield),
        "futex_wait" => Function::new_native(&mut store, ctx, futex_wait::<Memory64>),
        "futex_wake" => Function::new_native(&mut store, ctx, futex_wake::<Memory64>),
        "futex_wake_all" => Function::new_native(&mut store, ctx, futex_wake_all::<Memory64>),
        "getpid" => Function::new_native(&mut store, ctx, getpid::<Memory64>),
        "process_spawn" => Function::new_native(&mut store, ctx, process_spawn::<Memory64>),
        "bus_open_local" => Function::new_native(&mut store, ctx, bus_open_local::<Memory64>),
        "bus_open_remote" => Function::new_native(&mut store, ctx, bus_open_remote::<Memory64>),
        "bus_close" => Function::new_native(&mut store, ctx, bus_close),
        "bus_call" => Function::new_native(&mut store, ctx, bus_call::<Memory64>),
        "bus_subcall" => Function::new_native(&mut store, ctx, bus_subcall::<Memory64>),
        "bus_poll" => Function::new_native(&mut store, ctx, bus_poll::<Memory64>),
        "call_reply" => Function::new_native(&mut store, ctx, call_reply::<Memory64>),
        "call_fault" => Function::new_native(&mut store, ctx, call_fault),
        "call_close" => Function::new_native(&mut store, ctx, call_close),
        "ws_connect" => Function::new_native(&mut store, ctx, ws_connect::<Memory64>),
        "http_request" => Function::new_native(&mut store, ctx, http_request::<Memory64>),
        "http_status" => Function::new_native(&mut store, ctx, http_status::<Memory64>),
        "port_bridge" => Function::new_native(&mut store, ctx, port_bridge::<Memory64>),
        "port_unbridge" => Function::new_native(&mut store, ctx, port_unbridge),
        "port_dhcp_acquire" => Function::new_native(&mut store, ctx, port_dhcp_acquire),
        "port_addr_add" => Function::new_native(&mut store, ctx, port_addr_add::<Memory64>),
        "port_addr_remove" => Function::new_native(&mut store, ctx, port_addr_remove::<Memory64>),
        "port_addr_clear" => Function::new_native(&mut store, ctx, port_addr_clear),
        "port_addr_list" => Function::new_native(&mut store, ctx, port_addr_list::<Memory64>),
        "port_mac" => Function::new_native(&mut store, ctx, port_mac::<Memory64>),
        "port_gateway_set" => Function::new_native(&mut store, ctx, port_gateway_set::<Memory64>),
        "port_route_add" => Function::new_native(&mut store, ctx, port_route_add::<Memory64>),
        "port_route_remove" => Function::new_native(&mut store, ctx, port_route_remove::<Memory64>),
        "port_route_clear" => Function::new_native(&mut store, ctx, port_route_clear),
        "port_route_list" => Function::new_native(&mut store, ctx, port_route_list::<Memory64>),
        "sock_status" => Function::new_native(&mut store, ctx, sock_status::<Memory64>),
        "sock_addr_local" => Function::new_native(&mut store, ctx, sock_addr_local::<Memory64>),
        "sock_addr_peer" => Function::new_native(&mut store, ctx, sock_addr_peer::<Memory64>),
        "sock_open" => Function::new_native(&mut store, ctx, sock_open::<Memory64>),
        "sock_set_opt_flag" => Function::new_native(&mut store, ctx, sock_set_opt_flag),
        "sock_get_opt_flag" => Function::new_native(&mut store, ctx, sock_get_opt_flag::<Memory64>),
        "sock_set_opt_time" => Function::new_native(&mut store, ctx, sock_set_opt_time::<Memory64>),
        "sock_get_opt_time" => Function::new_native(&mut store, ctx, sock_get_opt_time::<Memory64>),
        "sock_set_opt_size" => Function::new_native(&mut store, ctx, sock_set_opt_size),
        "sock_get_opt_size" => Function::new_native(&mut store, ctx, sock_get_opt_size::<Memory64>),
        "sock_join_multicast_v4" => Function::new_native(&mut store, ctx, sock_join_multicast_v4::<Memory64>),
        "sock_leave_multicast_v4" => Function::new_native(&mut store, ctx, sock_leave_multicast_v4::<Memory64>),
        "sock_join_multicast_v6" => Function::new_native(&mut store, ctx, sock_join_multicast_v6::<Memory64>),
        "sock_leave_multicast_v6" => Function::new_native(&mut store, ctx, sock_leave_multicast_v6::<Memory64>),
        "sock_bind" => Function::new_native(&mut store, ctx, sock_bind::<Memory64>),
        "sock_listen" => Function::new_native(&mut store, ctx, sock_listen::<Memory64>),
        "sock_accept" => Function::new_native(&mut store, ctx, sock_accept::<Memory64>),
        "sock_connect" => Function::new_native(&mut store, ctx, sock_connect::<Memory64>),
        "sock_recv" => Function::new_native(&mut store, ctx, sock_recv::<Memory64>),
        "sock_recv_from" => Function::new_native(&mut store, ctx, sock_recv_from::<Memory64>),
        "sock_send" => Function::new_native(&mut store, ctx, sock_send::<Memory64>),
        "sock_send_to" => Function::new_native(&mut store, ctx, sock_send_to::<Memory64>),
        "sock_send_file" => Function::new_native(&mut store, ctx, sock_send_file::<Memory64>),
        "sock_shutdown" => Function::new_native(&mut store, ctx, sock_shutdown),
        "resolve" => Function::new_native(&mut store, ctx, resolve::<Memory64>),
    };
    namespace
}

pub fn import_object_for_all_wasi_versions(
    store: &mut impl AsStoreMut,
    ctx: &FunctionEnv<WasiEnv>,
) -> Imports {
    let exports_wasi_unstable = wasi_unstable_exports(store, ctx);
    let exports_wasi_snapshot_preview1 = wasi_snapshot_preview1_exports(store, ctx);
    let exports_wasix_32v1 = wasix_exports_32(store, ctx);
    let exports_wasix_64v1 = wasix_exports_64(store, ctx);
    imports! {
        "wasi_unstable" => exports_wasi_unstable,
        "wasi_snapshot_preview1" => exports_wasi_snapshot_preview1,
        "wasix_32v1" => exports_wasix_32v1,
        "wasix_64v1" => exports_wasix_64v1,
    }
}

/// Combines a state generating function with the import list for legacy WASI
fn generate_import_object_snapshot0(
    store: &mut impl AsStoreMut,
    ctx: &FunctionEnv<WasiEnv>,
) -> Imports {
    let exports_unstable = wasi_unstable_exports(store, ctx);
    imports! {
        "wasi_unstable" => exports_unstable
    }
}

fn generate_import_object_snapshot1(
    store: &mut impl AsStoreMut,
    ctx: &FunctionEnv<WasiEnv>,
) -> Imports {
    let exports_wasi_snapshot_preview1 = wasi_snapshot_preview1_exports(store, ctx);
    imports! {
        "wasi_snapshot_preview1" => exports_wasi_snapshot_preview1
    }
}

/// Combines a state generating function with the import list for snapshot 1
fn generate_import_object_wasix32_v1(
    store: &mut impl AsStoreMut,
    ctx: &FunctionEnv<WasiEnv>,
) -> Imports {
    let exports_wasix_32v1 = wasix_exports_32(store, ctx);
    imports! {
        "wasix_32v1" => exports_wasix_32v1
    }
}

fn generate_import_object_wasix64_v1(
    store: &mut impl AsStoreMut,
    ctx: &FunctionEnv<WasiEnv>,
) -> Imports {
    let exports_wasix_64v1 = wasix_exports_64(store, ctx);
    imports! {
        "wasix_64v1" => exports_wasix_64v1
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
