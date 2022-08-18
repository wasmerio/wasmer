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

#[cfg(feature="compiler")]
#[cfg(not(any(feature = "compiler-cranelift", feature = "compiler-llvm", feature = "compiler-singlepass")))]
compile_error!("Either feature \"compiler_cranelift\", \"compiler_singlepass\" or \"compiler_llvm\" must be enabled when using \"compiler\".");

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

#[macro_use]
mod macros;
pub mod runtime;
mod state;
mod syscalls;
mod utils;
pub mod fs;
#[cfg(feature = "os")]
pub mod wapm;
#[cfg(feature = "os")]
pub mod bin_factory;
#[cfg(feature = "os")]
pub mod builtins;
#[cfg(feature = "os")]
pub mod os;

#[cfg(feature = "compiler")]
pub use wasmer_compiler;
#[cfg(feature = "compiler-cranelift")]
pub use wasmer_compiler_cranelift;
#[cfg(feature = "compiler-llvm")]
pub use wasmer_compiler_llvm;
#[cfg(feature = "compiler-singlepass")]
pub use wasmer_compiler_singlepass;

pub use crate::state::{
    Fd, Pipe, WasiFs, WasiInodes, WasiState, WasiStateBuilder,
    WasiThreadId, WasiThreadHandle, WasiProcessId, WasiControlPlane, WasiThread, WasiProcess, WasiPipe,
    WasiStateCreationError, ALL_RIGHTS, VIRTUAL_ROOT_FD, default_fs_backing
};
pub use crate::syscalls::types;
pub use crate::utils::{
    get_wasi_version, get_wasi_versions, is_wasi_module, is_wasix_module, WasiVersion,
};
#[cfg(feature = "os")]
use bin_factory::BinFactory;
#[allow(unused_imports)]
use bytes::{BytesMut, Bytes};
use derivative::Derivative;
use syscalls::platform_clock_time_get;
use tracing::{trace, warn, error};
use wasmer_vbus::SpawnEnvironmentIntrinsics;
pub use wasmer_vbus::{DefaultVirtualBus, VirtualBus, BusSpawnedProcessJoin};
#[deprecated(since = "2.1.0", note = "Please use `wasmer_vfs::FsError`")]
pub use wasmer_vfs::FsError as WasiFsError;
#[deprecated(since = "2.1.0", note = "Please use `wasmer_vfs::VirtualFile`")]
pub use wasmer_vfs::VirtualFile as WasiFile;
pub use wasmer_vfs::{FsError, VirtualFile};
pub use wasmer_vnet::{UnsupportedVirtualNetworking, VirtualNetworking};
use wasmer_wasi_types::{__WASI_CLOCK_MONOTONIC, __WASI_SIGKILL, __WASI_SIGQUIT, __WASI_SIGINT, __WASI_EINTR};

// re-exports needed for OS
#[cfg(feature = "os")]
pub use wasmer_vfs;
#[cfg(feature = "os")]
pub use wasmer_vnet;
#[cfg(feature = "os")]
pub use wasmer_vbus;
#[cfg(feature = "os")]
pub use wasmer;

use std::cell::RefCell;
use std::ops::Deref;
use std::sync::atomic::{AtomicU32, Ordering};
use thiserror::Error;
use wasmer::{
    imports, namespace, AsStoreMut, Exports, Function, FunctionEnv, Imports, Memory, Memory32,
    MemoryAccessError, MemorySize, Module, TypedFunction, Memory64, MemoryView, AsStoreRef, Instance, ExportError, Global, Value, Store,
};

pub use runtime::{
    PluggableRuntimeImplementation, WasiRuntimeImplementation, WasiThreadError, WasiTtyState,
    WebSocketAbi, VirtualTaskManager, SpawnedMemory
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

/// Represents the ID of a WASI calling thread
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WasiCallingId(u32);

impl WasiCallingId {
    pub fn raw(&self) -> u32 {
        self.0
    }

    pub fn inc(&mut self) -> WasiCallingId {
        self.0 += 1;
        self.clone()
    }
}

impl From<u32> for WasiCallingId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}
impl From<WasiCallingId> for u32 {
    fn from(t: WasiCallingId) -> u32 {
        t.0 as u32
    }
}

#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct WasiEnvInner
{
    /// Represents a reference to the memory
    memory: Memory,
    /// Represents the module that is being used (this is NOT send/sync)
    /// however the code itself makes sure that it is used in a safe way
    module: Module,
    /// All the exports for the module
    exports: Exports,
    //// Points to the current location of the memory stack pointer
    stack_pointer: Option<Global>,
    /// Main function that will be invoked (name = "_start")
    #[derivative(Debug = "ignore")]
    start: Option<TypedFunction<(), ()>>,
    /// Function thats invoked to initialize the WASM module (nane = "_initialize")
    #[allow(dead_code)]
    #[derivative(Debug = "ignore")]
    initialize: Option<TypedFunction<(), ()>>,
    /// Represents the callback for spawning a thread (name = "_start_thread")
    /// (due to limitations with i64 in browsers the parameters are broken into i32 pairs)
    /// [this takes a user_data field]
    #[derivative(Debug = "ignore")]
    thread_spawn: Option<TypedFunction<(i32, i32), ()>>,
    /// Represents the callback for spawning a reactor (name = "_react")
    /// (due to limitations with i64 in browsers the parameters are broken into i32 pairs)
    /// [this takes a user_data field]
    #[derivative(Debug = "ignore")]
    react: Option<TypedFunction<(i32, i32), ()>>,
    /// Represents the callback for signals (name = "__wasm_signal")
    /// Signals are triggered asynchronously at idle times of the process
    #[derivative(Debug = "ignore")]
    signal: Option<TypedFunction<i32, ()>>,
    /// Flag that indicates if the signal callback has been set by the WASM
    /// process - if it has not been set then the runtime behaves differently
    /// when a CTRL-C is pressed.
    signal_set: bool,
    /// Represents the callback for destroying a local thread variable (name = "_thread_local_destroy")
    /// [this takes a pointer to the destructor and the data to be destroyed]
    #[derivative(Debug = "ignore")]
    thread_local_destroy: Option<TypedFunction<(i32, i32, i32, i32), ()>>,
    /// asyncify_start_unwind(data : i32): call this to start unwinding the
    /// stack from the current location. "data" must point to a data
    /// structure as described above (with fields containing valid data).
    #[derivative(Debug = "ignore")]
    asyncify_start_unwind: Option<TypedFunction<i32, ()>>,
    /// asyncify_stop_unwind(): call this to note that unwinding has
    /// concluded. If no other code will run before you start to rewind,
    /// this is not strictly necessary, however, if you swap between
    /// coroutines, or even just want to run some normal code during a
    /// "sleep", then you must call this at the proper time. Otherwise,
    /// the code will think it is still unwinding when it should not be,
    /// which means it will keep unwinding in a meaningless way.
    #[derivative(Debug = "ignore")]
    asyncify_stop_unwind: Option<TypedFunction<(), ()>>,
    /// asyncify_start_rewind(data : i32): call this to start rewinding the
    /// stack vack up to the location stored in the provided data. This prepares
    /// for the rewind; to start it, you must call the first function in the
    /// call stack to be unwound.
    #[derivative(Debug = "ignore")]
    asyncify_start_rewind: Option<TypedFunction<i32, ()>>,
    /// asyncify_stop_rewind(): call this to note that rewinding has
    /// concluded, and normal execution can resume.
    #[derivative(Debug = "ignore")]
    asyncify_stop_rewind: Option<TypedFunction<(), ()>>,
    /// asyncify_get_state(): call this to get the current value of the
    /// internal "__asyncify_state" variable as described above.
    /// It can be used to distinguish between unwinding/rewinding and normal
    /// calls, so that you know when to start an asynchronous operation and
    /// when to propagate results back.
    #[allow(dead_code)]
    #[derivative(Debug = "ignore")]
    asyncify_get_state: Option<TypedFunction<(), i32>>,
}

