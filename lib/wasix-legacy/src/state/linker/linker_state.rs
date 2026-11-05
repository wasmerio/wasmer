use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Barrier},
};

use tracing::trace;
use virtual_mio::block_on;
use wasmer::{
    AsStoreMut, AsStoreRef, Engine, Extern, ExternType, ImportType, Memory, MemoryError, Module,
    Type,
};

use crate::{Runtime, runtime::module_cache::HashedModuleData};

use super::{
    DlModule, DlModuleSpec, DlOperation, DylinkInfo, INVALID_MODULE_HANDLE, InProgressLinkState,
    InProgressModuleLoad, InProgressSymbolResolution, InstanceGroupState, LinkError,
    MAIN_MODULE_HANDLE, MemoryAllocator, ModuleHandle, NeededSymbolResolutionKey,
    SymbolResolutionKey, SymbolResolutionResult,
};

use super::{get_integer_global_type_from_import, locate_module, parse_dylink0_section};

use crate::state::WasiState;

// There is only one LinkerState for all instance groups
pub(super) struct LinkerState {
    pub(super) engine: Engine,

    pub(super) main_module: Module,
    pub(super) main_module_dylink_info: DylinkInfo,
    pub(super) main_module_memory_base: u64,

    // We used to have an issue where spawning instances out-of-order in new threads
    // would break globals. That has since been fixed. However, spawning in the same
    // order helps with diagnosing potential linker issues, so we're keeping the
    // hack from back then.
    // To ensure the same order, we use a BTreeMap here, which means when we
    // iterate over it, we'll get the modules from lowest handle to highest, and
    // order is preserved.
    pub(super) side_modules: BTreeMap<ModuleHandle, DlModule>,
    pub(super) side_modules_by_name: HashMap<PathBuf, ModuleHandle>,
    pub(super) next_module_handle: u32,

    pub(super) memory_allocator: MemoryAllocator,
    pub(super) heap_base: u64,

    /// Tracks which slots in the function table are currently used for closures
    ///
    /// True if the closure is currently in use, false otherwise.
    pub(super) allocated_closure_functions: BTreeMap<u32, bool>,
    /// Slots in the indirect function table that were allocated for closures but are currently not in use.
    /// These can be given out without needing to lock all threads.
    pub(super) available_closure_functions: Vec<u32>,

    pub(super) symbol_resolution_records: HashMap<SymbolResolutionKey, SymbolResolutionResult>,

    pub(super) send_pending_operation_barrier: bus::Bus<Arc<Barrier>>,
    pub(super) send_pending_operation: bus::Bus<DlOperation>,
}

impl LinkerState {
    pub(super) fn allocate_memory(
        &mut self,
        store: &mut impl AsStoreMut,
        memory: &Memory,
        mem_info: &wasmparser::MemInfo,
    ) -> Result<u64, MemoryError> {
        trace!(?mem_info, "Allocating memory");

        let new_size = if mem_info.memory_size == 0 {
            0
        } else {
            self.memory_allocator.allocate(
                memory,
                store,
                mem_info.memory_size,
                2_u32.pow(mem_info.memory_alignment),
            )? as u64
        };

        trace!(new_size, "Final size");

        Ok(new_size)
    }

    pub(super) fn memory_base(&self, module_handle: ModuleHandle) -> u64 {
        if module_handle == MAIN_MODULE_HANDLE {
            self.main_module_memory_base
        } else {
            self.side_modules
                .get(&module_handle)
                .expect("Internal error: bad module handle")
                .memory_base
        }
    }

    pub(super) fn dylink_info(&self, module_handle: ModuleHandle) -> &DylinkInfo {
        if module_handle == MAIN_MODULE_HANDLE {
            &self.main_module_dylink_info
        } else {
            &self
                .side_modules
                .get(&module_handle)
                .expect("Internal error: bad module handle")
                .dylink_info
        }
    }

