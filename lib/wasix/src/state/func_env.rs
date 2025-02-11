use std::sync::Arc;

use tracing::trace;
use wasmer::{
    AsStoreMut, AsStoreRef, ExportError, FunctionEnv, Imports, Instance, Memory, Module, Store,
};
use wasmer_wasix_types::wasi::ExitCode;

#[allow(unused_imports)]
use crate::os::task::thread::RewindResultType;
#[cfg(feature = "journal")]
use crate::syscalls::restore_snapshot;
use crate::{
    import_object_for_all_wasi_versions,
    runtime::task_manager::SpawnMemoryTypeOrStore,
    state::WasiInstanceHandles,
    utils::{get_wasi_version, get_wasi_versions, store::restore_store_snapshot},
    RewindStateOption, StoreSnapshot, WasiEnv, WasiError, WasiRuntimeError, WasiThreadError,
};

/// The default stack size for WASIX - the number itself is the default that compilers
/// have used in the past when compiling WASM apps.
///
/// (this is only used for programs that have no stack pointer)
const DEFAULT_STACK_SIZE: u64 = 1_048_576u64;
const DEFAULT_STACK_BASE: u64 = DEFAULT_STACK_SIZE;

#[derive(Clone, Debug)]
pub struct WasiFunctionEnv {
    pub env: FunctionEnv<WasiEnv>,
}

impl WasiFunctionEnv {
    pub fn new(store: &mut impl AsStoreMut, env: WasiEnv) -> Self {
        Self {
            env: FunctionEnv::new(store, env),
        }
    }

    // Creates a new environment context on a new store
    pub fn new_with_store(
        module: Module,
        env: WasiEnv,
        store_snapshot: Option<StoreSnapshot>,
        spawn_type: SpawnMemoryTypeOrStore,
        update_layout: bool,
    ) -> Result<(Self, Store), WasiThreadError> {
        // Create a new store and put the memory object in it
        // (but only if it has imported memory)
        let (memory, store): (Option<wasmer::Memory>, Option<wasmer::Store>) = match spawn_type {
            SpawnMemoryTypeOrStore::New => (None, None),
            SpawnMemoryTypeOrStore::Type(mut ty) => {
                ty.shared = true;

                let mut store = env.runtime.new_store();

                // Note: If memory is shared, maximum needs to be set in the
                // browser otherwise creation will fail.
                let _ = ty.maximum.get_or_insert(wasmer_types::Pages::max_value());

                let mem = Memory::new(&mut store, ty).map_err(|err| {
                    tracing::error!(
                        error = &err as &dyn std::error::Error,
                        memory_type=?ty,
                        "could not create memory",
                    );
                    WasiThreadError::MemoryCreateFailed(err)
                })?;
                (Some(mem), Some(store))
            }
            SpawnMemoryTypeOrStore::StoreAndMemory(s, m) => (m, Some(s)),
        };

        let mut store = store.unwrap_or_else(|| env.runtime().new_store());

        // Build the context object and import the memory
        let mut ctx = WasiFunctionEnv::new(&mut store, env);
        let (mut import_object, init) =
            import_object_for_all_wasi_versions(&module, &mut store, &ctx.env);
        if let Some(memory) = memory.clone() {
            import_object.define("env", "memory", memory);
        }

        let instance = Instance::new(&mut store, &module, &import_object).map_err(|err| {
            tracing::warn!("failed to create instance - {}", err);
            WasiThreadError::InstanceCreateFailed(Box::new(err))
        })?;

        init(&instance, &store).map_err(|err| {
            tracing::warn!("failed to init instance - {}", err);
            WasiThreadError::InitFailed(Arc::new(err))
        })?;

        // Initialize the WASI environment
        ctx.initialize_with_memory(&mut store, instance, memory, update_layout)
            .map_err(|err| {
                tracing::warn!("failed initialize environment - {}", err);
                WasiThreadError::ExportError(err)
            })?;

        // Set all the globals
        if let Some(snapshot) = store_snapshot {
            restore_store_snapshot(&mut store, &snapshot);
        }

        Ok((ctx, store))
    }

