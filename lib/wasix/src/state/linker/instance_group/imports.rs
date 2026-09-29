use std::sync::{Mutex, TryLockError};

use tracing::trace;
use wasmer::{
    AsStoreMut, Extern, ExternType, Function, FunctionEnv, FunctionEnvMut, FunctionType,
    ImportType, Imports, Module, RuntimeError, Type, Value,
};

use crate::{WasiEnv, WasiError, flatten_runtime_error};

use super::{
    InProgressLinkState, InProgressSymbolResolution, InstanceGroupState, LinkError, LinkerState,
    ModuleHandle, NeededSymbolResolutionKey, PartiallyResolvedExport,
    PendingFunctionResolutionFromLinkerState, PendingResolutionsFromLinker, PendingTlsPointer,
    ResolveError, SymbolResolutionKey, SymbolResolutionResult, UnresolvedGlobal,
    define_integer_global_import,
};

#[derive(Clone, Copy)]
enum MemoryImportMode {
    GrowToMinimum,
    DefineOnly,
}

impl InstanceGroupState {
    // This function populates the imports object for a single module from the given
    // in-progress link state.
    pub(in crate::state::linker) fn populate_imports_from_link_state(
        &self,
        module_handle: ModuleHandle,
        linker_state: &mut LinkerState,
        link_state: &mut InProgressLinkState,
        store: &mut impl AsStoreMut,
        module: &Module,
        imports: &mut Imports,
        env: &FunctionEnv<WasiEnv>,
        well_known_imports: &[(&str, &str, u64)],
    ) -> Result<(), LinkError> {
        trace!(?module_handle, "Populating imports object from link state");

        for import in module.imports() {
            // Skip non-DL-related import modules
            if !matches!(import.module(), "env" | "GOT.mem" | "GOT.func") {
                continue;
            }

            if self.define_common_import(
                module_handle,
                store,
                &import,
                imports,
                well_known_imports,
                MemoryImportMode::GrowToMinimum,
            )? {
                continue;
            }

            let key = NeededSymbolResolutionKey {
                module_handle,
                import_module: import.module().to_owned(),
                import_name: import.name().to_owned(),
            };

            // Finally, go through the resolution results
            let resolution = link_state.symbols.get(&key).unwrap_or_else(|| {
                panic!(
                    "Internal error: missing import resolution '{0}'.{1}",
                    key.import_module, key.import_name
                )
            });

            trace!(?module_handle, ?import, ?resolution, "Resolution");

            match resolution {
                InProgressSymbolResolution::Function(module_handle) => {
                    let func = self
                        .instance(*module_handle)
                        .exports
                        .get_function(import.name())
                        .expect("Internal error: bad in-progress symbol resolution");
                    imports.define(import.module(), import.name(), func.clone());
                    linker_state.symbol_resolution_records.insert(
                        SymbolResolutionKey::Needed(key.clone()),
                        SymbolResolutionResult::Function {
                            ty: func.ty(store),
                            resolved_from: *module_handle,
                        },
                    );
                }

                InProgressSymbolResolution::StubFunction(func_ty) => {
                    let func = self.generate_stub_function(
                        store,
                        func_ty,
                        env,
                        module_handle,
                        import.name().to_string(),
                    );
                    imports.define(import.module(), import.name(), func);
                    linker_state.symbol_resolution_records.insert(
                        SymbolResolutionKey::Needed(key.clone()),
                        SymbolResolutionResult::StubFunction(func_ty.clone()),
                    );
                }

                InProgressSymbolResolution::MemGlobal(module_handle) => {
                    let export = match self.resolve_export_from(
                        store,
                        *module_handle,
                        import.name(),
                        self.instance(*module_handle),
                        linker_state.dylink_info(*module_handle),
                        linker_state.memory_base(*module_handle),
                        self.tls_base(*module_handle),
                        true,
                    ) {
                        Ok(export) => export,
                        Err(ResolveError::NoTlsBaseGlobalExport) => {
                            return Err(LinkError::MissingTlsBaseExport(
                                import.name().to_string(),
                                *module_handle,
                            ));
                        }
                        Err(e) => {
                            panic!("Internal error: bad in-progress symbol resolution: {e:?}")
                        }
                    };

                    match export {
                        PartiallyResolvedExport::Global(addr) => {
                            trace!(?module_handle, ?import, addr, "Memory address");

                            let global =
                                define_integer_global_import(store, &import, addr).unwrap();

                            imports.define(import.module(), import.name(), global);
                            linker_state.symbol_resolution_records.insert(
                                SymbolResolutionKey::Needed(key.clone()),
                                SymbolResolutionResult::Memory(addr),
                            );
                        }

                        PartiallyResolvedExport::Tls { offset, final_addr } => {
                            trace!(?module_handle, ?import, offset, final_addr, "TLS address");

                            let global =
                                define_integer_global_import(store, &import, final_addr).unwrap();

                            imports.define(import.module(), import.name(), global);
                            linker_state.symbol_resolution_records.insert(
                                SymbolResolutionKey::Needed(key.clone()),
                                SymbolResolutionResult::Tls {
                                    resolved_from: *module_handle,
                                    offset,
                                },
                            );
                        }

                        PartiallyResolvedExport::Function(_) => {
                            panic!("Internal error: bad in-progress symbol resolution")
                        }
                    }
                }

                InProgressSymbolResolution::UnresolvedMemGlobal => {
                    let global = define_integer_global_import(store, &import, 0).unwrap();
                    imports.define(import.module(), import.name(), global.clone());

                    link_state
                        .unresolved_globals
                        .push(UnresolvedGlobal::Mem(key, global));
                }

                InProgressSymbolResolution::FuncGlobal(module_handle) => {
                    let func = self
                        .instance(*module_handle)
                        .exports
                        .get_function(import.name())
                        .expect("Internal error: bad in-progress symbol resolution");

                    let func_handle = self
                        .append_to_function_table(store, func.clone())
                        .map_err(LinkError::TableAllocationError)?;
                    trace!(
                        ?module_handle,
                        ?import,
                        index = func_handle,
                        "Allocated function table index"
                    );
                    let global =
                        define_integer_global_import(store, &import, func_handle as u64).unwrap();

                    imports.define(import.module(), import.name(), global);
                    linker_state.symbol_resolution_records.insert(
                        SymbolResolutionKey::Needed(key.clone()),
                        SymbolResolutionResult::FunctionPointer {
                            resolved_from: *module_handle,
                            function_table_index: func_handle,
                        },
                    );
                }

                InProgressSymbolResolution::UnresolvedFuncGlobal => {
                    let global = define_integer_global_import(store, &import, 0).unwrap();
                    imports.define(import.module(), import.name(), global.clone());

                    link_state
                        .unresolved_globals
                        .push(UnresolvedGlobal::Func(key, global));
                }
            }
        }

        trace!(?module_handle, "Imports object populated successfully");

        Ok(())
    }

