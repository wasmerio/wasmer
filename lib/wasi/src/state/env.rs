use std::{
    ops::Deref,
    sync::{Arc, RwLockReadGuard, RwLockWriteGuard},
};

use derivative::Derivative;
use tracing::{trace, warn};
use wasmer::{
    AsStoreMut, AsStoreRef, Exports, Global, Instance, Memory, MemoryView, Module, TypedFunction,
};
use wasmer_vbus::{SpawnEnvironmentIntrinsics, VirtualBus};
use wasmer_vnet::DynVirtualNetworking;
use wasmer_wasi_types::{
    types::Signal,
    wasi::{Errno, ExitCode, Snapshot0Clockid},
};

use crate::{
    bin_factory,
    bin_factory::BinFactory,
    fs::WasiInodes,
    os::{
        command::builtins::cmd_wasmer::CmdWasmer,
        task::{
            process::{WasiProcess, WasiProcessId},
            thread::{WasiThread, WasiThreadHandle, WasiThreadId},
        },
    },
    syscalls::platform_clock_time_get,
    PluggableRuntimeImplementation, VirtualTaskManager, WasiError, WasiRuntimeImplementation,
    WasiState, WasiStateCreationError, WasiVFork, DEFAULT_STACK_SIZE,
};

#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct WasiEnvInner {
    /// Represents a reference to the memory
    pub(crate) memory: Memory,
    /// Represents the module that is being used (this is NOT send/sync)
    /// however the code itself makes sure that it is used in a safe way
    pub(crate) module: Module,
    /// All the exports for the module
    pub(crate) exports: Exports,
    //// Points to the current location of the memory stack pointer
    pub(crate) stack_pointer: Option<Global>,
    /// Main function that will be invoked (name = "_start")
    #[derivative(Debug = "ignore")]
    // TODO: review allow...
    #[allow(dead_code)]
    pub(crate) start: Option<TypedFunction<(), ()>>,
    /// Function thats invoked to initialize the WASM module (nane = "_initialize")
    #[derivative(Debug = "ignore")]
    // TODO: review allow...
    #[allow(dead_code)]
    pub(crate) initialize: Option<TypedFunction<(), ()>>,
    /// Represents the callback for spawning a thread (name = "_start_thread")
    /// (due to limitations with i64 in browsers the parameters are broken into i32 pairs)
    /// [this takes a user_data field]
    #[derivative(Debug = "ignore")]
    pub(crate) thread_spawn: Option<TypedFunction<(i32, i32), ()>>,
    /// Represents the callback for spawning a reactor (name = "_react")
    /// (due to limitations with i64 in browsers the parameters are broken into i32 pairs)
    /// [this takes a user_data field]
    #[derivative(Debug = "ignore")]
    pub(crate) react: Option<TypedFunction<(i32, i32), ()>>,
    /// Represents the callback for signals (name = "__wasm_signal")
    /// Signals are triggered asynchronously at idle times of the process
    #[derivative(Debug = "ignore")]
    pub(crate) signal: Option<TypedFunction<i32, ()>>,
    /// Flag that indicates if the signal callback has been set by the WASM
    /// process - if it has not been set then the runtime behaves differently
    /// when a CTRL-C is pressed.
    pub(crate) signal_set: bool,
    /// Represents the callback for destroying a local thread variable (name = "_thread_local_destroy")
    /// [this takes a pointer to the destructor and the data to be destroyed]
    #[derivative(Debug = "ignore")]
    pub(crate) thread_local_destroy: Option<TypedFunction<(i32, i32, i32, i32), ()>>,
    /// asyncify_start_unwind(data : i32): call this to start unwinding the
    /// stack from the current location. "data" must point to a data
    /// structure as described above (with fields containing valid data).
    #[derivative(Debug = "ignore")]
    // TODO: review allow...
    #[allow(dead_code)]
    pub(crate) asyncify_start_unwind: Option<TypedFunction<i32, ()>>,
    /// asyncify_stop_unwind(): call this to note that unwinding has
    /// concluded. If no other code will run before you start to rewind,
    /// this is not strictly necessary, however, if you swap between
    /// coroutines, or even just want to run some normal code during a
    /// "sleep", then you must call this at the proper time. Otherwise,
    /// the code will think it is still unwinding when it should not be,
    /// which means it will keep unwinding in a meaningless way.
    #[derivative(Debug = "ignore")]
    // TODO: review allow...
    #[allow(dead_code)]
    pub(crate) asyncify_stop_unwind: Option<TypedFunction<(), ()>>,
    /// asyncify_start_rewind(data : i32): call this to start rewinding the
    /// stack vack up to the location stored in the provided data. This prepares
    /// for the rewind; to start it, you must call the first function in the
    /// call stack to be unwound.
    #[derivative(Debug = "ignore")]
    // TODO: review allow...
    #[allow(dead_code)]
    pub(crate) asyncify_start_rewind: Option<TypedFunction<i32, ()>>,
    /// asyncify_stop_rewind(): call this to note that rewinding has
    /// concluded, and normal execution can resume.
    #[derivative(Debug = "ignore")]
    // TODO: review allow...
    #[allow(dead_code)]
    pub(crate) asyncify_stop_rewind: Option<TypedFunction<(), ()>>,
    /// asyncify_get_state(): call this to get the current value of the
    /// internal "__asyncify_state" variable as described above.
    /// It can be used to distinguish between unwinding/rewinding and normal
    /// calls, so that you know when to start an asynchronous operation and
    /// when to propagate results back.
    #[allow(dead_code)]
    #[derivative(Debug = "ignore")]
    pub(crate) asyncify_get_state: Option<TypedFunction<(), i32>>,
}

