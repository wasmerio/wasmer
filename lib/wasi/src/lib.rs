#![deny(unused_mut)]
#![doc(html_favicon_url = "https://wasmer.io/static/icons/favicon.ico")]
#![doc(html_logo_url = "https://github.com/wasmerio.png?size=200")]

//! Wasmer's WASI implementation
//!
//! Use `generate_import_object` to create an [`ImportObject`].  This [`ImportObject`]
//! can be combined with a module to create an `Instance` which can execute WASI
//! Wasm functions.
//!
//! See `state` for the experimental WASI FS API.  Also see the
//! [WASI plugin example](https://github.com/wasmerio/wasmer/blob/master/examples/plugin.rs)
//! for an example of how to extend WASI using the WASI FS API.

#[macro_use]
mod macros;
mod ptr;
mod state;
mod syscalls;
mod utils;

use crate::syscalls::*;

pub use crate::state::{
    Fd, Pipe, Stderr, Stdin, Stdout, WasiFile, WasiFs, WasiFsError, WasiState, WasiStateBuilder,
    WasiStateCreationError, ALL_RIGHTS, VIRTUAL_ROOT_FD,
};
pub use crate::syscalls::types;
pub use crate::utils::{get_wasi_version, is_wasi_module, WasiVersion};

use thiserror::Error;
use wasmer::{imports, Function, ImportObject, Memory, Module, Store};
#[cfg(all(target_os = "macos", target_arch = "aarch64",))]
use wasmer::{FunctionType, ValType};

use std::cell::UnsafeCell;
use std::fmt;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

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
#[derive(Debug, Clone)]
pub struct WasiEnv {
    /// Shared state of the WASI system. Manages all the data that the
    /// executing WASI program can see.
    ///
    /// Be careful when using this in host functions that call into Wasm:
    /// if the lock is held and the Wasm calls into a host function that tries
    /// to lock this mutex, the program will deadlock.
    pub state: Arc<Mutex<WasiState>>,
    memory: Arc<WasiMemory>,
}

/// Wrapper type around `Memory` used to delay initialization of the memory.
///
/// The `initialized` field is used to indicate if it's safe to read `memory` as `Memory`.
///
/// The `mutate_lock` is used to prevent access from multiple threads during initialization.
struct WasiMemory {
    initialized: AtomicBool,
    memory: UnsafeCell<MaybeUninit<Memory>>,
    mutate_lock: Mutex<()>,
}

impl fmt::Debug for WasiMemory {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("WasiMemory")
            .field("initialized", &self.initialized)
            .finish()
    }
}

impl WasiMemory {
    fn new() -> Self {
        Self {
            initialized: AtomicBool::new(false),
            memory: UnsafeCell::new(MaybeUninit::zeroed()),
            mutate_lock: Mutex::new(()),
        }
    }

    /// Initialize the memory, making it safe to read from.
    ///
    /// Returns whether or not the set was successful. If the set failed then
    /// the memory has already been initialized.
    fn set_memory(&self, memory: Memory) -> bool {
        // synchronize it
        let _guard = self.mutate_lock.lock();
        if self.initialized.load(Ordering::Acquire) {
            return false;
        }

        unsafe {
            let ptr = self.memory.get();
            let mem_inner: &mut MaybeUninit<Memory> = &mut *ptr;
            mem_inner.as_mut_ptr().write(memory);
        }
        self.initialized.store(true, Ordering::Release);

        true
    }

    /// Returns `None` if the memory has not been initialized yet.
    /// Otherwise returns the memory that was used to initialize it.
    fn get_memory(&self) -> Option<&Memory> {
        // Based on normal usage, `Relaxed` is fine...
        // TODO: investigate if it's possible to use the API in a way where `Relaxed`
        //       is not fine
        if self.initialized.load(Ordering::Relaxed) {
            unsafe {
                let maybe_mem = self.memory.get();
                Some(&*(*maybe_mem).as_ptr())
            }
        } else {
            None
        }
    }
}