    // For when we receive a module loaded DL operation
    pub(in crate::state::linker) fn populate_imports_from_linker(
        &self,
        module_handle: ModuleHandle,
        linker_state: &LinkerState,
        store: &mut impl AsStoreMut,
        module: &Module,
        imports: &mut Imports,
        env: &FunctionEnv<WasiEnv>,
        well_known_imports: &[(&str, &str, u64)],
        pending_resolutions: &mut PendingResolutionsFromLinker,
    ) -> Result<(), LinkError> {
        trace!(
            ?module_handle,
            "Populating imports object for existing module from linker state"
        );

        for import in module.imports() {
            // Skip non-DL-related import modules
            if !matches!(import.module(), "env" | "GOT.mem" | "GOT.func") {
                continue;
            }

            if self.define_common_import(
                module_handle,
                store,
                &import,
                imports,
                well_known_imports,
                MemoryImportMode::DefineOnly,
            )? {
                continue;
            }

            let key = SymbolResolutionKey::Needed(NeededSymbolResolutionKey {
                module_handle,
                import_module: import.module().to_owned(),
                import_name: import.name().to_owned(),
            });

            // Finally, go through the resolution results
            let resolution = linker_state
                .symbol_resolution_records
                .get(&key)
                .unwrap_or_else(|| {
                    panic!(
                        "Internal error: missing symbol resolution record for '{0}'.{1}",
                        import.module(),
                        import.name()
                    )
                });

            trace!(?module_handle, ?import, ?resolution, "Resolution");

            match resolution {
                SymbolResolutionResult::Function { ty, resolved_from } => {
                    let func = match self.try_instance(*resolved_from) {
                        Some(instance) => {
                            trace!(
                                ?module_handle,
                                ?import,
                                ?resolved_from,
                                "Already have instance to resolve from"
                            );
                            instance
                                .exports
                                .get_function(import.name())
                                .expect("Internal error: failed to get exported function")
                                .clone()
                        }
                        // We may be loading a module tree, and the instance from which
                        // we're supposed to import the function may not exist yet, so
                        // we add in a stub, which will later use the resolution records
                        // to locate the function.
                        None => {
                            trace!(
                                ?module_handle,
                                ?import,
                                ?resolved_from,
                                "Don't have instance yet"
                            );

                            self.generate_stub_function(
                                store,
                                ty,
                                env,
                                module_handle,
                                import.name().to_owned(),
                            )
                        }
                    };
                    imports.define(import.module(), import.name(), func);
                }
                SymbolResolutionResult::StubFunction(ty) => {
                    let func = self.generate_stub_function(
                        store,
                        ty,
                        env,
                        module_handle,
                        import.name().to_owned(),
                    );
                    imports.define(import.module(), import.name(), func.clone());
                }
                SymbolResolutionResult::FunctionPointer {
                    resolved_from,
                    function_table_index,
                } => {
                    let func = self.try_instance(*resolved_from).map(|instance| {
                        instance
                            .exports
                            .get_function(import.name())
                            .unwrap_or_else(|e| {
                                panic!(
                                    "Internal error: failed to resolve function {}: {e:?}",
                                    import.name()
                                )
                            })
                    });
                    match func {
                        Some(func) => {
                            trace!(
                                ?module_handle,
                                ?import,
                                function_table_index,
                                "Placing function pointer into table"
                            );
                            self.place_in_function_table_at(
                                store,
                                func.clone(),
                                *function_table_index,
                            )
                            .map_err(LinkError::TableAllocationError)?;
                        }
                        None => {
                            trace!(
                                ?module_handle,
                                ?import,
                                function_table_index,
                                "Don't have instance yet, creating a pending function"
                            );
                            // Since we know the final value of the global, we can create it
                            // and just fill the function table in later
                            pending_resolutions.functions.push(
                                PendingFunctionResolutionFromLinkerState {
                                    resolved_from: *resolved_from,
                                    name: import.name().to_string(),
                                    function_table_index: *function_table_index,
                                },
                            );
                        }
                    };
                    let global =
                        define_integer_global_import(store, &import, *function_table_index as u64)?;
                    imports.define(import.module(), import.name(), global);
                }
                SymbolResolutionResult::Memory(addr) => {
                    let global = define_integer_global_import(store, &import, *addr)?;
                    imports.define(import.module(), import.name(), global);
                }
                SymbolResolutionResult::Tls {
                    resolved_from,
                    offset,
                } => {
                    let global = define_integer_global_import(store, &import, 0)?;
                    pending_resolutions.tls.push(PendingTlsPointer {
                        global: global.clone(),
                        resolved_from: *resolved_from,
                        offset: *offset,
                    });
                    imports.define(import.module(), import.name(), global);
                }
            }
        }

        Ok(())
    }

