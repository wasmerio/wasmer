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
mod state;
mod syscalls;
mod utils;

use crate::syscalls::*;

pub use crate::state::{
    Fd, Pipe, Stderr, Stdin, Stdout, WasiFs, WasiState, WasiStateBuilder, WasiStateCreationError,
    ALL_RIGHTS, VIRTUAL_ROOT_FD,
};
pub use crate::syscalls::types;
pub use crate::utils::{get_wasi_version, get_wasi_versions, is_wasi_module, WasiVersion};
use derivative::*;
use std::ops::Deref;
#[deprecated(since = "2.1.0", note = "Please use `wasmer_vfs::FsError`")]
pub use wasmer_vfs::FsError as WasiFsError;
#[deprecated(since = "2.1.0", note = "Please use `wasmer_vfs::VirtualFile`")]
pub use wasmer_vfs::VirtualFile as WasiFile;
pub use wasmer_vfs::{FsError, VirtualFile};

use thiserror::Error;
use wasmer::{
    imports, Function, Imports, LazyInit, Memory, MemoryAccessError, Module, Store, WasmerEnv,
};

use std::time::{Instant, Duration};
use std::sync::{atomic::AtomicU32, atomic::Ordering, Arc, Mutex, MutexGuard};

/// This is returned in `RuntimeError`.
/// Use `downcast` or `downcast_ref` to retrieve the `ExitCode`.
#[derive(Error, Debug)]
pub enum WasiError {
    #[error("WASI exited with code: {0}")]
    Exit(syscalls::types::__wasi_exitcode_t),
    #[error("The WASI version could not be determined")]
    UnknownWasiVersion,
}

/// The environment provided to the WASI imports.
#[derive(Derivative)]
#[derivative(Debug)]
#[derive(Clone, WasmerEnv)]
pub struct WasiThread {
    /// ID of this thread
    id: u32,
    /// Provides access to the WASI environment
    env: WasiEnv,
    #[wasmer(export)]
    memory: LazyInit<Memory>,
}

/// The WASI thread dereferences into the WASI environment
impl Deref for WasiThread {
    type Target = WasiEnv;

    fn deref(&self) -> &WasiEnv {
        &self.env
    }
}

impl WasiThread {
    /// Returns the unique ID of this thread
    pub fn thread_id(&self) -> u32 {
        self.id
    }

    /// Yields execution
    pub fn yield_now(&self) {
        if let Some(callback) = self.on_yield.as_ref() {
            callback(self);
        }
        std::thread::yield_now();
    }

    /// Sleeps for a period of time
    pub fn sleep(&self, duration: Duration) {
        let start = Instant::now();
        self.yield_now();
        loop {
            let delta = match Instant::now().checked_duration_since(start) {
                Some(a) => a,
                None => { break; }
            };            
            if delta > duration {
                break;
            }
            let remaining = match duration.checked_sub(delta) {
                Some(a) => a.min(Duration::from_millis(10)),
                None => { break; }
            };
            self.yield_now();
            std::thread::sleep(remaining);
        }
    }