impl WasiEnvInner
{
    pub fn new(module: Module, memory: Memory, store: &impl AsStoreRef, instance: &Instance) -> Self
    {
        WasiEnvInner {
            module,
            memory,
            exports: instance.exports.clone(),
            stack_pointer: instance.exports.get_global("__stack_pointer").map(|a| a.clone()).ok(),
            start: instance.exports.get_typed_function(store, "_start").ok(),
            initialize: instance.exports.get_typed_function(store, "_initialize").ok(),
            thread_spawn: instance.exports.get_typed_function(store, "_start_thread").ok(),
            react: instance.exports.get_typed_function(store, "_react").ok(),
            signal: instance.exports.get_typed_function(store, "__wasm_signal").ok(),
            signal_set: false,
            asyncify_start_unwind: instance.exports.get_typed_function(store, "asyncify_start_unwind").ok(),
            asyncify_stop_unwind: instance.exports.get_typed_function(store, "asyncify_stop_unwind").ok(),
            asyncify_start_rewind: instance.exports.get_typed_function(store, "asyncify_start_rewind").ok(),
            asyncify_stop_rewind: instance.exports.get_typed_function(store, "asyncify_stop_rewind").ok(),
            asyncify_get_state: instance.exports.get_typed_function(store, "asyncify_get_state").ok(),
            thread_local_destroy: instance.exports.get_typed_function(store, "_thread_local_destroy").ok(),
        }
    }
}

/// The code itself makes safe use of the struct so multiple threads don't access
/// it (without this the JS code prevents the reference to the module from being stored
/// which is needed for the multithreading mode)
unsafe impl Send for WasiEnvInner { }
unsafe impl Sync for WasiEnvInner { }

/// The default stack size for WASIX
pub const DEFAULT_STACK_SIZE: u64 = 1_048_576u64;
pub const DEFAULT_STACK_BASE: u64 = DEFAULT_STACK_SIZE;

#[derive(Debug, Clone)]
pub struct WasiVFork {
    /// The unwound stack before the vfork occured
    pub rewind_stack: BytesMut,
    /// The memory stack before the vfork occured
    pub memory_stack: BytesMut,
    /// The mutable parts of the store
    pub store_data: Bytes,
    /// The environment before the vfork occured
    pub env: Box<WasiEnv>,
    /// Handle of the thread we have forked (dropping this handle
    /// will signal that the thread is dead)
    pub handle: WasiThreadHandle,
    /// Offset into the memory where the PID will be
    /// written when the real fork takes places
    pub pid_offset: u64,
}

/// The environment provided to the WASI imports.
#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct WasiEnv
where
{
    /// Represents the process this environment is attached to
    pub process: WasiProcess,
    /// Represents the thread this environment is attached to
    pub thread: WasiThread,
    /// Represents a fork of the process that is currently in play
    pub vfork: Option<WasiVFork>,
    /// Base stack pointer for the memory stack
    pub stack_base: u64,
    /// Start of the stack memory that is allocated for this thread
    pub stack_start: u64,
    /// Shared state of the WASI system. Manages all the data that the
    /// executing WASI program can see.
    pub state: Arc<WasiState>,
    /// Binary factory attached to this environment
    #[cfg(feature = "os")]
    #[derivative(Debug = "ignore")]
    pub bin_factory: BinFactory,
    /// Inner functions and references that are loaded before the environment starts
    pub inner: Option<WasiEnvInner>,
    /// List of the handles that are owned by this context 
    /// (this can be used to ensure that threads own themselves or others)
    pub owned_handles: Vec<WasiThreadHandle>,
    /// Implementation of the WASI runtime.
    pub runtime: Arc<dyn WasiRuntimeImplementation + Send + Sync + 'static>,
    /// Task manager used to spawn threads and manage the ASYNC runtime
    pub tasks: Arc<dyn VirtualTaskManager + Send + Sync + 'static>
}

impl WasiEnv {
    /// Forking the WasiState is used when either fork or vfork is called
    pub fn fork(&self) -> (Self, WasiThreadHandle)
    {
        let process = self.process.compute.new_process();        
        let handle = process.new_thread();
        
        let thread = handle.as_thread();
        thread.copy_stack_from(&self.thread);
        
        let state = Arc::new(self.state.fork());
        
        #[cfg(feature = "os")]
        let bin_factory = {
            let mut bin_factory = self.bin_factory.clone();
            bin_factory.state = state.clone();
            bin_factory
        };

        (
            Self {
                process: process,
                thread,
                vfork: None,
                stack_base: self.stack_base,
                stack_start: self.stack_start,
                #[cfg(feature = "os")]
                bin_factory,
                state,
                inner: None,
                owned_handles: Vec::new(),
                runtime: self.runtime.clone(),
                tasks: self.tasks.clone(),
            },
            handle
        )
    }

    pub fn pid(&self) -> WasiProcessId {
        self.process.pid()
    }

    pub fn tid(&self) -> WasiThreadId {
        self.thread.tid()
    }
}

// Represents the current thread ID for the executing method
thread_local!(pub(crate) static CALLER_ID: RefCell<u32> = RefCell::new(0));
thread_local!(pub(crate) static REWIND: RefCell<Option<bytes::Bytes>> = RefCell::new(None));
lazy_static::lazy_static! {
    static ref CALLER_ID_SEED: Arc<AtomicU32> = Arc::new(AtomicU32::new(1));
}

/// Returns the current thread ID
pub fn current_caller_id() -> WasiCallingId {
    CALLER_ID.with(|f| {
        let mut caller_id = f.borrow_mut();
        if *caller_id == 0 {
            *caller_id = CALLER_ID_SEED.fetch_add(1, Ordering::AcqRel);
        }
        *caller_id
    }).into()
}