    fn define_common_import(
        &self,
        module_handle: ModuleHandle,
        store: &mut impl AsStoreMut,
        import: &ImportType,
        imports: &mut Imports,
        well_known_imports: &[(&str, &str, u64)],
        memory_import_mode: MemoryImportMode,
    ) -> Result<bool, LinkError> {
        // Important env imports first.
        if import.module() == "env" {
            match import.name() {
                "memory" => {
                    let ExternType::Memory(memory_ty) = import.ty() else {
                        return Err(LinkError::BadImport(
                            import.module().to_string(),
                            import.name().to_string(),
                            import.ty().clone(),
                        ));
                    };
                    trace!(?module_handle, ?import, "Main memory");

                    if matches!(memory_import_mode, MemoryImportMode::GrowToMinimum) {
                        // Make sure the memory is big enough for the module being instantiated.
                        let current_size = self.memory.grow(store, 0)?;
                        if current_size < memory_ty.minimum {
                            self.memory.grow(store, memory_ty.minimum - current_size)?;
                        }
                    }

                    imports.define(
                        import.module(),
                        import.name(),
                        Extern::Memory(self.memory.clone()),
                    );
                    return Ok(true);
                }
                "__indirect_function_table" => {
                    if !matches!(import.ty(), ExternType::Table(ty) if ty.ty == Type::FuncRef) {
                        return Err(LinkError::BadImport(
                            import.module().to_string(),
                            import.name().to_string(),
                            import.ty().clone(),
                        ));
                    }
                    trace!(?module_handle, ?import, "Function table");
                    imports.define(
                        import.module(),
                        import.name(),
                        Extern::Table(self.indirect_function_table.clone()),
                    );
                    return Ok(true);
                }
                "__stack_pointer" => {
                    if !matches!(import.ty(), ExternType::Global(ty) if *ty == self.stack_pointer.ty(store))
                    {
                        return Err(LinkError::BadImport(
                            import.module().to_string(),
                            import.name().to_string(),
                            import.ty().clone(),
                        ));
                    }
                    trace!(?module_handle, ?import, "Stack pointer");
                    imports.define(
                        import.module(),
                        import.name(),
                        Extern::Global(self.stack_pointer.clone()),
                    );
                    return Ok(true);
                }
                // Clang generates this symbol when building modules that use EH-based sjlj.
                "__c_longjmp" => {
                    if !matches!(import.ty(), ExternType::Tag(ty) if *ty.params == [Type::I32]) {
                        return Err(LinkError::BadImport(
                            import.module().to_string(),
                            import.name().to_string(),
                            import.ty().clone(),
                        ));
                    }
                    trace!(?module_handle, ?import, "setjmp/longjmp exception tag");
                    imports.define(import.module(), import.name(), self.c_longjmp.clone());
                    return Ok(true);
                }
                // Clang generates this symbol when building C++ code that uses exception handling.
                "__cpp_exception" => {
                    if !matches!(import.ty(), ExternType::Tag(ty) if *ty.params == [Type::I32]) {
                        return Err(LinkError::BadImport(
                            import.module().to_string(),
                            import.name().to_string(),
                            import.ty().clone(),
                        ));
                    }
                    trace!(?module_handle, ?import, "C++ exception tag");
                    imports.define(import.module(), import.name(), self.cpp_exception.clone());
                    return Ok(true);
                }
                _ => (),
            }
        }

        if let Some(well_known_value) = well_known_imports.iter().find_map(|i| {
            if i.0 == import.module() && i.1 == import.name() {
                Some(i.2)
            } else {
                None
            }
        }) {
            trace!(
                ?module_handle,
                ?import,
                well_known_value,
                "Well-known value"
            );
            imports.define(
                import.module(),
                import.name(),
                define_integer_global_import(store, import, well_known_value)?,
            );
            return Ok(true);
        }

        Ok(false)
    }