    /// Get an `Imports` for a specific version of WASI detected in the module.
    pub fn import_object(&mut self, module: &Module) -> Result<Imports, WasiError> {
        let wasi_version = get_wasi_version(module, false).ok_or(WasiError::UnknownWasiVersion)?;
        Ok(generate_import_object_from_thread(
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
                generate_import_object_from_thread(module.store(), self.clone(), *version);
            for ((n, m), e) in new_import_object.into_iter() {
                resolver.define(&n, &m, e);
            }
        }
        Ok(resolver)
    }

    /// Get a reference to the memory
    pub fn memory(&self) -> &Memory {
        self.memory_ref()
            .expect("Memory should be set on `WasiThread` first")
    }

    // Copy the lazy reference so that when its initialized during the
    // export phase that all the other references get a copy of it
    pub fn memory_clone(&self) -> LazyInit<Memory> {
        self.memory.clone()
    }

    pub(crate) fn get_memory_and_wasi_state(
        &self,
        _mem_index: u32,
    ) -> (&Memory, MutexGuard<WasiState>) {
        let memory = self.memory();
        let state = self.state.lock().unwrap();
        (memory, state)
    }
}

/// The environment provided to the WASI imports.
#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct WasiEnv {
    /// Represents a reference to the memory
    memory: LazyInit<Memory>,
    /// Shared state of the WASI system. Manages all the data that the
    /// executing WASI program can see.
    ///
    /// Be careful when using this in host functions that call into Wasm:
    /// if the lock is held and the Wasm calls into a host function that tries
    /// to lock this mutex, the program will deadlock.
    pub state: Arc<Mutex<WasiState>>,
    /// Optional callback thats invoked whenever a syscall is made
    /// which is used to make callbacks to the process without breaking
    /// the single threaded WASM modules
    #[derivative(Debug = "ignore")]
    pub(crate) on_yield: Option<Arc<dyn Fn(&WasiThread) + Send + Sync + 'static>>,
    /// The thread ID seed is used to generate unique thread identifiers
    /// for each WasiThread. These are needed for multithreading code that needs
    /// this information in the syscalls
    pub(crate) thread_id_seed: Arc<AtomicU32>,
}

impl WasiEnv {
    pub fn new(state: WasiState) -> Self {
        Self {
            memory: LazyInit::new(),
            state: Arc::new(Mutex::new(state)),
            on_yield: None,
            thread_id_seed: Arc::new(AtomicU32::new(1u32)),
        }
    }

    /// Creates a new thread only this wasi environment
    pub fn new_thread(&self) -> WasiThread {
        WasiThread {
            id: self.thread_id_seed.fetch_add(1, Ordering::Relaxed),
            env: self.clone(),
            memory: self.memory_clone(),
        }
    }

