use std::{
    collections::HashMap,
    sync::{Arc, Barrier},
};

use tracing::trace;
use wasmer::{AsStoreMut, FunctionEnv, Global, Instance, Memory, Table, Tag};

use crate::{WasiEnv, import_object_for_all_wasi_versions};

use super::{
    DlModule, DlOperation, DylinkInfo, InProgressLinkState, InProgressSymbolResolution, LinkError,
    LinkerState, MAIN_MODULE_HANDLE, ModuleHandle, NeededSymbolResolutionKey,
    PartiallyResolvedExport, PendingFunctionResolutionFromLinkerState,
    PendingResolutionsFromLinker, PendingTlsPointer, ResolveError, SymbolResolutionKey,
    SymbolResolutionResult, UnresolvedGlobal, WasiModuleInstanceHandles,
    call_initialization_function, define_integer_global_import, get_tls_base_export,
    set_integer_global,
};

mod exports;
mod imports;
mod table;

pub(super) struct DlInstance {
    pub(super) instance: Instance,
    #[allow(dead_code)]
    pub(super) instance_handles: WasiModuleInstanceHandles,
    pub(super) tls_base: Option<u64>,
}

pub(super) struct PreparedSideFromLinker {
    pub(super) module_handle: ModuleHandle,
    pub(super) instance: Instance,
}

pub(super) struct InstanceGroupState {
    pub(super) main_instance: Option<Instance>,
    pub(super) main_instance_tls_base: Option<u64>,

    pub(super) side_instances: HashMap<ModuleHandle, DlInstance>,

    pub(super) stack_pointer: Global,
    pub(super) memory: Memory,
    pub(super) indirect_function_table: Table,
    pub(super) c_longjmp: Tag,
    pub(super) cpp_exception: Tag,

    // Once the dl_operation_pending flag is set, a barrier is created and broadcast
    // by the instigating group, which others must use to rendezvous with it.
    pub(super) recv_pending_operation_barrier: bus::BusReader<Arc<Barrier>>,
    // The corresponding sender is stored in the shared linker state, and is used
    // by the instigating instance group  to broadcast the results.
    pub(super) recv_pending_operation: bus::BusReader<DlOperation>,
}

// TODO: split further
impl InstanceGroupState {
    fn main_instance(&self) -> Option<&Instance> {
        self.main_instance.as_ref()
    }

    pub(super) fn tls_base(&self, module_handle: ModuleHandle) -> Option<u64> {
        if module_handle == MAIN_MODULE_HANDLE {
            // Main's TLS area is at the beginning of its memory
            self.main_instance_tls_base
        } else {
            self.side_instances
                .get(&module_handle)
                .expect("Internal error: bad module handle")
                .tls_base
        }
    }

    fn try_instance(&self, handle: ModuleHandle) -> Option<&Instance> {
        if handle == MAIN_MODULE_HANDLE {
            self.main_instance.as_ref()
        } else {
            self.side_instances.get(&handle).map(|i| &i.instance)
        }
    }

    fn instance(&self, handle: ModuleHandle) -> &Instance {
        self.try_instance(handle)
            .expect("Internal error: bad module handle or not instantiated in this group")
    }