    fn generate_stub_function(
        &self,
        store: &mut impl AsStoreMut,
        ty: &FunctionType,
        env: &FunctionEnv<WasiEnv>,
        requesting_module: ModuleHandle,
        name: String,
    ) -> Function {
        // TODO: only search through needed modules for the symbol. This requires the implementation
        // of needing/needed relationships between modules.
        trace!(?requesting_module, name, "Generating stub function");

        let ty = ty.clone();
        let resolved: Mutex<Option<Option<Function>>> = Mutex::new(None);

        Function::new_with_env(
            store,
            env,
            ty.clone(),
            move |mut env: FunctionEnvMut<'_, WasiEnv>, params: &[Value]| {
                let mk_error = || {
                    RuntimeError::user(Box::new(WasiError::DlSymbolResolutionFailed(name.clone())))
                };

                let mut resolved_guard = resolved.lock().unwrap();
                let func = match *resolved_guard {
                    None => {
                        trace!(?requesting_module, name, "Resolving stub function");

                        let (data, store) = env.data_and_store_mut();
                        let env_inner = data.inner();
                        // Safe to unwrap since we already know we're doing DL
                        let linker = env_inner.linker().unwrap();

                        // Best-effort lock only: stubs can run during cross-module init while
                        // another group holds the linker write lock. Cooperative write would block
                        // here; if we can't lock, we resolve but skip recording for other groups.
                        let linker_state = match linker.shared.try_write_linker_state() {
                            Ok(guard) => {
                                trace!(
                                    ?requesting_module,
                                    name, "Locked linker state successfully"
                                );
                                Some(guard)
                            }
                            Err(TryLockError::WouldBlock) => {
                                trace!(?requesting_module, name, "Failed to lock linker state");
                                None
                            }
                            Err(TryLockError::Poisoned(_)) => {
                                *resolved_guard = Some(None);
                                return Err(mk_error());
                            }
                        };

                        let group_guard = linker.instance_group_state.lock().unwrap();
                        let Some(group_state) = group_guard.as_ref() else {
                            trace!(?requesting_module, name, "Instance group is already dead");
                            *resolved_guard = Some(None);
                            return Err(mk_error());
                        };

                        let resolution_key =
                            SymbolResolutionKey::Needed(NeededSymbolResolutionKey {
                                module_handle: requesting_module,
                                import_module: "env".to_owned(),
                                import_name: name.clone(),
                            });

                        match linker_state
                            .as_ref()
                            .and_then(|l| l.symbol_resolution_records.get(&resolution_key))
                        {
                            Some(SymbolResolutionResult::Function {
                                resolved_from,
                                ty: resolved_ty,
                            }) => {
                                trace!(
                                    ?requesting_module,
                                    name, "Function was already resolved in the linker"
                                );

                                if ty != *resolved_ty {
                                    *resolved_guard = Some(None);
                                    return Err(mk_error());
                                }

                                let func = group_state
                                    .instance(*resolved_from)
                                    .exports
                                    .get_function(&name)
                                    .unwrap()
                                    .clone();
                                *resolved_guard = Some(Some(func.clone()));
                                func
                            }
                            Some(SymbolResolutionResult::StubFunction(_)) | None => {
                                trace!(?requesting_module, name, "Resolving function");

                                let Some((resolved_from, export)) =
                                    group_state.resolve_exported_symbol(name.as_str())
                                else {
                                    trace!(?requesting_module, name, "Failed to resolve symbol");
                                    *resolved_guard = Some(None);
                                    return Err(mk_error());
                                };
                                let Extern::Function(func) = export else {
                                    trace!(
                                        ?requesting_module,
                                        name,
                                        ?resolved_from,
                                        "Resolved symbol is not a function"
                                    );
                                    *resolved_guard = Some(None);
                                    return Err(mk_error());
                                };
                                if func.ty(&store) != ty {
                                    trace!(
                                        ?requesting_module,
                                        name,
                                        ?resolved_from,
                                        "Resolved function has bad type"
                                    );
                                    *resolved_guard = Some(None);
                                    return Err(mk_error());
                                }

                                trace!(
                                    ?requesting_module,
                                    name,
                                    ?resolved_from,
                                    "Function resolved successfully"
                                );

                                // Only store the result if we can also put it in the linker's
                                // resolution records for other groups to find.
                                if let Some(mut linker_state) = linker_state {
                                    trace!(
                                        ?requesting_module,
                                        name,
                                        ?resolved_from,
                                        "Updating linker state with this resolution"
                                    );

                                    *resolved_guard = Some(Some(func.clone()));
                                    linker_state.symbol_resolution_records.insert(
                                        resolution_key,
                                        SymbolResolutionResult::Function {
                                            ty: func.ty(&store),
                                            resolved_from,
                                        },
                                    );
                                }

                                func.clone()
                            }
                            Some(resolution) => panic!(
                                "Internal error: resolution record for symbol \
                                {name} indicates non-function resolution {resolution:?}"
                            ),
                        }
                    }
                    Some(None) => return Err(mk_error()),
                    Some(Some(ref func)) => func.clone(),
                };
                drop(resolved_guard);

                let mut store = env.as_store_mut();
                func.call(&mut store, params)
                    .map(|ret| ret.into())
                    .map_err(flatten_runtime_error)
            },
        )
    }
}
