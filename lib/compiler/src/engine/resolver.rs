//! Custom resolution for external references.

use crate::LinkError;
use more_asserts::assert_ge;
use wasmer_types::{
    entity::{BoxedSlice, EntityRef, PrimaryMap},
    TagIndex, TagKind,
};
use wasmer_types::{
    ExternType, FunctionIndex, ImportError, ImportIndex, MemoryIndex, ModuleInfo, TableIndex,
    TagType,
};

use wasmer_vm::{
    FunctionBodyPtr, Imports, InternalStoreHandle, LinearMemory, MemoryStyle, StoreObjects,
    TableStyle, VMExtern, VMFunctionBody, VMFunctionImport, VMFunctionKind, VMGlobalImport,
    VMMemoryImport, VMTableImport, VMTag,
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
        ImportIndex::Tag(index) => {
            let func = module.signatures[module.tags[*index]].clone();
            ExternType::Tag(TagType::from_fn_type(
                wasmer_types::TagKind::Exception,
                func,
            ))
        }
    }
}

/// Get an `ExternType` given an export (and Engine signatures in case is a function).
fn get_extern_type(context: &StoreObjects, extern_: &VMExtern) -> ExternType {
    match extern_ {
        VMExtern::Tag(f) => ExternType::Tag(wasmer_types::TagType::from_fn_type(
            wasmer_types::TagKind::Exception,
            f.get(context).signature.clone(),
        )),
        VMExtern::Function(f) => ExternType::Function(f.get(context).signature.clone()),
        VMExtern::Table(t) => ExternType::Table(*t.get(context).ty()),
        VMExtern::Memory(m) => ExternType::Memory(m.get(context).ty()),
        VMExtern::Global(g) => {
            let global = g.get(context).ty();
            ExternType::Global(*global)
        }
    }
}

fn get_runtime_size(context: &StoreObjects, extern_: &VMExtern) -> Option<u32> {
    match extern_ {
        VMExtern::Table(t) => Some(t.get(context).get_runtime_size()),
        VMExtern::Memory(m) => Some(m.get(context).get_runtime_size()),
        _ => None,
    }
}

/// This function allows to match all imports of a `ModuleInfo` with concrete definitions provided by
/// a `Resolver`, except for tags which are resolved separately through `resolve_tags`.
///
/// If all imports are satisfied returns an `Imports` instance required for a module instantiation.
#[allow(clippy::result_large_err)]
pub fn resolve_imports(
    module: &ModuleInfo,
    imports: &[VMExtern],
    context: &mut StoreObjects,
    finished_dynamic_function_trampolines: &BoxedSlice<FunctionIndex, FunctionBodyPtr>,
    memory_styles: &PrimaryMap<MemoryIndex, MemoryStyle>,
    _table_styles: &PrimaryMap<TableIndex, TableStyle>,
) -> Result<Imports, LinkError> {
    let mut function_imports = PrimaryMap::with_capacity(module.num_imported_functions);
    let mut table_imports = PrimaryMap::with_capacity(module.num_imported_tables);
    let mut memory_imports = PrimaryMap::with_capacity(module.num_imported_memories);
    let mut global_imports = PrimaryMap::with_capacity(module.num_imported_globals);

    for (import_key, import_index) in module
        .imports
        .iter()
        .filter(|(_, import_index)| !matches!(import_index, ImportIndex::Tag(_)))
    {
        let ResolvedImport {
            resolved,
            import_extern,
            extern_type,
        } = resolve_import(module, imports, context, import_key, import_index)?;
        match *resolved {
            VMExtern::Function(handle) => {
                let f = handle.get_mut(context);
                let address = match f.kind {
                    VMFunctionKind::Dynamic => {
                        // If this is a dynamic imported function,
                        // the address of the function is the address of the
                        // reverse trampoline.
                        let index = FunctionIndex::new(function_imports.len());
                        let ptr = finished_dynamic_function_trampolines[index].0
                            as *mut VMFunctionBody as _;
                        // The logic is currently handling the "resolution" of dynamic imported functions at instantiation time.
                        // However, ideally it should be done even before then, as you may have dynamic imported functions that
                        // are linked at runtime and not instantiation time. And those will not work properly with the current logic.
                        // Ideally, this logic should be done directly in the `wasmer-vm` crate.
                        // TODO (@syrusakbary): Get rid of `VMFunctionKind`
                        unsafe { f.anyfunc.as_ptr().as_mut() }.func_ptr = ptr;
                        ptr
                    }
                    VMFunctionKind::Static => unsafe { f.anyfunc.as_ptr().as_ref().func_ptr },
                };

                function_imports.push(VMFunctionImport {
                    body: address,
                    environment: unsafe { f.anyfunc.as_ptr().as_ref().vmctx },
                    handle,
                });
            }
            VMExtern::Table(handle) => {
                let t = handle.get(context);
                match import_index {
                    ImportIndex::Table(index) => {
                        let import_table_ty = t.ty();
                        let expected_table_ty = &module.tables[*index];
                        if import_table_ty.ty != expected_table_ty.ty {
                            return Err(LinkError::Import(
                                import_key.module.to_string(),
                                import_key.field.to_string(),
                                ImportError::IncompatibleType(import_extern, extern_type),
                            ));
                        }

                        table_imports.push(VMTableImport {
                            definition: t.vmtable(),
                            handle,
                        });
                    }
                    _ => {
                        unreachable!("Table resolution did not match");
                    }
                }
            }
            VMExtern::Memory(handle) => {
                let m = handle.get(context);
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
                        ) = (export_memory_style, &import_memory_style)
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
                    definition: m.vmmemory(),
                    handle,
                });
            }

            VMExtern::Global(handle) => {
                let g = handle.get(context);
                global_imports.push(VMGlobalImport {
                    definition: g.vmglobal(),
                    handle,
                });
            }

            VMExtern::Tag(_) => unreachable!("We already filtered tags out"),
        }
    }

    Ok(Imports::new(
        function_imports,
        table_imports,
        memory_imports,
        global_imports,
    ))
}