impl Drop for WasiMemory {
    fn drop(&mut self) {
        if self.initialized.load(Ordering::Acquire) {
            unsafe {
                // We want to get the internal value in memory, so we need to consume
                // the `UnsafeCell` and assume the `MapbeInit` is initialized, but because
                // we only have a `&mut self` we can't do this directly, so we swap the data
                // out so we can drop it (via `assume_init`).
                let mut maybe_uninit = UnsafeCell::new(MaybeUninit::zeroed());
                std::mem::swap(&mut self.memory, &mut maybe_uninit);
                maybe_uninit.into_inner().assume_init();
            }
        }
    }
}

impl WasiEnv {
    pub fn new(state: WasiState) -> Self {
        Self {
            state: Arc::new(Mutex::new(state)),
            memory: Arc::new(WasiMemory::new()),
        }
    }

    pub fn import_object(&mut self, module: &Module) -> Result<ImportObject, WasiError> {
        let wasi_version = get_wasi_version(module, false).ok_or(WasiError::UnknownWasiVersion)?;
        Ok(generate_import_object_from_env(
            module.store(),
            self.clone(),
            wasi_version,
        ))
    }

    /// Set the memory
    pub fn set_memory(&mut self, memory: Memory) -> bool {
        self.memory.set_memory(memory)
    }

    /// Get the WASI state
    ///
    /// Be careful when using this in host functions that call into Wasm:
    /// if the lock is held and the Wasm calls into a host function that tries
    /// to lock this mutex, the program will deadlock.
    pub fn state(&self) -> MutexGuard<WasiState> {
        self.state.lock().unwrap()
    }

    // TODO: delete this method before 1.0.0 release
    #[doc(hidden)]
    #[deprecated(since = "1.0.0-beta1", note = "Please use the `state` method instead")]
    pub fn state_mut(&mut self) -> MutexGuard<WasiState> {
        self.state.lock().unwrap()
    }

    /// Get a reference to the memory
    pub fn memory(&self) -> &Memory {
        self.memory.get_memory().expect("The expected Memory is not attached to the `WasiEnv`. Did you forgot to call wasi_env.set_memory(...)?")
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

/// Create an [`ImportObject`] with an existing [`WasiEnv`]. `WasiEnv`
/// needs a [`WasiState`], that can be constructed from a
/// [`WasiStateBuilder`](state::WasiStateBuilder).
pub fn generate_import_object_from_env(
    store: &Store,
    wasi_env: WasiEnv,
    version: WasiVersion,
) -> ImportObject {
    match version {
        WasiVersion::Snapshot0 => generate_import_object_snapshot0(store, wasi_env),
        WasiVersion::Snapshot1 | WasiVersion::Latest => {
            generate_import_object_snapshot1(store, wasi_env)
        }
    }
}

// Note: we use this wrapper because native functions with more than 9 params
// fail on Apple Silicon (with Cranelift).
fn get_path_open_for_store(store: &Store, env: WasiEnv) -> Function {
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64",)))]
    let path_open = Function::new_native_with_env(store, env.clone(), path_open);
    #[cfg(all(target_os = "macos", target_arch = "aarch64",))]
    let path_open = Function::new_with_env(
        store,
        &FunctionType::new(
            vec![
                ValType::I32,
                ValType::I32,
                ValType::I32,
                ValType::I32,
                ValType::I32,
                ValType::I64,
                ValType::I64,
                ValType::I32,
                ValType::I32,
            ],
            vec![ValType::I32],
        ),
        env.clone(),
        path_open_dynamic,
    );
    path_open
}

/// Combines a state generating function with the import list for legacy WASI
fn generate_import_object_snapshot0(store: &Store, env: WasiEnv) -> ImportObject {
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
            "path_open" => get_path_open_for_store(store, env.clone()),
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
            "sock_shutdown" => Function::new_native_with_env(store, env.clone(), sock_shutdown),
        },
    }
}

/// Combines a state generating function with the import list for snapshot 1
fn generate_import_object_snapshot1(store: &Store, env: WasiEnv) -> ImportObject {
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
            "path_open" => get_path_open_for_store(store, env.clone()),
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
            "sock_shutdown" => Function::new_native_with_env(store, env.clone(), sock_shutdown),
        }
    }
}