    // Resolves all imports for the given module, and places the results into
    // the in progress link state's symbol collection.
    // A follow-up call to [`InstanceGroupState::populate_imports_from_link_state`]
    // is needed to create a usable imports object, which needs to happen once per
    // instance group.
    // Each instance group has a different store, so the group ID corresponding
    // to the given store must be provided to resolve globals from the correct
    // instances.
    pub(super) fn resolve_symbols(
        &self,
        group: &InstanceGroupState,
        store: &mut impl AsStoreMut,
        module: &Module,
        module_handle: ModuleHandle,
        link_state: &mut InProgressLinkState,
        // Used only to "skip over" well known imports, so we don't actually need the
        // u64 values. However, we use the same type as populate_imports to let calling
        // code construct the data only once.
        well_known_imports: &[(&str, &str, u64)],
    ) -> Result<(), LinkError> {
        trace!(?module_handle, "Resolving symbols");
        for import in module.imports() {
            // Skip over well known imports, since they'll be provided externally
            if well_known_imports
                .iter()
                .any(|i| i.0 == import.module() && i.1 == import.name())
            {
                trace!(?import, "Skipping resolution of well-known symbol");
                continue;
            }

            // Skip over the memory, function table and stack pointer imports as well
            match import.name() {
                "memory"
                | "__indirect_function_table"
                | "__stack_pointer"
                | "__c_longjmp"
                | "__cpp_exception" => {
                    trace!(?import, "Skipping resolution of special symbol");
                    continue;
                }
                _ => (),
            }

            match import.module() {
                "env" => {
                    let resolution = self.resolve_env_symbol(group, &import, store)?;
                    trace!(?import, ?resolution, "Symbol resolved");
                    link_state.symbols.insert(
                        NeededSymbolResolutionKey {
                            module_handle,
                            import_module: "env".to_owned(),
                            import_name: import.name().to_string(),
                        },
                        resolution,
                    );
                }
                "GOT.mem" => {
                    let resolution = self.resolve_got_mem_symbol(group, &import, store)?;
                    trace!(?import, ?resolution, "Symbol resolved");
                    link_state.symbols.insert(
                        NeededSymbolResolutionKey {
                            module_handle,
                            import_module: "GOT.mem".to_owned(),
                            import_name: import.name().to_string(),
                        },
                        resolution,
                    );
                }
                "GOT.func" => {
                    let resolution = self.resolve_got_func_symbol(group, &import, store)?;
                    trace!(?import, ?resolution, "Symbol resolved");
                    link_state.symbols.insert(
                        NeededSymbolResolutionKey {
                            module_handle,
                            import_module: "GOT.func".to_owned(),
                            import_name: import.name().to_string(),
                        },
                        resolution,
                    );
                }
                _ => (),
            }
        }

        trace!(?module_handle, "All symbols resolved");

        Ok(())
    }

    // Imports from the env module are:
    //   * the memory and indirect function table
    //   * well-known addresses, such as __stack_pointer and __memory_base
    //   * functions that are imported directly
    // resolve_env_symbol only handles the imported functions.
    fn resolve_env_symbol(
        &self,
        group: &InstanceGroupState,
        import: &ImportType,
        store: &impl AsStoreRef,
    ) -> Result<InProgressSymbolResolution, LinkError> {
        let ExternType::Function(import_func_ty) = import.ty() else {
            return Err(LinkError::ImportMustBeFunction(
                "env",
                import.name().to_string(),
            ));
        };

        let export = group.resolve_exported_symbol(import.name());

        match export {
            Some((module_handle, export)) => {
                let Extern::Function(export_func) = export else {
                    return Err(LinkError::ImportTypeMismatch(
                        "env".to_string(),
                        import.name().to_string(),
                        ExternType::Function(import_func_ty.clone()),
                        export.ty(store).clone(),
                    ));
                };

                if export_func.ty(store) != *import_func_ty {
                    return Err(LinkError::ImportTypeMismatch(
                        "env".to_string(),
                        import.name().to_string(),
                        ExternType::Function(import_func_ty.clone()),
                        export.ty(store).clone(),
                    ));
                }

                Ok(InProgressSymbolResolution::Function(module_handle))
            }
            None => {
                // The function may be exported from a module we have yet to link in,
                // or otherwise not be used by the module at all. We provide a stub that,
                // when called, will try to resolve the symbol and call it. This lets
                // us resolve circular dependencies, as well as letting modules that don't
                // actually use their imports run successfully.
                Ok(InProgressSymbolResolution::StubFunction(
                    import_func_ty.clone(),
                ))
            }
        }
    }

    // "Global" imports (i.e. imports from GOT.mem and GOT.func) are integer globals.
    // GOT.mem imports should point to the address of another module's data.
    fn resolve_got_mem_symbol(
        &self,
        group: &InstanceGroupState,
        import: &ImportType,
        store: &impl AsStoreRef,
    ) -> Result<InProgressSymbolResolution, LinkError> {
        let global_type = get_integer_global_type_from_import(import)?;

        match group.resolve_exported_symbol(import.name()) {
            Some((module_handle, export)) => {
                let ExternType::Global(global_type) = export.ty(store) else {
                    return Err(LinkError::ImportTypeMismatch(
                        "GOT.mem".to_string(),
                        import.name().to_string(),
                        ExternType::Global(global_type),
                        export.ty(store).clone(),
                    ));
                };

                if !matches!(global_type.ty, Type::I32 | Type::I64) {
                    return Err(LinkError::ImportTypeMismatch(
                        "GOT.mem".to_string(),
                        import.name().to_string(),
                        ExternType::Global(global_type),
                        export.ty(store).clone(),
                    ));
                }

                Ok(InProgressSymbolResolution::MemGlobal(module_handle))
            }
            None => Ok(InProgressSymbolResolution::UnresolvedMemGlobal),
        }
    }

    // "Global" imports (i.e. imports from GOT.mem and GOT.func) are integer globals.
    // GOT.func imports are function pointers (i.e. indices into the indirect function
    // table).
    fn resolve_got_func_symbol(
        &self,
        group: &InstanceGroupState,
        import: &ImportType,
        store: &impl AsStoreRef,
    ) -> Result<InProgressSymbolResolution, LinkError> {
        // Ensure the global is the correct type (i32 or i64)
        let _ = get_integer_global_type_from_import(import)?;

        match group.resolve_exported_symbol(import.name()) {
            Some((module_handle, export)) => {
                let ExternType::Function(_) = export.ty(store) else {
                    return Err(LinkError::ExportMustBeFunction(
                        import.name().to_string(),
                        export.ty(store).clone(),
                    ));
                };

                Ok(InProgressSymbolResolution::FuncGlobal(module_handle))
            }
            None => Ok(InProgressSymbolResolution::UnresolvedFuncGlobal),
        }
    }