    /// Get the WASI state
    ///
    /// Be careful when using this in host functions that call into Wasm:
    /// if the lock is held and the Wasm calls into a host function that tries
    /// to lock this mutex, the program will deadlock.
    pub fn state(&self) -> MutexGuard<WasiState> {
        self.state.lock().unwrap()
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
}

/// Create an [`Imports`] with an existing [`WasiEnv`]. `WasiEnv`
/// needs a [`WasiState`], that can be constructed from a
/// [`WasiStateBuilder`](state::WasiStateBuilder).
pub fn generate_import_object_from_thread(
    store: &Store,
    thread: WasiThread,
    version: WasiVersion,
) -> Imports {
    match version {
        WasiVersion::Snapshot0 => generate_import_object_snapshot0(store, thread),
        WasiVersion::Snapshot1 | WasiVersion::Latest => {
            generate_import_object_snapshot1(store, thread)
        }
    }
}

/// Combines a state generating function with the import list for legacy WASI
fn generate_import_object_snapshot0(store: &Store, thread: WasiThread) -> Imports {
    imports! {
        "wasi_unstable" => {
            "args_get" => Function::new_native_with_env(store, thread.clone(), args_get),
            "args_sizes_get" => Function::new_native_with_env(store, thread.clone(), args_sizes_get),
            "clock_res_get" => Function::new_native_with_env(store, thread.clone(), clock_res_get),
            "clock_time_get" => Function::new_native_with_env(store, thread.clone(), clock_time_get),
            "environ_get" => Function::new_native_with_env(store, thread.clone(), environ_get),
            "environ_sizes_get" => Function::new_native_with_env(store, thread.clone(), environ_sizes_get),
            "fd_advise" => Function::new_native_with_env(store, thread.clone(), fd_advise),
            "fd_allocate" => Function::new_native_with_env(store, thread.clone(), fd_allocate),
            "fd_close" => Function::new_native_with_env(store, thread.clone(), fd_close),
            "fd_datasync" => Function::new_native_with_env(store, thread.clone(), fd_datasync),
            "fd_fdstat_get" => Function::new_native_with_env(store, thread.clone(), fd_fdstat_get),
            "fd_fdstat_set_flags" => Function::new_native_with_env(store, thread.clone(), fd_fdstat_set_flags),
            "fd_fdstat_set_rights" => Function::new_native_with_env(store, thread.clone(), fd_fdstat_set_rights),
            "fd_filestat_get" => Function::new_native_with_env(store, thread.clone(), legacy::snapshot0::fd_filestat_get),
            "fd_filestat_set_size" => Function::new_native_with_env(store, thread.clone(), fd_filestat_set_size),
            "fd_filestat_set_times" => Function::new_native_with_env(store, thread.clone(), fd_filestat_set_times),
            "fd_pread" => Function::new_native_with_env(store, thread.clone(), fd_pread),
            "fd_prestat_get" => Function::new_native_with_env(store, thread.clone(), fd_prestat_get),
            "fd_prestat_dir_name" => Function::new_native_with_env(store, thread.clone(), fd_prestat_dir_name),
            "fd_pwrite" => Function::new_native_with_env(store, thread.clone(), fd_pwrite),
            "fd_read" => Function::new_native_with_env(store, thread.clone(), fd_read),
            "fd_readdir" => Function::new_native_with_env(store, thread.clone(), fd_readdir),
            "fd_renumber" => Function::new_native_with_env(store, thread.clone(), fd_renumber),
            "fd_seek" => Function::new_native_with_env(store, thread.clone(), legacy::snapshot0::fd_seek),
            "fd_sync" => Function::new_native_with_env(store, thread.clone(), fd_sync),
            "fd_tell" => Function::new_native_with_env(store, thread.clone(), fd_tell),
            "fd_write" => Function::new_native_with_env(store, thread.clone(), fd_write),
            "path_create_directory" => Function::new_native_with_env(store, thread.clone(), path_create_directory),
            "path_filestat_get" => Function::new_native_with_env(store, thread.clone(), legacy::snapshot0::path_filestat_get),
            "path_filestat_set_times" => Function::new_native_with_env(store, thread.clone(), path_filestat_set_times),
            "path_link" => Function::new_native_with_env(store, thread.clone(), path_link),
            "path_open" => Function::new_native_with_env(store, thread.clone(), path_open),
            "path_readlink" => Function::new_native_with_env(store, thread.clone(), path_readlink),
            "path_remove_directory" => Function::new_native_with_env(store, thread.clone(), path_remove_directory),
            "path_rename" => Function::new_native_with_env(store, thread.clone(), path_rename),
            "path_symlink" => Function::new_native_with_env(store, thread.clone(), path_symlink),
            "path_unlink_file" => Function::new_native_with_env(store, thread.clone(), path_unlink_file),
            "poll_oneoff" => Function::new_native_with_env(store, thread.clone(), legacy::snapshot0::poll_oneoff),
            "proc_exit" => Function::new_native_with_env(store, thread.clone(), proc_exit),
            "proc_raise" => Function::new_native_with_env(store, thread.clone(), proc_raise),
            "random_get" => Function::new_native_with_env(store, thread.clone(), random_get),
            "sched_yield" => Function::new_native_with_env(store, thread.clone(), sched_yield),
            "sock_recv" => Function::new_native_with_env(store, thread.clone(), sock_recv),
            "sock_send" => Function::new_native_with_env(store, thread.clone(), sock_send),
            "sock_shutdown" => Function::new_native_with_env(store, thread, sock_shutdown),
        },
    }
}

/// Combines a state generating function with the import list for snapshot 1
fn generate_import_object_snapshot1(store: &Store, thread: WasiThread) -> Imports {
    imports! {
        "wasi_snapshot_preview1" => {
            "args_get" => Function::new_native_with_env(store, thread.clone(), args_get),
            "args_sizes_get" => Function::new_native_with_env(store, thread.clone(), args_sizes_get),
            "clock_res_get" => Function::new_native_with_env(store, thread.clone(), clock_res_get),
            "clock_time_get" => Function::new_native_with_env(store, thread.clone(), clock_time_get),
            "environ_get" => Function::new_native_with_env(store, thread.clone(), environ_get),
            "environ_sizes_get" => Function::new_native_with_env(store, thread.clone(), environ_sizes_get),
            "fd_advise" => Function::new_native_with_env(store, thread.clone(), fd_advise),
            "fd_allocate" => Function::new_native_with_env(store, thread.clone(), fd_allocate),
            "fd_close" => Function::new_native_with_env(store, thread.clone(), fd_close),
            "fd_datasync" => Function::new_native_with_env(store, thread.clone(), fd_datasync),
            "fd_fdstat_get" => Function::new_native_with_env(store, thread.clone(), fd_fdstat_get),
            "fd_fdstat_set_flags" => Function::new_native_with_env(store, thread.clone(), fd_fdstat_set_flags),
            "fd_fdstat_set_rights" => Function::new_native_with_env(store, thread.clone(), fd_fdstat_set_rights),
            "fd_filestat_get" => Function::new_native_with_env(store, thread.clone(), fd_filestat_get),
            "fd_filestat_set_size" => Function::new_native_with_env(store, thread.clone(), fd_filestat_set_size),
            "fd_filestat_set_times" => Function::new_native_with_env(store, thread.clone(), fd_filestat_set_times),
            "fd_pread" => Function::new_native_with_env(store, thread.clone(), fd_pread),
            "fd_prestat_get" => Function::new_native_with_env(store, thread.clone(), fd_prestat_get),
            "fd_prestat_dir_name" => Function::new_native_with_env(store, thread.clone(), fd_prestat_dir_name),
            "fd_pwrite" => Function::new_native_with_env(store, thread.clone(), fd_pwrite),
            "fd_read" => Function::new_native_with_env(store, thread.clone(), fd_read),
            "fd_readdir" => Function::new_native_with_env(store, thread.clone(), fd_readdir),
            "fd_renumber" => Function::new_native_with_env(store, thread.clone(), fd_renumber),
            "fd_seek" => Function::new_native_with_env(store, thread.clone(), fd_seek),
            "fd_sync" => Function::new_native_with_env(store, thread.clone(), fd_sync),
            "fd_tell" => Function::new_native_with_env(store, thread.clone(), fd_tell),
            "fd_write" => Function::new_native_with_env(store, thread.clone(), fd_write),
            "path_create_directory" => Function::new_native_with_env(store, thread.clone(), path_create_directory),
            "path_filestat_get" => Function::new_native_with_env(store, thread.clone(), path_filestat_get),
            "path_filestat_set_times" => Function::new_native_with_env(store, thread.clone(), path_filestat_set_times),
            "path_link" => Function::new_native_with_env(store, thread.clone(), path_link),
            "path_open" => Function::new_native_with_env(store, thread.clone(), path_open),
            "path_readlink" => Function::new_native_with_env(store, thread.clone(), path_readlink),
            "path_remove_directory" => Function::new_native_with_env(store, thread.clone(), path_remove_directory),
            "path_rename" => Function::new_native_with_env(store, thread.clone(), path_rename),
            "path_symlink" => Function::new_native_with_env(store, thread.clone(), path_symlink),
            "path_unlink_file" => Function::new_native_with_env(store, thread.clone(), path_unlink_file),
            "poll_oneoff" => Function::new_native_with_env(store, thread.clone(), poll_oneoff),
            "proc_exit" => Function::new_native_with_env(store, thread.clone(), proc_exit),
            "proc_raise" => Function::new_native_with_env(store, thread.clone(), proc_raise),
            "random_get" => Function::new_native_with_env(store, thread.clone(), random_get),
            "sched_yield" => Function::new_native_with_env(store, thread.clone(), sched_yield),
            "sock_recv" => Function::new_native_with_env(store, thread.clone(), sock_recv),
            "sock_send" => Function::new_native_with_env(store, thread.clone(), sock_send),
            "sock_shutdown" => Function::new_native_with_env(store, thread, sock_shutdown),
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