    /// Get an `Imports` for a specific version of WASI detected in the module.
    pub fn import_object(
        &self,
        store: &mut impl AsStoreMut,
        module: &Module,
    ) -> Result<Imports, WasiError> {
        let wasi_version = get_wasi_version(module, false).ok_or(WasiError::UnknownWasiVersion)?;
        Ok(crate::generate_import_object_from_env(
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
        instance: Instance,
    ) -> Result<(), ExportError> {
        self.initialize_with_memory(store, instance, None, true)
    }

    /// Initializes the WasiEnv using the instance exports and a provided optional memory
    /// (this must be executed before attempting to use it)
    /// (as the stores can not by themselves be passed between threads we can store the module
    ///  in a thread-local variables and use it later - for multithreading)
    pub fn initialize_with_memory(
        &mut self,
        store: &mut impl AsStoreMut,
        instance: Instance,
        memory: Option<Memory>,
        update_layout: bool,
    ) -> Result<(), ExportError> {
        let is_wasix_module = crate::utils::is_wasix_module(instance.module());

        let exported_memory = instance
            .exports
            .iter()
            .filter_map(|(_, export)| {
                if let wasmer::Extern::Memory(memory) = export {
                    Some(memory.clone())
                } else {
                    None
                }
            })
            .next();
        let memory = match (exported_memory, memory) {
            (Some(memory), _) => memory,
            (None, Some(memory)) => memory,
            (None, None) => {
                return Err(ExportError::Missing(
                    "No imported or exported memory found".to_string(),
                ))
            }
        };

        let new_inner = WasiInstanceHandles::new(memory, store, instance);

        let stack_pointer = new_inner.stack_pointer.clone();
        let data_end = new_inner.data_end.clone();
        let stack_low = new_inner.stack_low.clone();
        let stack_high = new_inner.stack_high.clone();

        let env = self.data_mut(store);
        env.set_inner(new_inner);

        env.state.fs.set_is_wasix(is_wasix_module);

        // If the stack offset and size is not set then do so
        if update_layout {
            // Set the base stack
            let stack_base = if let Some(stack_high) = stack_high {
                match stack_high.get(store) {
                    wasmer::Value::I32(a) => a as u64,
                    wasmer::Value::I64(a) => a as u64,
                    _ => DEFAULT_STACK_BASE,
                }
            } else if let Some(stack_pointer) = stack_pointer {
                match stack_pointer.get(store) {
                    wasmer::Value::I32(a) => a as u64,
                    wasmer::Value::I64(a) => a as u64,
                    _ => DEFAULT_STACK_BASE,
                }
            } else {
                DEFAULT_STACK_BASE
            };

            if stack_base == 0 {
                return Err(ExportError::Missing(
                    "stack_high or stack_pointer is not set to the upper stack range".to_string(),
                ));
            }

            let mut stack_lower = if let Some(stack_low) = stack_low {
                match stack_low.get(store) {
                    wasmer::Value::I32(a) => a as u64,
                    wasmer::Value::I64(a) => a as u64,
                    _ => 0,
                }
            } else if let Some(data_end) = data_end {
                let data_end = match data_end.get(store) {
                    wasmer::Value::I32(a) => a as u64,
                    wasmer::Value::I64(a) => a as u64,
                    _ => 0,
                };
                // It's possible for the data section to be above the stack, we check for that here and
                // if it is, we'll assume the stack starts at address 0
                if data_end >= stack_base {
                    0
                } else {
                    data_end
                }
            } else {
                // clang-16 and higher generate the `__stack_low` global, and it can be exported with
                // `-Wl,--export=__stack_low`. clang-15 generates `__data_end`, which should be identical
                // and can be exported if `__stack_low` is not available.
                if self.data(store).will_use_asyncify() {
                    tracing::warn!("Missing both __stack_low and __data_end exports, unwinding may cause memory corruption");
                }
                0
            };

            if stack_lower >= stack_base {
                if self.data(store).will_use_asyncify() {
                    tracing::warn!(
                        "Detected lower end of stack to be above higher end, ignoring stack_lower; \
                        unwinding may cause memory corruption"
                    );
                }
                stack_lower = 0;
            }

            // Update the stack layout which is need for asyncify
            let env = self.data_mut(store);
            let tid = env.tid();
            let layout = &mut env.layout;
            layout.stack_upper = stack_base;
            layout.stack_lower = stack_lower;
            layout.stack_size = layout.stack_upper - layout.stack_lower;

            // Replace the thread object itself
            env.thread.set_memory_layout(layout.clone());

            // Replace the thread object with this new layout
            {
                let mut guard = env.process.lock();
                guard
                    .threads
                    .values_mut()
                    .filter(|t| t.tid() == tid)
                    .for_each(|t| t.set_memory_layout(layout.clone()))
            }
        }
        tracing::trace!("initializing with layout {:?}", self.data(store).layout);

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
            let new_import_object =
                crate::generate_import_object_from_env(store, &self.env, *version);
            for ((n, m), e) in new_import_object.into_iter() {
                resolver.define(&n, &m, e);
            }
        }

        Ok(resolver)
    }

    /// # Safety
    ///
    /// This function should only be called from within a syscall
    /// as it can potentially execute local thread variable cleanup
    /// code
    pub fn on_exit(&self, store: &mut impl AsStoreMut, process_exit_code: Option<ExitCode>) {
        trace!(
            "wasi[{}:{}]::on_exit",
            self.data(store).pid(),
            self.data(store).tid()
        );

        // Cleans up all the open files (if this is the main thread)
        self.data(store).blocking_on_exit(process_exit_code);
    }

    /// Bootstraps this main thread and context with any journals that
    /// may be present
    ///
    /// # Safety
    ///
    /// This function manipulates the memory of the process and thus must be executed
    /// by the WASM process thread itself.
    ///
    #[allow(clippy::result_large_err)]
    #[allow(unused_variables, unused_mut)]
    #[tracing::instrument(skip_all)]
    pub unsafe fn bootstrap(
        &self,
        mut store: &'_ mut impl AsStoreMut,
    ) -> Result<RewindStateOption, WasiRuntimeError> {
        tracing::debug!("bootstrap start");

        #[allow(unused_mut)]
        let mut rewind_state = None;

        #[cfg(feature = "journal")]
        {
            // If there are journals we need to restore then do so (this will
            // prevent the initialization function from running
            let restore_journals = self.data(&store).runtime.journals().clone();
            if !restore_journals.is_empty() {
                tracing::trace!("replaying journal=true");
                self.data_mut(&mut store).replaying_journal = true;

                for journal in restore_journals {
                    let ctx = self.env.clone().into_mut(&mut store);
                    let rewind = match restore_snapshot(ctx, journal, true) {
                        Ok(r) => r,
                        Err(err) => {
                            tracing::trace!("replaying journal=false (err={:?})", err);
                            self.data_mut(&mut store).replaying_journal = false;
                            return Err(err);
                        }
                    };
                    rewind_state = rewind.map(|rewind| (rewind, RewindResultType::RewindRestart));
                }

                tracing::trace!("replaying journal=false");
                self.data_mut(&mut store).replaying_journal = false;
            }

            // If there is no rewind state then the journal is being replayed
            // and hence we do not need to write an init module event
            //
            // But otherwise we need to notify the journal of the module hash
            // so that recompiled modules will restart
            if rewind_state.is_none() {
                // The first event we save is an event that records the module hash.
                // Note: This is used to detect if an incorrect journal is used on the wrong
                // process or if a process has been recompiled
                let wasm_hash = Box::from(self.data(&store).process.module_hash.as_bytes());
                let mut ctx = self.env.clone().into_mut(&mut store);
                crate::journal::JournalEffector::save_event(
                    &mut ctx,
                    crate::journal::JournalEntry::InitModuleV1 { wasm_hash },
                )
                .map_err(|err| {
                    WasiRuntimeError::Runtime(wasmer::RuntimeError::new(format!(
                        "journal failed to save the module initialization event - {err}"
                    )))
                })?;
            } else {
                // Otherwise we should emit a clear ethereal event
                let mut ctx = self.env.clone().into_mut(&mut store);
                crate::journal::JournalEffector::save_event(
                    &mut ctx,
                    crate::journal::JournalEntry::ClearEtherealV1,
                )
                .map_err(|err| {
                    WasiRuntimeError::Runtime(wasmer::RuntimeError::new(format!(
                        "journal failed to save clear ethereal event - {err}",
                    )))
                })?;
            }
        }

        tracing::debug!("bootstrap complete");

        Ok(rewind_state)
    }
}