    pub(super) fn instantiate_side_module_from_link_state(
        &mut self,
        linker_state: &mut LinkerState,
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<WasiEnv>,
        link_state: &mut InProgressLinkState,
        module_handle: ModuleHandle,
    ) -> Result<(), LinkError> {
        let Some(pending_module) = link_state
            .new_modules
            .iter()
            .find(|m| m.handle == module_handle)
        else {
            panic!(
                "Only recently-loaded modules in the link state can be instantiated \
                by instantiate_side_module_from_link_state"
            )
        };

        trace!(
            ?module_handle,
            ?link_state,
            "Instantiating module from link state"
        );

        let memory_base = linker_state.allocate_memory(
            store,
            &self.memory,
            &pending_module.dylink_info.mem_info,
        )?;
        let table_base = self
            .allocate_function_table(
                store,
                pending_module.dylink_info.mem_info.table_size,
                pending_module.dylink_info.mem_info.table_alignment,
            )
            .map_err(LinkError::TableAllocationError)?;

        trace!(
            memory_base,
            table_base, "Allocated memory and table for module"
        );

        let mut imports = import_object_for_all_wasi_versions(&pending_module.module, store, env);

        let well_known_imports = [
            ("env", "__memory_base", memory_base),
            ("env", "__table_base", table_base),
        ];

        let module = pending_module.module.clone();
        let dylink_info = pending_module.dylink_info.clone();

        trace!(?module_handle, "Resolving symbols");
        linker_state.resolve_symbols(
            self,
            store,
            &module,
            module_handle,
            link_state,
            &well_known_imports,
        )?;

        trace!(?module_handle, "Populating imports object");
        self.populate_imports_from_link_state(
            module_handle,
            linker_state,
            link_state,
            store,
            &module,
            &mut imports,
            env,
            &well_known_imports,
        )?;

        let instance = Instance::new(store, &module, &imports)?;

        let instance_handles = WasiModuleInstanceHandles::new(
            self.memory.clone(),
            store,
            instance.clone(),
            Some(self.indirect_function_table.clone()),
        );

        let dl_module = DlModule {
            module,
            dylink_info,
            memory_base,
            table_base,
        };

        let tls_base = get_tls_base_export(&instance, store)?;

        let dl_instance = DlInstance {
            instance: instance.clone(),
            instance_handles,
            // The TLS base of a side module's main instance is read from the module's
            // `__tls_base` export via `get_tls_base_export`, and is not necessarily at the
            // beginning of its memory.
            tls_base,
        };

        linker_state.side_modules.insert(module_handle, dl_module);
        self.side_instances.insert(module_handle, dl_instance);

        trace!(?module_handle, "Module instantiated");

        Ok(())
    }

    pub(super) fn prepare_side_module_from_linker(
        &mut self,
        linker_state: &LinkerState,
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<WasiEnv>,
        module_handle: ModuleHandle,
        pending_resolutions: &mut PendingResolutionsFromLinker,
    ) -> Result<PreparedSideFromLinker, LinkError> {
        if self.side_instances.contains_key(&module_handle) {
            panic!(
                "Internal error: Module with handle {module_handle:?} \
                was already instantiated in this group"
            )
        };

        trace!(?module_handle, "Instantiating existing module from linker");

        let dl_module = linker_state
            .side_modules
            .get(&module_handle)
            .expect("Internal error: module not loaded into linker");

        let mut imports = import_object_for_all_wasi_versions(&dl_module.module, store, env);

        let well_known_imports = [
            ("env", "__memory_base", dl_module.memory_base),
            ("env", "__table_base", dl_module.table_base),
        ];

        trace!(?module_handle, "Populating imports object");
        self.populate_imports_from_linker(
            module_handle,
            linker_state,
            store,
            &dl_module.module,
            &mut imports,
            env,
            &well_known_imports,
            pending_resolutions,
        )?;

        let instance = Instance::new(store, &dl_module.module, &imports)?;

        Ok(PreparedSideFromLinker {
            module_handle,
            instance,
        })
    }

    pub(super) fn complete_side_module_from_linker(
        &mut self,
        prepared: PreparedSideFromLinker,
        tls_base: Option<u64>,
        store: &mut impl AsStoreMut,
    ) -> Result<(), LinkError> {
        let PreparedSideFromLinker {
            module_handle,
            instance,
        } = prepared;

        let instance_handles = WasiModuleInstanceHandles::new(
            self.memory.clone(),
            store,
            instance.clone(),
            Some(self.indirect_function_table.clone()),
        );

        let dl_instance = DlInstance {
            instance: instance.clone(),
            instance_handles,
            tls_base,
        };

        self.side_instances.insert(module_handle, dl_instance);

        // Initialization logic must only be run once, so no init calls here; it is
        // assumed that the module was instantiated and its init callbacks were called
        // by whichever thread first called instantiate_side_module_from_link_state.

        trace!(?module_handle, "Existing module instantiated successfully");

        Ok(())
    }