impl WasiEnv {
    pub fn new(state: WasiState, #[cfg(feature = "os")] compiled_modules: Arc<bin_factory::CachedCompiledModules>, process: WasiProcess, thread: WasiThreadHandle) -> Self {
        let state = Arc::new(state);
        let runtime = Arc::new(PluggableRuntimeImplementation::default());
        Self::new_ext(state, #[cfg(feature = "os")] compiled_modules, process, thread, runtime)
    }

    pub fn new_ext(state: Arc<WasiState>, #[cfg(feature = "os")] compiled_modules: Arc<bin_factory::CachedCompiledModules>, process: WasiProcess, thread: WasiThreadHandle, runtime: Arc<dyn WasiRuntimeImplementation + Send + Sync>) -> Self {
        #[cfg(feature = "os")]
        let bin_factory = BinFactory::new(
            state.clone(),
            compiled_modules,
            runtime.clone()
        );
        let tasks = runtime.new_task_manager();
        let mut ret = Self {
            process,
            thread: thread.as_thread(),
            vfork: None,
            stack_base: DEFAULT_STACK_SIZE,
            stack_start: 0,
            state,
            inner: None,
            owned_handles: Vec::new(),
            runtime,
            tasks,
            #[cfg(feature = "os")]
            bin_factory
        };
        ret.owned_handles.push(thread);
        ret
    }
    
    /// Returns a copy of the current runtime implementation for this environment
    pub fn runtime<'a>(&'a self) -> &'a (dyn WasiRuntimeImplementation) {
        self.runtime.deref()
    }

    /// Returns a copy of the current tasks implementation for this environment
    pub fn tasks<'a>(&'a self) -> &'a (dyn VirtualTaskManager) {
        self.tasks.deref()
    }

    /// Overrides the runtime implementation for this environment
    pub fn set_runtime<R>(&mut self, runtime: R) 
    where
        R: WasiRuntimeImplementation + Send + Sync + 'static,
    {
        self.runtime = Arc::new(runtime);
    }

    /// Returns the number of active threads
    pub fn active_threads(&self) -> u32 {
        self.process.active_threads()
    }

    /// Porcesses any signals that are batched up
    pub fn process_signals(&self, store: &mut impl AsStoreMut) -> Result<(), WasiError>
    {
        // If a signal handler has never been set then we need to handle signals
        // differently
        if self.inner().signal_set == false {
            let signals = self.thread.pop_signals();
            for sig in signals {
                if sig == __WASI_SIGINT ||
                   sig == __WASI_SIGQUIT ||
                   sig == __WASI_SIGKILL
                {
                    return Err(WasiError::Exit(__WASI_EINTR as u32));
                } else {
                    trace!("wasi[{}]::signal-ignored: {}", self.pid(), sig);
                }
            }
        }

        // Check for any signals that we need to trigger
        // (but only if a signal handler is registered)
        if let Some(handler) = self.inner().signal.clone() {
            let mut signals = self.thread.pop_signals();

            // We might also have signals that trigger on timers
            let mut now = 0;
            let has_signal_interval = {
                let mut any = false;
                let inner = self.process.inner.read().unwrap();
                if inner.signal_intervals.is_empty() == false {
                    now = platform_clock_time_get(__WASI_CLOCK_MONOTONIC, 1_000_000).unwrap() as u128;
                    for signal in inner.signal_intervals.values() {
                        let elapsed = now - signal.last_signal;
                        if elapsed >= signal.interval.as_nanos() {
                            any = true;
                            break;
                        }
                    }
                }
                any
            };
            if has_signal_interval {
                let mut inner = self.process.inner.write().unwrap();
                for signal in inner.signal_intervals.values_mut() {
                    let elapsed = now - signal.last_signal;
                    if elapsed >= signal.interval.as_nanos() {
                        signal.last_signal = now;
                        signals.push(signal.signal);
                    }
                }
            }

            for signal in signals {
                tracing::trace!("wasi[{}]::processing-signal: {}", self.pid(), signal);
                if let Err(err) = handler.call(store, signal as i32) {
                    match err.downcast::<WasiError>() {
                        Ok(err) => {
                            return Err(err);
                        }
                        Err(err) => {
                            warn!("wasi[{}]::signal handler runtime error - {}", self.pid(), err);
                            return Err(WasiError::Exit(1));
                        }
                    }
                }
            }
        }
        self.yield_now()
    }

    // Yields execution
    pub fn yield_now_with_signals(&self, store: &mut impl AsStoreMut) -> Result<(), WasiError>
    {
        self.process_signals(store)?;
        self.yield_now()
    }

    // Yields execution
    pub fn yield_now(&self) -> Result<(), WasiError> {
        if let Some(forced_exit) = self.thread.try_join() {
            return Err(WasiError::Exit(forced_exit));
        }
        if let Some(forced_exit) = self.process.try_join() {
            return Err(WasiError::Exit(forced_exit));
        }
        let tasks = self.tasks.clone();
        self.tasks.block_on(Box::pin(async move {
            tasks.sleep_now(current_caller_id(), 0);
        }));        
        Ok(())
    }
    
    // Sleeps for a period of time
    pub fn sleep(&self, store: &mut impl AsStoreMut, duration: Duration) -> Result<(), WasiError> {
        let mut signaler = self.thread.signals.1.subscribe();
        
        let tasks = self.tasks.clone();
        let (tx_signaller, mut rx_signaller) = tokio::sync::mpsc::unbounded_channel();
        self.tasks.block_on(Box::pin(async move {
            loop {
                tokio::select! {
                    _ = tasks.sleep_now(current_caller_id(), duration.as_millis()) => { },
                    _ = signaler.recv() => {
                        let _ = tx_signaller.send(true);
                        break;
                    }
                }
            }
        }));
        if let Ok(true) = rx_signaller.try_recv() {
            self.process_signals(store)?;
        }
        Ok(())
    }

    /// Accesses the virtual networking implementation
    pub fn net<'a>(&'a self) -> Arc<dyn VirtualNetworking + Send + Sync + 'static> {
        self.runtime.networking()
    }

    /// Accesses the virtual bus implementation
    pub fn bus<'a>(&'a self) -> Arc<dyn VirtualBus<WasiEnv> + Send + Sync + 'static> {
        self.runtime.bus()
    }

    /// Providers safe access to the initialized part of WasiEnv
    /// (it must be initialized before it can be used)
    pub fn inner(&self) -> &WasiEnvInner {
        self.inner.as_ref()
            .expect("You must initialize the WasiEnv before using it")
    }

    /// Providers safe access to the initialized part of WasiEnv
    /// (it must be initialized before it can be used)
    pub fn inner_mut(&mut self) -> &mut WasiEnvInner {
        self.inner.as_mut()
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

    #[cfg(feature = "os")]
    pub fn uses<'a, I>(&self, uses: I) -> Result<(), WasiStateCreationError>
    where I: IntoIterator<Item = String>
    {
        use std::{collections::{VecDeque, HashMap}, borrow::Cow};
        // Load all the containers that we inherit from
        #[allow(unused_imports)]
        use std::path::Path;
        #[allow(unused_imports)]
        use wasmer_vfs::FileSystem;

        use crate::state::WasiFsRoot;

        let mut already: HashMap<String, Cow<'static, str>> = HashMap::new();

        let mut use_packages = uses.into_iter().collect::<VecDeque<_>>();
        while let Some(use_package) = use_packages.pop_back() {
            if let Some(package) = self.bin_factory.builtins.cmd_wasmer.get(use_package.clone(), self.tasks.deref())
            {
                // If its already been added make sure the version is correct
                let package_name = package.package_name.to_string();
                if let Some(version) = already.get(&package_name) {
                    if version.as_ref() != package.version.as_ref() {
                        return Err(WasiStateCreationError::WasiInheritError(format!("webc package version conflict for {} - {} vs {}", use_package, version, package.version)));
                    }
                    continue;
                }
                already.insert(package_name, package.version.clone());
                
                // Add the additional dependencies
                for dependency in package.uses.clone() {
                    use_packages.push_back(dependency);
                }

                if let WasiFsRoot::Sandbox(root_fs) = &self.state.fs.root_fs {
                    // We first need to copy any files in the package over to the temporary file system
                    if let Some(fs) = package.webc_fs.as_ref() {
                        root_fs.union(fs);
                    }

                    // Add all the commands as binaries in the bin folder
                    let commands = package.commands.read().unwrap();
                    if commands.is_empty() == false {
                        let _ = root_fs.create_dir(Path::new("/bin"));
                        for command in commands.iter() {
                            let path = format!("/bin/{}", command.name);
                            let path = Path::new(path.as_str());
                            if let Err(err) = root_fs
                                .new_open_options_ext()
                                .insert_ro_file(path, command.atom.clone())
                            {
                                tracing::debug!("failed to add package [{}] command [{}] - {}", use_package, command.name, err);
                                continue;
                            }

                            // Add the binary package to the bin factory (zero copy the atom)
                            let mut package = package.clone();
                            package.entry = command.atom.clone();
                            self.bin_factory.set_binary(path.as_os_str().to_string_lossy().as_ref(), package);
                        }
                    }
                } else {
                    return Err(WasiStateCreationError::WasiInheritError(format!("failed to add package as the file system is not sandboxed")));
                }
            } else {
                return Err(WasiStateCreationError::WasiInheritError(format!("failed to fetch webc package for {}", use_package)));
            }
        }
        Ok(())
    }

    #[cfg(feature = "os")]
    #[cfg(feature = "sys")]
    pub fn map_commands(&self, map_commands: std::collections::HashMap<String, std::path::PathBuf>) -> Result<(), WasiStateCreationError>
    {
        // Load all the mapped atoms
        #[allow(unused_imports)]
        use std::path::Path;
        #[allow(unused_imports)]
        use wasmer_vfs::FileSystem;

        use crate::state::WasiFsRoot;

        #[cfg(feature = "sys")]
        for (command, target) in map_commands.iter() {
            // Read the file
            let file = std::fs::read(target)
                .map_err(|err| {
                    WasiStateCreationError::WasiInheritError(format!("failed to read local binary [{}] - {}", target.as_os_str().to_string_lossy(), err))
                })?;
            let file: std::borrow::Cow<'static, [u8]> = file.into();
            
            if let WasiFsRoot::Sandbox(root_fs) = &self.state.fs.root_fs {
                let _ = root_fs.create_dir(Path::new("/bin"));
                
                let path = format!("/bin/{}", command);
                let path = Path::new(path.as_str());
                if let Err(err) = root_fs
                    .new_open_options_ext()
                    .insert_ro_file(path, file)
                {
                    tracing::debug!("failed to add atom command [{}] - {}", command, err);
                    continue;
                }
            } else {
                tracing::debug!("failed to add atom command [{}] to the root file system as it is not sandboxed", command);
                continue;
            }
        }
        Ok(())
    }
}