impl WasiEnvInner {
    pub fn new(
        module: Module,
        memory: Memory,
        store: &impl AsStoreRef,
        instance: &Instance,
    ) -> Self {
        WasiEnvInner {
            module,
            memory,
            exports: instance.exports.clone(),
            stack_pointer: instance
                .exports
                .get_global("__stack_pointer")
                .map(|a| a.clone())
                .ok(),
            start: instance.exports.get_typed_function(store, "_start").ok(),
            initialize: instance
                .exports
                .get_typed_function(store, "_initialize")
                .ok(),
            thread_spawn: instance
                .exports
                .get_typed_function(store, "_start_thread")
                .ok(),
            react: instance.exports.get_typed_function(store, "_react").ok(),
            signal: instance
                .exports
                .get_typed_function(store, "__wasm_signal")
                .ok(),
            signal_set: false,
            asyncify_start_unwind: instance
                .exports
                .get_typed_function(store, "asyncify_start_unwind")
                .ok(),
            asyncify_stop_unwind: instance
                .exports
                .get_typed_function(store, "asyncify_stop_unwind")
                .ok(),
            asyncify_start_rewind: instance
                .exports
                .get_typed_function(store, "asyncify_start_rewind")
                .ok(),
            asyncify_stop_rewind: instance
                .exports
                .get_typed_function(store, "asyncify_stop_rewind")
                .ok(),
            asyncify_get_state: instance
                .exports
                .get_typed_function(store, "asyncify_get_state")
                .ok(),
            thread_local_destroy: instance
                .exports
                .get_typed_function(store, "_thread_local_destroy")
                .ok(),
        }
    }
}

/// The code itself makes safe use of the struct so multiple threads don't access
/// it (without this the JS code prevents the reference to the module from being stored
/// which is needed for the multithreading mode)
unsafe impl Send for WasiEnvInner {}

unsafe impl Sync for WasiEnvInner {}

/// The environment provided to the WASI imports.
#[derive(Debug, Clone)]
pub struct WasiEnv {
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
    pub bin_factory: BinFactory,
    /// Inner functions and references that are loaded before the environment starts
    pub inner: Option<WasiEnvInner>,
    /// List of the handles that are owned by this context
    /// (this can be used to ensure that threads own themselves or others)
    pub owned_handles: Vec<WasiThreadHandle>,
    /// Implementation of the WASI runtime.
    pub runtime: Arc<dyn WasiRuntimeImplementation + Send + Sync + 'static>,
    /// Task manager used to spawn threads and manage the ASYNC runtime
    pub tasks: Arc<dyn VirtualTaskManager + Send + Sync + 'static>,
}

// FIXME: remove unsafe impls!
// Added because currently WasiEnv can hold a wasm_bindgen::JsValue via wasmer::Module.
#[cfg(feature = "js")]
unsafe impl Send for WasiEnv {}
#[cfg(feature = "js")]
unsafe impl Sync for WasiEnv {}

impl WasiEnv {
    /// Forking the WasiState is used when either fork or vfork is called
    pub fn fork(&self) -> (Self, WasiThreadHandle) {
        let process = self.process.compute.new_process();
        let handle = process.new_thread();

        let thread = handle.as_thread();
        thread.copy_stack_from(&self.thread);

        let state = Arc::new(self.state.fork(true));

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
                bin_factory,
                state,
                inner: None,
                owned_handles: Vec::new(),
                runtime: self.runtime.clone(),
                tasks: self.tasks.clone(),
            },
            handle,
        )
    }

    pub fn pid(&self) -> WasiProcessId {
        self.process.pid()
    }

    pub fn tid(&self) -> WasiThreadId {
        self.thread.tid()
    }
}