    // TODO: give loaded library a different wasi env that specifies its module handle
    // This function loads the module (and its needed modules) and puts the resulting `Module`s
    // in the linker state, while assigning handles and putting the handles in the in-progress
    // link state. The modules must then get their symbols resolved and be instantiated in the
    // order in which their handles exist in the link state.
    // Returns the handle of the originally requested module. This will be the last entry in
    // the link state's list of module handles, but only if the module was actually loaded; if
    // it was already loaded, the existing handle is returned.
    pub(super) fn load_module_tree(
        &mut self,
        module_spec: DlModuleSpec,
        link_state: &mut InProgressLinkState,
        runtime: &Arc<dyn Runtime + Send + Sync + 'static>,
        wasi_state: &WasiState,
        runtime_path: &[impl AsRef<str>],
        calling_module_path: Option<impl AsRef<Path>>,
    ) -> Result<ModuleHandle, LinkError> {
        let module_name = match module_spec {
            DlModuleSpec::FileSystem { module_spec, .. } => Cow::Borrowed(module_spec),
            DlModuleSpec::Memory { module_name, .. } => {
                Cow::Owned(PathBuf::from(format!("::in-memory::{module_name}")))
            }
        };
        trace!(?module_name, "Locating and loading module");

        if let Some(handle) = self.side_modules_by_name.get(module_name.as_ref()) {
            let handle = *handle;

            trace!(?module_name, ?handle, "Module was already loaded");

            return Ok(handle);
        }

        // Locate and load the module bytes
        let (module_data, paths) = match module_spec {
            DlModuleSpec::FileSystem {
                module_spec,
                ld_library_path,
            } => {
                let (full_path, bytes) = block_on(locate_module(
                    module_spec,
                    ld_library_path,
                    runtime_path,
                    calling_module_path,
                    &wasi_state.fs,
                ))?;
                // TODO: this can be optimized by detecting early if the module is already
                // pending without loading its bytes
                if link_state.pending_module_paths.contains(&full_path) {
                    trace!("Module is already pending, won't load again");
                    // This is fine, since a non-empty pending_modules list means we are
                    // recursively resolving needed modules. We don't use the handle
                    // returned from this function for anything when running recursively
                    // (see self.load_module call below).
                    return Ok(INVALID_MODULE_HANDLE);
                }

                (
                    HashedModuleData::new(bytes),
                    Some((full_path, ld_library_path)),
                )
            }
            DlModuleSpec::Memory { bytes, .. } => (HashedModuleData::new(bytes), None),
        };

        let module = runtime.load_hashed_module_sync(module_data, Some(&self.engine))?;

        let dylink_info = parse_dylink0_section(&module)?;

        trace!(?dylink_info, "Loading side module");

        if let Some((full_path, ld_library_path)) = paths {
            link_state.pending_module_paths.push(full_path.clone());
            let num_pending_modules = link_state.pending_module_paths.len();
            let pop_pending_module = |link_state: &mut InProgressLinkState| {
                assert_eq!(
                    num_pending_modules,
                    link_state.pending_module_paths.len(),
                    "Internal error: pending modules not maintained correctly"
                );
                link_state.pending_module_paths.pop().unwrap();
            };

            for needed in &dylink_info.needed {
                trace!(needed, "Loading needed side module");
                match self.load_module_tree(
                    DlModuleSpec::FileSystem {
                        module_spec: Path::new(needed.as_str()),
                        ld_library_path,
                    },
                    link_state,
                    runtime,
                    wasi_state,
                    // RUNPATH, on which WASM_DYLINK_RUNTIME_PATH is based, is *not* applied
                    // recursively, so we discard the runtime_path parameter and
                    // only take the one from the module's dylink.0 section
                    dylink_info.runtime_path.as_ref(),
                    Some(&full_path),
                ) {
                    Ok(_) => (),
                    Err(e) => {
                        pop_pending_module(link_state);
                        return Err(e);
                    }
                }
            }

            pop_pending_module(link_state);
        } else if !dylink_info.needed.is_empty() {
            unreachable!(
                "Internal error: in-memory modules with further needed modules not \
                    supported and no code paths can create such a module"
            );
        }

        let handle = ModuleHandle(self.next_module_handle);
        self.next_module_handle += 1;

        trace!(?module_name, ?handle, "Assigned handle to module");

        link_state.new_modules.push(InProgressModuleLoad {
            handle,
            dylink_info,
            module,
        });
        // Put the name in the linker state - the actual DlModule must be
        // constructed later by the instance group once table addresses are
        // allocated for the module.
        // TODO: allocate table here (at least logically)?
        self.side_modules_by_name
            .insert(module_name.into_owned(), handle);

        Ok(handle)
    }
}