    // For when we receive a module loaded DL operation
    pub(super) fn instantiate_side_module_from_linker(
        &mut self,
        linker_state: &LinkerState,
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<WasiEnv>,
        module_handle: ModuleHandle,
        pending_resolutions: &mut PendingResolutionsFromLinker,
    ) -> Result<(), LinkError> {
        let prepared = self.prepare_side_module_from_linker(
            linker_state,
            store,
            env,
            module_handle,
            pending_resolutions,
        )?;

        // This is a non-main instance of a side module, so it needs a new TLS area
        let tls_base =
            call_initialization_function::<i32>(&prepared.instance, store, "__wasix_init_tls")?
                .map(|v| v as u64);

        self.complete_side_module_from_linker(prepared, tls_base, store)
    }

    pub(super) fn finalize_pending_resolutions_from_linker(
        &self,
        pending_resolutions: &PendingResolutionsFromLinker,
        store: &mut impl AsStoreMut,
    ) -> Result<(), LinkError> {
        trace!("Finalizing pending functions");

        for pending in &pending_resolutions.functions {
            let func = self
                .instance(pending.resolved_from)
                .exports
                .get_function(&pending.name)
                .unwrap_or_else(|e| {
                    panic!(
                        "Internal error: failed to resolve exported function {}: {e:?}",
                        pending.name
                    )
                });

            self.place_in_function_table_at(store, func.clone(), pending.function_table_index)
                .map_err(LinkError::TableAllocationError)?;

            trace!(?pending, "Placed pending function in table");
        }

        for tls in &pending_resolutions.tls {
            let Some(tls_base) = self.tls_base(tls.resolved_from) else {
                // This is a panic since this error should have been caught when the symbol
                // was originally resolved by the instigating instance group. We're just replaying
                // the changes.
                panic!(
                    "Internal error: Tried to import TLS symbol from module {} that \
                    has no TLS base",
                    tls.resolved_from.0
                );
            };

            let final_addr = tls_base + tls.offset;
            set_integer_global(store, "<pending TLS global>", &tls.global, final_addr)?;
            trace!(?tls, tls_base, final_addr, "Setting pending TLS global");
        }

        Ok(())
    }

    pub(super) fn apply_requested_symbols_from_linker(
        &self,
        store: &mut impl AsStoreMut,
        linker_state: &LinkerState,
    ) -> Result<(), LinkError> {
        for (key, val) in &linker_state.symbol_resolution_records {
            if let SymbolResolutionKey::Requested { name, .. } = key
                && let SymbolResolutionResult::FunctionPointer {
                    resolved_from,
                    function_table_index,
                } = val
            {
                self.apply_resolved_function(store, name, *resolved_from, *function_table_index)?;
            }
        }
        Ok(())
    }

    pub(super) fn apply_dl_operation(
        &mut self,
        linker_state: &LinkerState,
        operation: DlOperation,
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<WasiEnv>,
    ) -> Result<(), LinkError> {
        trace!(?operation, "Applying operation");
        match operation {
            DlOperation::LoadModules(module_handles) => {
                let mut pending_functions = PendingResolutionsFromLinker::default();
                for handle in module_handles {
                    // We need to do table allocation in exactly the same order as the instigating
                    // group, which is:
                    //   * Allocate module's own table space
                    //   * Fill GOT.func entries (through instantiating the module)
                    //   * Then repeat for the next module.
                    self.allocate_function_table_for_existing_module(linker_state, store, handle)?;
                    self.instantiate_side_module_from_linker(
                        linker_state,
                        store,
                        env,
                        handle,
                        &mut pending_functions,
                    )?;
                }
                self.finalize_pending_resolutions_from_linker(&pending_functions, store)?;
            }
            DlOperation::AllocateFunctionTable { index, size } => {
                self.apply_function_table_allocation(store, index, size)?
            }
        };
        trace!("Operation applied successfully");
        Ok(())
    }