impl WasiEnv {
    pub fn new(
        state: WasiState,
        compiled_modules: Arc<bin_factory::ModuleCache>,
        process: WasiProcess,
        thread: WasiThreadHandle,
    ) -> Self {
        let state = Arc::new(state);
        let runtime = Arc::new(PluggableRuntimeImplementation::default());
        Self::new_ext(state, compiled_modules, process, thread, runtime)
    }

    pub fn new_ext(
        state: Arc<WasiState>,
        compiled_modules: Arc<bin_factory::ModuleCache>,
        process: WasiProcess,
        thread: WasiThreadHandle,
        runtime: Arc<dyn WasiRuntimeImplementation + Send + Sync>,
    ) -> Self {
        let bin_factory = BinFactory::new(state.clone(), compiled_modules, runtime.clone());
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
            bin_factory,
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

    /// Porcesses any signals that are batched up or any forced exit codes
    pub fn process_signals_and_exit(
        &self,
        store: &mut impl AsStoreMut,
    ) -> Result<Result<bool, Errno>, WasiError> {
        // If a signal handler has never been set then we need to handle signals
        // differently
        if self.inner().signal_set == false {
            if let Ok(signals) = self.thread.pop_signals_or_subscribe() {
                let signal_cnt = signals.len();
                for sig in signals {
                    if sig == Signal::Sigint || sig == Signal::Sigquit || sig == Signal::Sigkill {
                        self.thread.terminate(Errno::Intr as u32);
                        return Err(WasiError::Exit(Errno::Intr as u32));
                    } else {
                        trace!("wasi[{}]::signal-ignored: {:?}", self.pid(), sig);
                    }
                }
                return Ok(Ok(signal_cnt > 0));
            } else {
                return Ok(Ok(false));
            }
        }

        // Check for forced exit
        if let Some(forced_exit) = self.should_exit() {
            return Err(WasiError::Exit(forced_exit));
        }

        Ok(self.process_signals(store)?)
    }

    /// Porcesses any signals that are batched up
    pub fn process_signals(
        &self,
        store: &mut impl AsStoreMut,
    ) -> Result<Result<bool, Errno>, WasiError> {
        // If a signal handler has never been set then we need to handle signals
        // differently
        if self.inner().signal_set == false {
            if self
                .thread
                .has_signal(&[Signal::Sigint, Signal::Sigquit, Signal::Sigkill])
            {
                self.thread.terminate(Errno::Intr as u32);
            }
            return Ok(Ok(false));
        }

        // Check for any signals that we need to trigger
        // (but only if a signal handler is registered)
        if let Some(handler) = self.inner().signal.clone() {
            if let Ok(mut signals) = self.thread.pop_signals_or_subscribe() {
                // We might also have signals that trigger on timers
                let mut now = 0;
                let has_signal_interval = {
                    let mut any = false;
                    let inner = self.process.inner.read().unwrap();
                    if inner.signal_intervals.is_empty() == false {
                        now = platform_clock_time_get(Snapshot0Clockid::Monotonic, 1_000_000)
                            .unwrap() as u128;
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
                    tracing::trace!("wasi[{}]::processing-signal: {:?}", self.pid(), signal);
                    if let Err(err) = handler.call(store, signal as i32) {
                        match err.downcast::<WasiError>() {
                            Ok(wasi_err) => {
                                warn!(
                                    "wasi[{}]::signal handler wasi error - {}",
                                    self.pid(),
                                    wasi_err
                                );
                                return Err(wasi_err);
                            }
                            Err(runtime_err) => {
                                warn!(
                                    "wasi[{}]::signal handler runtime error - {}",
                                    self.pid(),
                                    runtime_err
                                );
                                return Err(WasiError::Exit(Errno::Intr as ExitCode));
                            }
                        }
                    }
                }
            }
        }
        Ok(Ok(true))
    }

    /// Returns an exit code if the thread or process has been forced to exit
    pub fn should_exit(&self) -> Option<u32> {
        // Check for forced exit
        if let Some(forced_exit) = self.thread.try_join() {
            return Some(forced_exit);
        }
        if let Some(forced_exit) = self.process.try_join() {
            return Some(forced_exit);
        }
        None
    }

    /// Accesses the virtual networking implementation
    pub fn net<'a>(&'a self) -> DynVirtualNetworking {
        self.runtime.networking()
    }

    /// Accesses the virtual bus implementation
    pub fn bus<'a>(&'a self) -> Arc<dyn VirtualBus<WasiEnv> + Send + Sync + 'static> {
        self.runtime.bus()
    }

    /// Providers safe access to the initialized part of WasiEnv
    /// (it must be initialized before it can be used)
    pub fn inner(&self) -> &WasiEnvInner {
        self.inner
            .as_ref()
            .expect("You must initialize the WasiEnv before using it")
    }

    /// Providers safe access to the initialized part of WasiEnv
    /// (it must be initialized before it can be used)
    pub fn inner_mut(&mut self) -> &mut WasiEnvInner {
        self.inner
            .as_mut()
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

    pub fn uses<'a, I>(&self, uses: I) -> Result<(), WasiStateCreationError>
    where
        I: IntoIterator<Item = String>,
    {
        // Load all the containers that we inherit from
        #[allow(unused_imports)]
        use std::path::Path;
        use std::{
            borrow::Cow,
            collections::{HashMap, VecDeque},
        };

        #[allow(unused_imports)]
        use wasmer_vfs::FileSystem;

        use crate::fs::WasiFsRoot;

        let mut already: HashMap<String, Cow<'static, str>> = HashMap::new();

        let mut use_packages = uses.into_iter().collect::<VecDeque<_>>();

        let cmd_wasmer = self
            .bin_factory
            .commands
            .get("/bin/wasmer")
            .and_then(|cmd| cmd.as_any().downcast_ref::<CmdWasmer>());

        while let Some(use_package) = use_packages.pop_back() {
            if let Some(package) = cmd_wasmer
                .as_ref()
                .and_then(|cmd| cmd.get_package(use_package.clone(), self.tasks.deref()))
            {
                // If its already been added make sure the version is correct
                let package_name = package.package_name.to_string();
                if let Some(version) = already.get(&package_name) {
                    if version.as_ref() != package.version.as_ref() {
                        return Err(WasiStateCreationError::WasiInheritError(format!(
                            "webc package version conflict for {} - {} vs {}",
                            use_package, version, package.version
                        )));
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
                                tracing::debug!(
                                    "failed to add package [{}] command [{}] - {}",
                                    use_package,
                                    command.name,
                                    err
                                );
                                continue;
                            }

                            // Add the binary package to the bin factory (zero copy the atom)
                            let mut package = package.clone();
                            package.entry = Some(command.atom.clone());
                            self.bin_factory
                                .set_binary(path.as_os_str().to_string_lossy().as_ref(), package);
                        }
                    }
                } else {
                    return Err(WasiStateCreationError::WasiInheritError(format!(
                        "failed to add package as the file system is not sandboxed"
                    )));
                }
            } else {
                return Err(WasiStateCreationError::WasiInheritError(format!(
                    "failed to fetch webc package for {}",
                    use_package
                )));
            }
        }
        Ok(())
    }

    #[cfg(feature = "sys")]
    pub fn map_commands(
        &self,
        map_commands: std::collections::HashMap<String, std::path::PathBuf>,
    ) -> Result<(), WasiStateCreationError> {
        // Load all the mapped atoms
        #[allow(unused_imports)]
        use std::path::Path;

        #[allow(unused_imports)]
        use wasmer_vfs::FileSystem;

        use crate::fs::WasiFsRoot;

        #[cfg(feature = "sys")]
        for (command, target) in map_commands.iter() {
            // Read the file
            let file = std::fs::read(target).map_err(|err| {
                WasiStateCreationError::WasiInheritError(format!(
                    "failed to read local binary [{}] - {}",
                    target.as_os_str().to_string_lossy(),
                    err
                ))
            })?;
            let file: std::borrow::Cow<'static, [u8]> = file.into();

            if let WasiFsRoot::Sandbox(root_fs) = &self.state.fs.root_fs {
                let _ = root_fs.create_dir(Path::new("/bin"));

                let path = format!("/bin/{}", command);
                let path = Path::new(path.as_str());
                if let Err(err) = root_fs.new_open_options_ext().insert_ro_file(path, file) {
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

    /// Cleans up all the open files (if this is the main thread)
    pub fn cleanup(&self, exit_code: Option<ExitCode>) {
        // If this is the main thread then also close all the files
        if self.thread.is_main() {
            trace!("wasi[{}]:: cleaning up open file handles", self.pid());

            let inodes = self.state.inodes.read().unwrap();
            self.state.fs.close_all(inodes.deref());

            // Now send a signal that the thread is terminated
            self.process.signal_process(Signal::Sigquit);

            // Terminate the process
            let exit_code = exit_code.unwrap_or(Errno::Canceled as ExitCode);
            self.process.terminate(exit_code);
        }
    }
}

impl SpawnEnvironmentIntrinsics for WasiEnv {
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
