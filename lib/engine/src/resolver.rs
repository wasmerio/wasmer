//! Custom resolution for external references.

use crate::{Export, ExportFunctionMetadata, ImportError, LinkError};
use more_asserts::assert_ge;
use wasmer_types::entity::{BoxedSlice, EntityRef, PrimaryMap};
use wasmer_types::{ExternType, FunctionIndex, ImportIndex, MemoryIndex, ModuleInfo, TableIndex};

use wasmer_vm::{
    FunctionBodyPtr, ImportFunctionEnv, Imports, MemoryStyle, TableStyle, VMFunctionBody,
    VMFunctionEnvironment, VMFunctionImport, VMFunctionKind, VMGlobalImport, VMMemoryImport,
    VMTableImport,
};

/// Get an `ExternType` given a import index.
fn get_extern_from_import(module: &ModuleInfo, import_index: &ImportIndex) -> ExternType {
    match import_index {
        ImportIndex::Function(index) => {
            let func = module.signatures[module.functions[*index]].clone();
            ExternType::Function(func)
        }
        ImportIndex::Table(index) => {
            let table = module.tables[*index];
            ExternType::Table(table)
        }
        ImportIndex::Memory(index) => {
            let memory = module.memories[*index];
            ExternType::Memory(memory)
        }
        ImportIndex::Global(index) => {
            let global = module.globals[*index];
            ExternType::Global(global)
        }
    }
}

/// Get an `ExternType` given an export (and Engine signatures in case is a function).
fn get_extern_from_export(_module: &ModuleInfo, export: &Export) -> ExternType {
    match export {
        Export::Function(ref f) => ExternType::Function(f.vm_function.signature.clone()),
        Export::Table(ref t) => ExternType::Table(*t.ty()),
        Export::Memory(ref m) => ExternType::Memory(m.ty()),
        Export::Global(ref g) => {
            let global = g.from.ty();
            ExternType::Global(*global)
        }
    }
}

/// This function allows to match all imports of a `ModuleInfo` with concrete definitions provided by
/// a `Resolver`.
///
/// If all imports are satisfied returns an `Imports` instance required for a module instantiation.
pub fn resolve_imports(
    module: &ModuleInfo,
    imports: &[Export],
    finished_dynamic_function_trampolines: &BoxedSlice<FunctionIndex, FunctionBodyPtr>,
    memory_styles: &PrimaryMap<MemoryIndex, MemoryStyle>,
    _table_styles: &PrimaryMap<TableIndex, TableStyle>,
) -> Result<Imports, LinkError> {
    let mut function_imports = PrimaryMap::with_capacity(module.num_imported_functions);
    let mut host_function_env_initializers =
        PrimaryMap::with_capacity(module.num_imported_functions);
    let mut table_imports = PrimaryMap::with_capacity(module.num_imported_tables);
    let mut memory_imports = PrimaryMap::with_capacity(module.num_imported_memories);
    let mut global_imports = PrimaryMap::with_capacity(module.num_imported_globals);

    for ((module_name, field, import_idx), import_index) in module.imports.iter() {
        let import_extern = get_extern_from_import(module, import_index);
        let resolved = if let Some(r) = imports.get(*import_idx as usize) {
            r
        } else {
            return Err(LinkError::Import(
                module_name.to_string(),
                field.to_string(),
                ImportError::UnknownImport(import_extern),
            ));
        };
        let export_extern = get_extern_from_export(module, &resolved);
        if !export_extern.is_compatible_with(&import_extern) {
            return Err(LinkError::Import(
                module_name.to_string(),
                field.to_string(),
                ImportError::IncompatibleType(import_extern, export_extern),
            ));
        }
        match resolved {
            Export::Function(ref f) => {
                let address = match f.vm_function.kind {
                    VMFunctionKind::Dynamic => {
                        // If this is a dynamic imported function,
                        // the address of the function is the address of the
                        // reverse trampoline.
                        let index = FunctionIndex::new(function_imports.len());
                        finished_dynamic_function_trampolines[index].0 as *mut VMFunctionBody as _

                        // TODO: We should check that the f.vmctx actually matches
                        // the shape of `VMDynamicFunctionImportContext`
                    }
                    VMFunctionKind::Static => f.vm_function.address,
                };

                // Clone the host env for this `Instance`.
                let env = if let Some(ExportFunctionMetadata {
                    host_env_clone_fn: clone,
                    ..
                }) = f.metadata.as_deref()
                {
                    // TODO: maybe start adding asserts in all these
                    // unsafe blocks to prevent future changes from
                    // horribly breaking things.
                    unsafe {
                        assert!(!f.vm_function.vmctx.host_env.is_null());
                        (clone)(f.vm_function.vmctx.host_env)
                    }
                } else {
                    // No `clone` function means we're dealing with some
                    // other kind of `vmctx`, not a host env of any
                    // kind.
                    unsafe { f.vm_function.vmctx.host_env }
                };

                function_imports.push(VMFunctionImport {
                    body: address,
                    environment: VMFunctionEnvironment { host_env: env },
                });

                let initializer = f.metadata.as_ref().and_then(|m| m.import_init_function_ptr);
                let clone = f.metadata.as_ref().map(|m| m.host_env_clone_fn);
                let destructor = f.metadata.as_ref().map(|m| m.host_env_drop_fn);
                let import_function_env =
                    if let (Some(clone), Some(destructor)) = (clone, destructor) {
                        ImportFunctionEnv::Env {
                            env,
                            clone,
                            initializer,
                            destructor,
                        }
                    } else {
                        ImportFunctionEnv::NoEnv
                    };

                host_function_env_initializers.push(import_function_env);
            }
            Export::Table(ref t) => match import_index {
                ImportIndex::Table(index) => {
                    let import_table_ty = t.from.ty();
                    let expected_table_ty = &module.tables[*index];
                    if import_table_ty.ty != expected_table_ty.ty {
                        return Err(LinkError::Import(
                            module_name.to_string(),
                            field.to_string(),
                            ImportError::IncompatibleType(import_extern, export_extern),
                        ));
                    }

                    table_imports.push(VMTableImport {
                        definition: t.from.vmtable(),
                        from: t.from.clone(),
                    });
                }
                _ => {
                    unreachable!("Table resolution did not match");
                }
            },
            Export::Memory(ref m) => {
                match import_index {
                    ImportIndex::Memory(index) => {
                        // Sanity-check: Ensure that the imported memory has at least
                        // guard-page protections the importing module expects it to have.
                        let export_memory_style = m.style();
                        let import_memory_style = &memory_styles[*index];
                        if let (
                            MemoryStyle::Static { bound, .. },
                            MemoryStyle::Static {
                                bound: import_bound,
                                ..
                            },
                        ) = (export_memory_style.clone(), &import_memory_style)
                        {
                            assert_ge!(bound, *import_bound);
                        }
                        assert_ge!(
                            export_memory_style.offset_guard_size(),
                            import_memory_style.offset_guard_size()
                        );
                    }
                    _ => {
                        // This should never be reached, as we did compatibility
                        // checks before
                        panic!("Memory resolution didn't matched");
                    }
                }

                memory_imports.push(VMMemoryImport {
                    definition: m.from.vmmemory(),
                    from: m.from.clone(),
                });
            }

            Export::Global(ref g) => {
                global_imports.push(VMGlobalImport {
                    definition: g.from.vmglobal(),
                    from: g.from.clone(),
                });
            }
        }
    }

    Ok(Imports::new(
        function_imports,
        host_function_env_initializers,
        table_imports,
        memory_imports,
        global_imports,
    ))
}