    pub(super) fn finalize_pending_globals(
        &self,
        linker_state: &mut LinkerState,
        store: &mut impl AsStoreMut,
        unresolved_globals: &Vec<UnresolvedGlobal>,
    ) -> Result<(), LinkError> {
        trace!("Finalizing pending globals");

        for unresolved in unresolved_globals {
            let key = unresolved.key();
            let import_metadata = &linker_state.dylink_info(key.module_handle).import_metadata;
            let is_weak = import_metadata
                .get(&(key.import_module.to_owned(), key.import_name.to_owned()))
                // clang seems to like putting the import-info in the "env" module
                // sometimes, so try that as well
                .or_else(|| import_metadata.get(&("env".to_owned(), key.import_name.to_owned())))
                .map(|flags| flags.contains(wasmparser::SymbolFlags::BINDING_WEAK))
                .unwrap_or(false);
            trace!(?unresolved, is_weak, "Resolving pending global");

            match (
                unresolved,
                self.resolve_export(linker_state, store, None, &key.import_name, true),
            ) {
                (
                    UnresolvedGlobal::Mem(key, global),
                    Ok((PartiallyResolvedExport::Global(addr), resolved_from)),
                ) => {
                    trace!(
                        ?unresolved,
                        ?resolved_from,
                        addr,
                        "Resolved to memory address"
                    );
                    set_integer_global(store, &key.import_name, global, addr)?;
                    linker_state.symbol_resolution_records.insert(
                        SymbolResolutionKey::Needed(key.clone()),
                        SymbolResolutionResult::Memory(addr),
                    );
                }

                (
                    UnresolvedGlobal::Mem(key, global),
                    Ok((PartiallyResolvedExport::Tls { offset, final_addr }, resolved_from)),
                ) => {
                    trace!(
                        ?unresolved,
                        ?resolved_from,
                        offset,
                        final_addr,
                        "Resolved to TLS address"
                    );
                    set_integer_global(store, &key.import_name, global, final_addr)?;
                    linker_state.symbol_resolution_records.insert(
                        SymbolResolutionKey::Needed(key.clone()),
                        SymbolResolutionResult::Tls {
                            resolved_from,
                            offset,
                        },
                    );
                }

                (
                    UnresolvedGlobal::Func(key, global),
                    Ok((PartiallyResolvedExport::Function(func), resolved_from)),
                ) => {
                    let func_handle = self
                        .append_to_function_table(store, func)
                        .map_err(LinkError::TableAllocationError)?;
                    trace!(
                        ?unresolved,
                        ?resolved_from,
                        function_table_index = ?func_handle,
                        "Resolved to function pointer"
                    );
                    set_integer_global(store, &key.import_name, global, func_handle as u64)?;
                    linker_state.symbol_resolution_records.insert(
                        SymbolResolutionKey::Needed(key.clone()),
                        SymbolResolutionResult::FunctionPointer {
                            resolved_from,
                            function_table_index: func_handle,
                        },
                    );
                }

                // Expected memory address, resolved function or vice-versa
                (_, Ok(_)) => {
                    return Err(LinkError::UnresolvedGlobal(
                        unresolved.import_module().to_string(),
                        key.import_name.clone(),
                        Box::new(ResolveError::MissingExport),
                    ));
                }

                // Missing weak symbols get resolved to a null address
                (_, Err(ResolveError::MissingExport)) if is_weak => {
                    trace!(?unresolved, "Weak global not found");
                    set_integer_global(store, &key.import_name, unresolved.global(), 0)?;
                    linker_state.symbol_resolution_records.insert(
                        SymbolResolutionKey::Needed(key.clone()),
                        SymbolResolutionResult::Memory(0),
                    );
                }

                (_, Err(e)) => {
                    return Err(LinkError::UnresolvedGlobal(
                        "GOT.mem".to_string(),
                        key.import_name.clone(),
                        Box::new(e),
                    ));
                }
            }
        }

        Ok(())
    }
}