impl SpawnEnvironmentIntrinsics
for WasiEnv
{
    fn args(&self) -> &Vec<String> {
        &self.state.args
    }

    fn preopen(&self) -> &Vec<String> {
        &self.state.preopen
    }

    fn stdin_mode(&self) -> wasmer_vbus::StdioMode {
        match self.state.stdin() {
            Ok(Some(_)) => wasmer_vbus::StdioMode::Inherit,
            _ => wasmer_vbus::StdioMode::Null,
        }
    }

    fn stdout_mode(&self) -> wasmer_vbus::StdioMode {
        match self.state.stdout() {
            Ok(Some(_)) => wasmer_vbus::StdioMode::Inherit,
            _ => wasmer_vbus::StdioMode::Null,
        }
    }

    fn stderr_mode(&self) -> wasmer_vbus::StdioMode {
        match self.state.stderr() {
            Ok(Some(_)) => wasmer_vbus::StdioMode::Inherit,
            _ => wasmer_vbus::StdioMode::Null,
        }
    }

    fn working_dir(&self) -> String {
        let guard = self.state.fs.current_dir.lock().unwrap();
        guard.clone()
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
    pub fn data_mut<'a>(&'a mut self, store: &'a mut impl AsStoreMut) -> &'a mut WasiEnv {
        self.env
            .as_mut(store)
    }

    /// Initializes the WasiEnv using the instance exports
    /// (this must be executed before attempting to use it)
    /// (as the stores can not by themselves be passed between threads we can store the module
    ///  in a thread-local variables and use it later - for multithreading)
    pub fn initialize(&mut self, store: &mut impl AsStoreMut, instance: &Instance) -> Result<(), ExportError>
    {
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
        let new_inner = WasiEnvInner {
            memory,
            module: instance.module().clone(),
            exports: instance.exports.clone(),
            stack_pointer: instance.exports.get_global("__stack_pointer").map(|a| a.clone()).ok(),
            start: instance.exports.get_typed_function(store, "_start").ok(),
            initialize: instance.exports.get_typed_function(store, "_initialize").ok(),
            thread_spawn: instance.exports.get_typed_function(store, "_start_thread").ok(),
            react: instance.exports.get_typed_function(store, "_react").ok(),
            signal: instance.exports.get_typed_function(&store, "__wasm_signal").ok(),
            signal_set: false,
            asyncify_start_unwind: instance.exports.get_typed_function(store, "asyncify_start_unwind").ok(),
            asyncify_stop_unwind: instance.exports.get_typed_function(store, "asyncify_stop_unwind").ok(),
            asyncify_start_rewind: instance.exports.get_typed_function(store, "asyncify_start_rewind").ok(),
            asyncify_stop_rewind: instance.exports.get_typed_function(store, "asyncify_stop_rewind").ok(),
            asyncify_get_state: instance.exports.get_typed_function(store, "asyncify_get_state").ok(),
            thread_local_destroy: instance.exports.get_typed_function(store, "_thread_local_destroy").ok(),
        };

        let env = self.data_mut(store);
        env.inner.replace(new_inner);

        env.state.fs.is_wasix.store(
            is_wasix_module(instance.module()),
            std::sync::atomic::Ordering::Release,
        );

        // Set the base stack
        let stack_base = if let Some(stack_pointer) = env.inner().stack_pointer.clone() {
            match stack_pointer.get(store) {
                Value::I32(a) => a as u64,
                Value::I64(a) => a as u64,
                _ => DEFAULT_STACK_SIZE
            }
        } else {
            DEFAULT_STACK_SIZE
        };
        self.data_mut(store).stack_base = stack_base;

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

    pub fn cleanup(&self, store: &mut Store) {
        trace!("wasi[{}]:: cleaning up local thread variables", self.data(store).pid());

        // Destroy all the local thread variables that were allocated for this thread
        let to_local_destroy = {
            let thread_id = self.data(store).thread.tid();
            let mut to_local_destroy = Vec::new();
            let mut inner = self.data(store).process.write();
            for ((thread, key), val) in inner.thread_local.iter() {
                if *thread == thread_id {
                    if let Some(user_data) = inner.thread_local_user_data.get(key) {
                        to_local_destroy.push((*user_data, *val))
                    }
                }
            }
            inner.thread_local.retain(|(t, _), _| *t != thread_id);
            to_local_destroy
        };
        if to_local_destroy.len() > 0 {
            if let Some(thread_local_destroy) = self.data(store).inner().thread_local_destroy.as_ref().map(|a| a.clone()) {
                for (user_data, val) in to_local_destroy {
                    let user_data_low: u32 = (user_data & 0xFFFFFFFF) as u32;
                    let user_data_high: u32 = (user_data >> 32) as u32;

                    let val_low: u32 = (val & 0xFFFFFFFF) as u32;
                    let val_high: u32 = (val >> 32) as u32;

                    let _ = thread_local_destroy.call(store, user_data_low as i32, user_data_high as i32, val_low as i32, val_high as i32);
                }
            }
        }

        // If this is the main thread then also close all the files
        if self.data(store).thread.is_main() {
            trace!("wasi[{}]:: cleaning up open file handles", self.data(store).pid());
            
            let inodes = self.data(store).state.inodes.read().unwrap();
            self.data(store).state.fs.close_all(inodes.deref());
        }
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

fn wasi_unstable_exports(mut store: &mut impl AsStoreMut, env: &FunctionEnv<WasiEnv>) -> Exports {
    use syscalls::*;
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
        "proc_exit" => Function::new_typed_with_env(&mut store, env, proc_exit::<Memory32>),
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
    use syscalls::*;
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
        "proc_exit" => Function::new_typed_with_env(&mut store, env, proc_exit::<Memory32>),
        "proc_raise" => Function::new_typed_with_env(&mut store, env, proc_raise),
        "random_get" => Function::new_typed_with_env(&mut store, env, random_get::<Memory32>),
        "sched_yield" => Function::new_typed_with_env(&mut store, env, sched_yield),
        "sock_recv" => Function::new_typed_with_env(&mut store, env, sock_recv::<Memory32>),
        "sock_send" => Function::new_typed_with_env(&mut store, env, sock_send::<Memory32>),
        "sock_shutdown" => Function::new_typed_with_env(&mut store, env, sock_shutdown),
    };
    namespace
}

fn wasix_exports_32(
    mut store: &mut impl AsStoreMut,
    env: &FunctionEnv<WasiEnv>,
) -> Exports
{
    use syscalls::*;
    let namespace = namespace! {
        "args_get" => Function::new_typed_with_env(&mut store, env, args_get::<Memory32>),
        "args_sizes_get" => Function::new_typed_with_env(&mut store, env, args_sizes_get::<Memory32>),
        "clock_res_get" => Function::new_typed_with_env(&mut store, env, clock_res_get::<Memory32>),
        "clock_time_get" => Function::new_typed_with_env(&mut store, env, clock_time_get::<Memory32>),
        "clock_time_set" => Function::new_typed_with_env(&mut store, env, clock_time_set::<Memory32>),
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
        "fd_dup" => Function::new_typed_with_env(&mut store, env, fd_dup::<Memory32>),
        "fd_event" => Function::new_typed_with_env(&mut store, env, fd_event::<Memory32>),
        "fd_seek" => Function::new_typed_with_env(&mut store, env, fd_seek::<Memory32>),
        "fd_sync" => Function::new_typed_with_env(&mut store, env, fd_sync),
        "fd_tell" => Function::new_typed_with_env(&mut store, env, fd_tell::<Memory32>),
        "fd_write" => Function::new_typed_with_env(&mut store, env, fd_write::<Memory32>),
        "fd_pipe" => Function::new_typed_with_env(&mut store, env, fd_pipe::<Memory32>),
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
        "proc_exit" => Function::new_typed_with_env(&mut store, env, proc_exit::<Memory32>),
        "proc_fork" => Function::new_typed_with_env(&mut store, env, proc_fork::<Memory32>),
        "proc_join" => Function::new_typed_with_env(&mut store, env, proc_join::<Memory32>),
        "proc_signal" => Function::new_typed_with_env(&mut store, env, proc_signal::<Memory32>),
        "proc_exec" => Function::new_typed_with_env(&mut store, env, proc_exec::<Memory32>),
        "proc_raise" => Function::new_typed_with_env(&mut store, env, proc_raise),
        "proc_raise_interval" => Function::new_typed_with_env(&mut store, env, proc_raise_interval),
        "proc_spawn" => Function::new_typed_with_env(&mut store, env, proc_spawn::<Memory32>),
        "proc_id" => Function::new_typed_with_env(&mut store, env, proc_id::<Memory32>),
        "proc_parent" => Function::new_typed_with_env(&mut store, env, proc_parent::<Memory32>),
        "random_get" => Function::new_typed_with_env(&mut store, env, random_get::<Memory32>),
        "tty_get" => Function::new_typed_with_env(&mut store, env, tty_get::<Memory32>),
        "tty_set" => Function::new_typed_with_env(&mut store, env, tty_set::<Memory32>),
        "getcwd" => Function::new_typed_with_env(&mut store, env, getcwd::<Memory32>),
        "chdir" => Function::new_typed_with_env(&mut store, env, chdir::<Memory32>),
        "callback_signal" => Function::new_typed_with_env(&mut store, env, callback_signal::<Memory32>),
        "callback_thread" => Function::new_typed_with_env(&mut store, env, callback_thread::<Memory32>),
        "callback_reactor" => Function::new_typed_with_env(&mut store, env, callback_reactor::<Memory32>),
        "callback_thread_local_destroy" => Function::new_typed_with_env(&mut store, env, callback_thread_local_destroy::<Memory32>),
        "thread_spawn" => Function::new_typed_with_env(&mut store, env, thread_spawn::<Memory32>),
        "thread_local_create" => Function::new_typed_with_env(&mut store, env, thread_local_create::<Memory32>),
        "thread_local_destroy" => Function::new_typed_with_env(&mut store, env, thread_local_destroy),
        "thread_local_set" => Function::new_typed_with_env(&mut store, env, thread_local_set),
        "thread_local_get" => Function::new_typed_with_env(&mut store, env, thread_local_get::<Memory32>),
        "thread_sleep" => Function::new_typed_with_env(&mut store, env, thread_sleep),
        "thread_id" => Function::new_typed_with_env(&mut store, env, thread_id::<Memory32>),
        "thread_signal" => Function::new_typed_with_env(&mut store, env, thread_signal),
        "thread_join" => Function::new_typed_with_env(&mut store, env, thread_join),
        "thread_parallelism" => Function::new_typed_with_env(&mut store, env, thread_parallelism::<Memory32>),
        "thread_exit" => Function::new_typed_with_env(&mut store, env, thread_exit),
        "sched_yield" => Function::new_typed_with_env(&mut store, env, sched_yield),
        "stack_checkpoint" => Function::new_typed_with_env(&mut store, env, stack_checkpoint::<Memory32>),
        "stack_restore" => Function::new_typed_with_env(&mut store, env, stack_restore::<Memory32>),
        "futex_wait" => Function::new_typed_with_env(&mut store, env, futex_wait::<Memory32>),
        "futex_wake" => Function::new_typed_with_env(&mut store, env, futex_wake::<Memory32>),
        "futex_wake_all" => Function::new_typed_with_env(&mut store, env, futex_wake_all::<Memory32>),
        "bus_open_local" => Function::new_typed_with_env(&mut store, env, bus_open_local::<Memory32>),
        "bus_open_remote" => Function::new_typed_with_env(&mut store, env, bus_open_remote::<Memory32>),
        "bus_close" => Function::new_typed_with_env(&mut store, env, bus_close),
        "bus_call" => Function::new_typed_with_env(&mut store, env, bus_call::<Memory32>),
        "bus_subcall" => Function::new_typed_with_env(&mut store, env, bus_subcall::<Memory32>),
        "bus_poll" => Function::new_typed_with_env(&mut store, env, bus_poll::<Memory32>),
        "call_reply" => Function::new_typed_with_env(&mut store, env, call_reply::<Memory32>),
        "call_fault" => Function::new_typed_with_env(&mut store, env, call_fault),
        "call_close" => Function::new_typed_with_env(&mut store, env, call_close),
        "ws_connect" => Function::new_typed_with_env(&mut store, env, ws_connect::<Memory32>),
        "http_request" => Function::new_typed_with_env(&mut store, env, http_request::<Memory32>),
        "http_status" => Function::new_typed_with_env(&mut store, env, http_status::<Memory32>),
        "port_bridge" => Function::new_typed_with_env(&mut store, env, port_bridge::<Memory32>),
        "port_unbridge" => Function::new_typed_with_env(&mut store, env, port_unbridge),
        "port_dhcp_acquire" => Function::new_typed_with_env(&mut store, env, port_dhcp_acquire),
        "port_addr_add" => Function::new_typed_with_env(&mut store, env, port_addr_add::<Memory32>),
        "port_addr_remove" => Function::new_typed_with_env(&mut store, env, port_addr_remove::<Memory32>),
        "port_addr_clear" => Function::new_typed_with_env(&mut store, env, port_addr_clear),
        "port_addr_list" => Function::new_typed_with_env(&mut store, env, port_addr_list::<Memory32>),
        "port_mac" => Function::new_typed_with_env(&mut store, env, port_mac::<Memory32>),
        "port_gateway_set" => Function::new_typed_with_env(&mut store, env, port_gateway_set::<Memory32>),
        "port_route_add" => Function::new_typed_with_env(&mut store, env, port_route_add::<Memory32>),
        "port_route_remove" => Function::new_typed_with_env(&mut store, env, port_route_remove::<Memory32>),
        "port_route_clear" => Function::new_typed_with_env(&mut store, env, port_route_clear),
        "port_route_list" => Function::new_typed_with_env(&mut store, env, port_route_list::<Memory32>),
        "sock_status" => Function::new_typed_with_env(&mut store, env, sock_status::<Memory32>),
        "sock_addr_local" => Function::new_typed_with_env(&mut store, env, sock_addr_local::<Memory32>),
        "sock_addr_peer" => Function::new_typed_with_env(&mut store, env, sock_addr_peer::<Memory32>),
        "sock_open" => Function::new_typed_with_env(&mut store, env, sock_open::<Memory32>),
        "sock_set_opt_flag" => Function::new_typed_with_env(&mut store, env, sock_set_opt_flag),
        "sock_get_opt_flag" => Function::new_typed_with_env(&mut store, env, sock_get_opt_flag::<Memory32>),
        "sock_set_opt_time" => Function::new_typed_with_env(&mut store, env, sock_set_opt_time::<Memory32>),
        "sock_get_opt_time" => Function::new_typed_with_env(&mut store, env, sock_get_opt_time::<Memory32>),
        "sock_set_opt_size" => Function::new_typed_with_env(&mut store, env, sock_set_opt_size),
        "sock_get_opt_size" => Function::new_typed_with_env(&mut store, env, sock_get_opt_size::<Memory32>),
        "sock_join_multicast_v4" => Function::new_typed_with_env(&mut store, env, sock_join_multicast_v4::<Memory32>),
        "sock_leave_multicast_v4" => Function::new_typed_with_env(&mut store, env, sock_leave_multicast_v4::<Memory32>),
        "sock_join_multicast_v6" => Function::new_typed_with_env(&mut store, env, sock_join_multicast_v6::<Memory32>),
        "sock_leave_multicast_v6" => Function::new_typed_with_env(&mut store, env, sock_leave_multicast_v6::<Memory32>),
        "sock_bind" => Function::new_typed_with_env(&mut store, env, sock_bind::<Memory32>),
        "sock_listen" => Function::new_typed_with_env(&mut store, env, sock_listen::<Memory32>),
        "sock_accept" => Function::new_typed_with_env(&mut store, env, sock_accept::<Memory32>),
        "sock_connect" => Function::new_typed_with_env(&mut store, env, sock_connect::<Memory32>),
        "sock_recv" => Function::new_typed_with_env(&mut store, env, sock_recv::<Memory32>),
        "sock_recv_from" => Function::new_typed_with_env(&mut store, env, sock_recv_from::<Memory32>),
        "sock_send" => Function::new_typed_with_env(&mut store, env, sock_send::<Memory32>),
        "sock_send_to" => Function::new_typed_with_env(&mut store, env, sock_send_to::<Memory32>),
        "sock_send_file" => Function::new_typed_with_env(&mut store, env, sock_send_file::<Memory32>),
        "sock_shutdown" => Function::new_typed_with_env(&mut store, env, sock_shutdown),
        "resolve" => Function::new_typed_with_env(&mut store, env, resolve::<Memory32>),
    };
    namespace
}

fn wasix_exports_64(
    mut store: &mut impl AsStoreMut,
    env: &FunctionEnv<WasiEnv>,
) -> Exports
{
    use syscalls::*;
    let namespace = namespace! {
        "args_get" => Function::new_typed_with_env(&mut store, env, args_get::<Memory64>),
        "args_sizes_get" => Function::new_typed_with_env(&mut store, env, args_sizes_get::<Memory64>),
        "clock_res_get" => Function::new_typed_with_env(&mut store, env, clock_res_get::<Memory64>),
        "clock_time_get" => Function::new_typed_with_env(&mut store, env, clock_time_get::<Memory64>),
        "clock_time_set" => Function::new_typed_with_env(&mut store, env, clock_time_set::<Memory64>),
        "environ_get" => Function::new_typed_with_env(&mut store, env, environ_get::<Memory64>),
        "environ_sizes_get" => Function::new_typed_with_env(&mut store, env, environ_sizes_get::<Memory64>),
        "fd_advise" => Function::new_typed_with_env(&mut store, env, fd_advise),
        "fd_allocate" => Function::new_typed_with_env(&mut store, env, fd_allocate),
        "fd_close" => Function::new_typed_with_env(&mut store, env, fd_close),
        "fd_datasync" => Function::new_typed_with_env(&mut store, env, fd_datasync),
        "fd_fdstat_get" => Function::new_typed_with_env(&mut store, env, fd_fdstat_get::<Memory64>),
        "fd_fdstat_set_flags" => Function::new_typed_with_env(&mut store, env, fd_fdstat_set_flags),
        "fd_fdstat_set_rights" => Function::new_typed_with_env(&mut store, env, fd_fdstat_set_rights),
        "fd_filestat_get" => Function::new_typed_with_env(&mut store, env, fd_filestat_get::<Memory64>),
        "fd_filestat_set_size" => Function::new_typed_with_env(&mut store, env, fd_filestat_set_size),
        "fd_filestat_set_times" => Function::new_typed_with_env(&mut store, env, fd_filestat_set_times),
        "fd_pread" => Function::new_typed_with_env(&mut store, env, fd_pread::<Memory64>),
        "fd_prestat_get" => Function::new_typed_with_env(&mut store, env, fd_prestat_get::<Memory64>),
        "fd_prestat_dir_name" => Function::new_typed_with_env(&mut store, env, fd_prestat_dir_name::<Memory64>),
        "fd_pwrite" => Function::new_typed_with_env(&mut store, env, fd_pwrite::<Memory64>),
        "fd_read" => Function::new_typed_with_env(&mut store, env, fd_read::<Memory64>),
        "fd_readdir" => Function::new_typed_with_env(&mut store, env, fd_readdir::<Memory64>),
        "fd_renumber" => Function::new_typed_with_env(&mut store, env, fd_renumber),
        "fd_dup" => Function::new_typed_with_env(&mut store, env, fd_dup::<Memory64>),
        "fd_event" => Function::new_typed_with_env(&mut store, env, fd_event::<Memory64>),
        "fd_seek" => Function::new_typed_with_env(&mut store, env, fd_seek::<Memory64>),
        "fd_sync" => Function::new_typed_with_env(&mut store, env, fd_sync),
        "fd_tell" => Function::new_typed_with_env(&mut store, env, fd_tell::<Memory64>),
        "fd_write" => Function::new_typed_with_env(&mut store, env, fd_write::<Memory64>),
        "fd_pipe" => Function::new_typed_with_env(&mut store, env, fd_pipe::<Memory64>),
        "path_create_directory" => Function::new_typed_with_env(&mut store, env, path_create_directory::<Memory64>),
        "path_filestat_get" => Function::new_typed_with_env(&mut store, env, path_filestat_get::<Memory64>),
        "path_filestat_set_times" => Function::new_typed_with_env(&mut store, env, path_filestat_set_times::<Memory64>),
        "path_link" => Function::new_typed_with_env(&mut store, env, path_link::<Memory64>),
        "path_open" => Function::new_typed_with_env(&mut store, env, path_open::<Memory64>),
        "path_readlink" => Function::new_typed_with_env(&mut store, env, path_readlink::<Memory64>),
        "path_remove_directory" => Function::new_typed_with_env(&mut store, env, path_remove_directory::<Memory64>),
        "path_rename" => Function::new_typed_with_env(&mut store, env, path_rename::<Memory64>),
        "path_symlink" => Function::new_typed_with_env(&mut store, env, path_symlink::<Memory64>),
        "path_unlink_file" => Function::new_typed_with_env(&mut store, env, path_unlink_file::<Memory64>),
        "poll_oneoff" => Function::new_typed_with_env(&mut store, env, poll_oneoff::<Memory64>),
        "proc_exit" => Function::new_typed_with_env(&mut store, env, proc_exit::<Memory64>),
        "proc_fork" => Function::new_typed_with_env(&mut store, env, proc_fork::<Memory64>),
        "proc_join" => Function::new_typed_with_env(&mut store, env, proc_join::<Memory64>),
        "proc_signal" => Function::new_typed_with_env(&mut store, env, proc_signal::<Memory64>),
        "proc_exec" => Function::new_typed_with_env(&mut store, env, proc_exec::<Memory64>),
        "proc_raise" => Function::new_typed_with_env(&mut store, env, proc_raise),
        "proc_raise_interval" => Function::new_typed_with_env(&mut store, env, proc_raise_interval),
        "proc_spawn" => Function::new_typed_with_env(&mut store, env, proc_spawn::<Memory64>),
        "proc_id" => Function::new_typed_with_env(&mut store, env, proc_id::<Memory64>),
        "proc_parent" => Function::new_typed_with_env(&mut store, env, proc_parent::<Memory64>),
        "random_get" => Function::new_typed_with_env(&mut store, env, random_get::<Memory64>),
        "tty_get" => Function::new_typed_with_env(&mut store, env, tty_get::<Memory64>),
        "tty_set" => Function::new_typed_with_env(&mut store, env, tty_set::<Memory64>),
        "getcwd" => Function::new_typed_with_env(&mut store, env, getcwd::<Memory64>),
        "chdir" => Function::new_typed_with_env(&mut store, env, chdir::<Memory64>),
        "callback_signal" => Function::new_typed_with_env(&mut store, env, callback_signal::<Memory64>),
        "callback_thread" => Function::new_typed_with_env(&mut store, env, callback_thread::<Memory64>),
        "callback_reactor" => Function::new_typed_with_env(&mut store, env, callback_reactor::<Memory64>),
        "callback_thread_local_destroy" => Function::new_typed_with_env(&mut store, env, callback_thread_local_destroy::<Memory64>),
        "thread_spawn" => Function::new_typed_with_env(&mut store, env, thread_spawn::<Memory64>),
        "thread_local_create" => Function::new_typed_with_env(&mut store, env, thread_local_create::<Memory64>),
        "thread_local_destroy" => Function::new_typed_with_env(&mut store, env, thread_local_destroy),
        "thread_local_set" => Function::new_typed_with_env(&mut store, env, thread_local_set),
        "thread_local_get" => Function::new_typed_with_env(&mut store, env, thread_local_get::<Memory64>),
        "thread_sleep" => Function::new_typed_with_env(&mut store, env, thread_sleep),
        "thread_id" => Function::new_typed_with_env(&mut store, env, thread_id::<Memory64>),
        "thread_signal" => Function::new_typed_with_env(&mut store, env, thread_signal),
        "thread_join" => Function::new_typed_with_env(&mut store, env, thread_join),
        "thread_parallelism" => Function::new_typed_with_env(&mut store, env, thread_parallelism::<Memory64>),
        "thread_exit" => Function::new_typed_with_env(&mut store, env, thread_exit),
        "sched_yield" => Function::new_typed_with_env(&mut store, env, sched_yield),
        "stack_checkpoint" => Function::new_typed_with_env(&mut store, env, stack_checkpoint::<Memory64>),
        "stack_restore" => Function::new_typed_with_env(&mut store, env, stack_restore::<Memory64>),
        "futex_wait" => Function::new_typed_with_env(&mut store, env, futex_wait::<Memory64>),
        "futex_wake" => Function::new_typed_with_env(&mut store, env, futex_wake::<Memory64>),
        "futex_wake_all" => Function::new_typed_with_env(&mut store, env, futex_wake_all::<Memory64>),
        "bus_open_local" => Function::new_typed_with_env(&mut store, env, bus_open_local::<Memory64>),
        "bus_open_remote" => Function::new_typed_with_env(&mut store, env, bus_open_remote::<Memory64>),
        "bus_close" => Function::new_typed_with_env(&mut store, env, bus_close),
        "bus_call" => Function::new_typed_with_env(&mut store, env, bus_call::<Memory64>),
        "bus_subcall" => Function::new_typed_with_env(&mut store, env, bus_subcall::<Memory64>),
        "bus_poll" => Function::new_typed_with_env(&mut store, env, bus_poll::<Memory64>),
        "call_reply" => Function::new_typed_with_env(&mut store, env, call_reply::<Memory64>),
        "call_fault" => Function::new_typed_with_env(&mut store, env, call_fault),
        "call_close" => Function::new_typed_with_env(&mut store, env, call_close),
        "ws_connect" => Function::new_typed_with_env(&mut store, env, ws_connect::<Memory64>),
        "http_request" => Function::new_typed_with_env(&mut store, env, http_request::<Memory64>),
        "http_status" => Function::new_typed_with_env(&mut store, env, http_status::<Memory64>),
        "port_bridge" => Function::new_typed_with_env(&mut store, env, port_bridge::<Memory64>),
        "port_unbridge" => Function::new_typed_with_env(&mut store, env, port_unbridge),
        "port_dhcp_acquire" => Function::new_typed_with_env(&mut store, env, port_dhcp_acquire),
        "port_addr_add" => Function::new_typed_with_env(&mut store, env, port_addr_add::<Memory64>),
        "port_addr_remove" => Function::new_typed_with_env(&mut store, env, port_addr_remove::<Memory64>),
        "port_addr_clear" => Function::new_typed_with_env(&mut store, env, port_addr_clear),
        "port_addr_list" => Function::new_typed_with_env(&mut store, env, port_addr_list::<Memory64>),
        "port_mac" => Function::new_typed_with_env(&mut store, env, port_mac::<Memory64>),
        "port_gateway_set" => Function::new_typed_with_env(&mut store, env, port_gateway_set::<Memory64>),
        "port_route_add" => Function::new_typed_with_env(&mut store, env, port_route_add::<Memory64>),
        "port_route_remove" => Function::new_typed_with_env(&mut store, env, port_route_remove::<Memory64>),
        "port_route_clear" => Function::new_typed_with_env(&mut store, env, port_route_clear),
        "port_route_list" => Function::new_typed_with_env(&mut store, env, port_route_list::<Memory64>),
        "sock_status" => Function::new_typed_with_env(&mut store, env, sock_status::<Memory64>),
        "sock_addr_local" => Function::new_typed_with_env(&mut store, env, sock_addr_local::<Memory64>),
        "sock_addr_peer" => Function::new_typed_with_env(&mut store, env, sock_addr_peer::<Memory64>),
        "sock_open" => Function::new_typed_with_env(&mut store, env, sock_open::<Memory64>),
        "sock_set_opt_flag" => Function::new_typed_with_env(&mut store, env, sock_set_opt_flag),
        "sock_get_opt_flag" => Function::new_typed_with_env(&mut store, env, sock_get_opt_flag::<Memory64>),
        "sock_set_opt_time" => Function::new_typed_with_env(&mut store, env, sock_set_opt_time::<Memory64>),
        "sock_get_opt_time" => Function::new_typed_with_env(&mut store, env, sock_get_opt_time::<Memory64>),
        "sock_set_opt_size" => Function::new_typed_with_env(&mut store, env, sock_set_opt_size),
        "sock_get_opt_size" => Function::new_typed_with_env(&mut store, env, sock_get_opt_size::<Memory64>),
        "sock_join_multicast_v4" => Function::new_typed_with_env(&mut store, env, sock_join_multicast_v4::<Memory64>),
        "sock_leave_multicast_v4" => Function::new_typed_with_env(&mut store, env, sock_leave_multicast_v4::<Memory64>),
        "sock_join_multicast_v6" => Function::new_typed_with_env(&mut store, env, sock_join_multicast_v6::<Memory64>),
        "sock_leave_multicast_v6" => Function::new_typed_with_env(&mut store, env, sock_leave_multicast_v6::<Memory64>),
        "sock_bind" => Function::new_typed_with_env(&mut store, env, sock_bind::<Memory64>),
        "sock_listen" => Function::new_typed_with_env(&mut store, env, sock_listen::<Memory64>),
        "sock_accept" => Function::new_typed_with_env(&mut store, env, sock_accept::<Memory64>),
        "sock_connect" => Function::new_typed_with_env(&mut store, env, sock_connect::<Memory64>),
        "sock_recv" => Function::new_typed_with_env(&mut store, env, sock_recv::<Memory64>),
        "sock_recv_from" => Function::new_typed_with_env(&mut store, env, sock_recv_from::<Memory64>),
        "sock_send" => Function::new_typed_with_env(&mut store, env, sock_send::<Memory64>),
        "sock_send_to" => Function::new_typed_with_env(&mut store, env, sock_send_to::<Memory64>),
        "sock_send_file" => Function::new_typed_with_env(&mut store, env, sock_send_file::<Memory64>),
        "sock_shutdown" => Function::new_typed_with_env(&mut store, env, sock_shutdown),
        "resolve" => Function::new_typed_with_env(&mut store, env, resolve::<Memory64>),
    };
    namespace
}

pub fn import_object_for_all_wasi_versions(
    store: &mut impl AsStoreMut,
    env: &FunctionEnv<WasiEnv>,
) -> Imports {
    let exports_wasi_unstable = wasi_unstable_exports(store, env);
    let exports_wasi_snapshot_preview1 = wasi_snapshot_preview1_exports(store, env);
    let exports_wasix_32v1 = wasix_exports_32(store, env);
    let exports_wasix_64v1 = wasix_exports_64(store, env);
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
    env: &FunctionEnv<WasiEnv>,
) -> Imports {
    let exports_unstable = wasi_unstable_exports(store, env);
    imports! {
        "wasi_unstable" => exports_unstable
    }
}

fn generate_import_object_snapshot1(
    store: &mut impl AsStoreMut,
    env: &FunctionEnv<WasiEnv>,
) -> Imports {
    let exports_wasi_snapshot_preview1 = wasi_snapshot_preview1_exports(store, env);
    imports! {
        "wasi_snapshot_preview1" => exports_wasi_snapshot_preview1
    }
}

/// Combines a state generating function with the import list for snapshot 1
fn generate_import_object_wasix32_v1(
    store: &mut impl AsStoreMut,
    env: &FunctionEnv<WasiEnv>,
) -> Imports {
    let exports_wasix_32v1 = wasix_exports_32(store, env);
    imports! {
        "wasix_32v1" => exports_wasix_32v1
    }
}

fn generate_import_object_wasix64_v1(
    store: &mut impl AsStoreMut,
    env: &FunctionEnv<WasiEnv>,
) -> Imports {
    let exports_wasix_64v1 = wasix_exports_64(store, env);
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