/// This function resolves all tags of a `ModuleInfo`. Imported tags are resolved from
/// the `StoreObjects`, whereas local tags are created and pushed to it. This is because
/// we need every tag to have a unique `VMSharedTagIndex` in the `StoreObjects`, regardless
/// of whether it's local or imported, so that exception handling can correctly resolve
/// cross-module exceptions.
// TODO: I feel this code can be cleaned up. Maybe we can handle tag indices better, so we don't have to search through the imports again?
// TODO: don't we create store handles for everything else as well? Should tags get special handling here?
#[allow(clippy::result_large_err)]
pub fn resolve_tags(
    module: &ModuleInfo,
    imports: &[VMExtern],
    context: &mut StoreObjects,
) -> Result<BoxedSlice<TagIndex, InternalStoreHandle<VMTag>>, LinkError> {
    let mut tags = PrimaryMap::with_capacity(module.tags.len());

    for (import_key, import_index) in module
        .imports
        .iter()
        .filter(|(_, import_index)| matches!(import_index, ImportIndex::Tag(_)))
    {
        let ResolvedImport {
            resolved,
            import_extern,
            extern_type,
        } = resolve_import(module, imports, context, import_key, import_index)?;
        match *resolved {
            VMExtern::Tag(handle) => {
                let t = handle.get(context);
                match import_index {
                    ImportIndex::Tag(index) => {
                        let import_tag_ty = &t.signature;
                        let expected_tag_ty = if let Some(expected_tag_ty) =
                            module.signatures.get(module.tags[*index])
                        {
                            expected_tag_ty
                        } else {
                            return Err(LinkError::Resource(format!(
                                "Could not find matching signature for tag index {index:?}"
                            )));
                        };
                        if *import_tag_ty != *expected_tag_ty {
                            return Err(LinkError::Import(
                                import_key.module.to_string(),
                                import_key.field.to_string(),
                                ImportError::IncompatibleType(import_extern, extern_type),
                            ));
                        }

                        tags.push(handle);
                    }
                    _ => {
                        unreachable!("Tag resolution did not match");
                    }
                }
            }
            _ => unreachable!("We already filtered everything else out"),
        }
    }

    // Now, create local tags.
    // Local tags are created in the StoreObjects once per instance, so that
    // when two instances of the same module are executing, they don't end
    // up catching each other's exceptions.
    for (tag_index, signature_index) in module.tags.iter() {
        if module.is_imported_tag(tag_index) {
            continue;
        }
        let sig_ty = if let Some(sig_ty) = module.signatures.get(*signature_index) {
            sig_ty
        } else {
            return Err(LinkError::Resource(format!(
                "Could not find matching signature for tag index {tag_index:?}"
            )));
        };
        let handle =
            InternalStoreHandle::new(context, VMTag::new(TagKind::Exception, sig_ty.clone()));
        tags.push(handle);
    }

    Ok(tags.into_boxed_slice())
}

struct ResolvedImport<'a> {
    resolved: &'a VMExtern,
    import_extern: ExternType,
    extern_type: ExternType,
}

#[allow(clippy::result_large_err)]
fn resolve_import<'a>(
    module: &ModuleInfo,
    imports: &'a [VMExtern],
    context: &mut StoreObjects,
    import: &wasmer_types::ImportKey,
    import_index: &ImportIndex,
) -> Result<ResolvedImport<'a>, LinkError> {
    let import_extern = get_extern_from_import(module, import_index);
    let resolved = if let Some(r) = imports.get(import.import_idx as usize) {
        r
    } else {
        return Err(LinkError::Import(
            import.module.to_string(),
            import.field.to_string(),
            ImportError::UnknownImport(import_extern),
        ));
    };
    let extern_type = get_extern_type(context, resolved);
    let runtime_size = get_runtime_size(context, resolved);
    if !extern_type.is_compatible_with(&import_extern, runtime_size) {
        return Err(LinkError::Import(
            import.module.to_string(),
            import.field.to_string(),
            ImportError::IncompatibleType(import_extern, extern_type),
        ));
    }
    Ok(ResolvedImport {
        resolved,
        import_extern,
        extern_type,
    })
}
