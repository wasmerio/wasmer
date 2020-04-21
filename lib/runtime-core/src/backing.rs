use crate::{
    error::{CreationError, LinkError, LinkResult},
    export::{Context, Export},
    global::Global,
    import::ImportObject,
    memory::Memory,
    module::{ImportName, ModuleInfo, ModuleInner},
    sig_registry::SigRegistry,
    structures::{BoxedMap, Map, SliceMap, TypedIndex},
    table::Table,
    typed_func::{always_trap, Func},
    types::{
        ImportedFuncIndex, ImportedGlobalIndex, ImportedMemoryIndex, ImportedTableIndex,
        Initializer, LocalFuncIndex, LocalGlobalIndex, LocalMemoryIndex, LocalOrImport,
        LocalTableIndex, SigIndex, Value,
    },
    vm,
};
use std::{fmt::Debug, ptr::NonNull, slice};

/// Size of the array for internal instance usage
pub const INTERNALS_SIZE: usize = 256;

pub(crate) struct Internals(pub(crate) [u64; INTERNALS_SIZE]);

impl Debug for Internals {
    fn fmt(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        write!(formatter, "Internals({:?})", &self.0[..])
    }
}

/// The `LocalBacking` "owns" the memory used by all the local resources of an Instance.
/// That is, local memories, tables, and globals (as well as some additional
/// data for the virtual call machinery).
#[derive(Debug)]
pub struct LocalBacking {
    /// This is a map from the local resource index to actual memory,
    /// table, and globals.
    pub(crate) memories: BoxedMap<LocalMemoryIndex, Memory>,
    pub(crate) tables: BoxedMap<LocalTableIndex, Table>,
    pub(crate) globals: BoxedMap<LocalGlobalIndex, Global>,

    /// This own the memory containing the pointers to the local memories.
    /// While simplifying implementation, this adds indirection and may hurt
    /// performance, especially on cache-starved systems.
    pub(crate) vm_memories: BoxedMap<LocalMemoryIndex, *mut vm::LocalMemory>,
    pub(crate) vm_tables: BoxedMap<LocalTableIndex, *mut vm::LocalTable>,
    pub(crate) vm_globals: BoxedMap<LocalGlobalIndex, *mut vm::LocalGlobal>,

    /// The dynamic sigindices are used to efficiently support caching and
    /// the `call_indirect` wasm instruction. This field (and local_functions
    /// as well) are subject to change.
    pub(crate) dynamic_sigindices: BoxedMap<SigIndex, vm::SigId>,
    pub(crate) local_functions: BoxedMap<LocalFuncIndex, *const vm::Func>,

    pub(crate) internals: Internals,
}

// Manually implemented because LocalBacking contains raw pointers directly
unsafe impl Send for LocalBacking {}

impl LocalBacking {
    pub(crate) fn new(
        module: &ModuleInner,
        imports: &ImportBacking,
        vmctx: *mut vm::Ctx,
    ) -> LinkResult<Self> {
        let mut memories = match Self::generate_memories(module) {
            Ok(m) => m,
            Err(e) => {
                return Err(vec![LinkError::Generic {
                    message: format!("unable to create memory: {:?}", e),
                }]);
            }
        };
        let mut tables = Self::generate_tables(module);
        let mut globals = Self::generate_globals(module, imports)?;

        // Ensure all initializers are valid before running finalizers
        Self::validate_memories(module, imports)?;
        Self::validate_tables(module, imports, &mut tables)?;

        let vm_memories = Self::finalize_memories(module, imports, &mut memories)?;
        let vm_tables = Self::finalize_tables(module, imports, &mut tables, vmctx)?;
        let vm_globals = Self::finalize_globals(&mut globals);

        let dynamic_sigindices = Self::generate_sigindices(&module.info);
        let local_functions = Self::generate_local_functions(module);

        Ok(Self {
            memories,
            tables,
            globals,

            vm_memories,
            vm_tables,
            vm_globals,

            dynamic_sigindices,
            local_functions,

            internals: Internals([0; INTERNALS_SIZE]),
        })
    }

    fn generate_local_functions(module: &ModuleInner) -> BoxedMap<LocalFuncIndex, *const vm::Func> {
        (0..module.info.func_assoc.len() - module.info.imported_functions.len())
            .map(|index| {
                module
                    .runnable_module
                    .get_func(&module.info, LocalFuncIndex::new(index))
                    .unwrap()
                    .as_ptr() as *const _
            })
            .collect::<Map<_, _>>()
            .into_boxed_map()
    }

    fn generate_sigindices(info: &ModuleInfo) -> BoxedMap<SigIndex, vm::SigId> {
        info.signatures
            .iter()
            .map(|(_, signature)| {
                let signature = SigRegistry.lookup_signature_ref(signature);
                let sig_index = SigRegistry.lookup_sig_index(signature);
                vm::SigId(sig_index.index() as u32)
            })
            .collect::<Map<_, _>>()
            .into_boxed_map()
    }

    fn generate_memories(
        module: &ModuleInner,
    ) -> Result<BoxedMap<LocalMemoryIndex, Memory>, CreationError> {
        let mut memories = Map::with_capacity(module.info.memories.len());
        for (_, &desc) in &module.info.memories {
            let memory = Memory::new(desc)?;
            memories.push(memory);
        }

        Ok(memories.into_boxed_map())
    }

    /// Validate each locally-defined memory in the Module.
    ///
    /// This involves copying in the data initializers.
    fn validate_memories(module: &ModuleInner, imports: &ImportBacking) -> LinkResult<()> {
        // Validate data size fits
        for init in module.info.data_initializers.iter() {
            let init_base = match init.base {
                Initializer::Const(Value::I32(offset)) => offset as u32,
                Initializer::Const(_) => {
                    return Err(vec![LinkError::Generic {
                        message: "a const initializer must be an i32".to_string(),
                    }]);
                }
                Initializer::GetGlobal(import_global_index) => {
                    if import_global_index.index() >= imports.globals.len() {
                        return Err(vec![LinkError::Generic {
                            message: "incorrect global index for initializer".to_string(),
                        }]);
                    }
                    if let Value::I32(x) = imports.globals[import_global_index].get() {
                        x as u32
                    } else {
                        return Err(vec![LinkError::Generic {
                            message: "unsupported global type for initializer".to_string(),
                        }]);
                    }
                }
            } as usize;

            // Validate data size fits
            match init.memory_index.local_or_import(&module.info) {
                LocalOrImport::Local(local_memory_index) => {
                    let memory_desc = module.info.memories[local_memory_index];
                    let data_top = init_base + init.data.len();
                    if memory_desc.minimum.bytes().0 < data_top || data_top < init_base {
                        return Err(vec![LinkError::Generic {
                            message: "data segment does not fit".to_string(),
                        }]);
                    }
                }
                LocalOrImport::Import(imported_memory_index) => {
                    // Write the initialization data to the memory that
                    // we think the imported memory is.
                    let local_memory = unsafe { &*imports.vm_memories[imported_memory_index] };
                    let data_top = init_base + init.data.len();
                    if local_memory.bound < data_top || data_top < init_base {
                        return Err(vec![LinkError::Generic {
                            message: "data segment does not fit".to_string(),
                        }]);
                    }
                }
            }
        }
        Ok(())
    }

    /// Initialize each locally-defined memory in the Module.
    ///
    /// This involves copying in the data initializers.
    fn finalize_memories(
        module: &ModuleInner,
        imports: &ImportBacking,
        memories: &mut SliceMap<LocalMemoryIndex, Memory>,
    ) -> LinkResult<BoxedMap<LocalMemoryIndex, *mut vm::LocalMemory>> {
        // For each init that has some data...
        // Initialize data
        for init in module.info.data_initializers.iter() {
            let init_base = match init.base {
                Initializer::Const(Value::I32(offset)) => offset as u32,
                Initializer::Const(_) => {
                    return Err(vec![LinkError::Generic {
                        message: "a const initializer must be an i32".to_string(),
                    }]);
                }
                Initializer::GetGlobal(import_global_index) => {
                    if import_global_index.index() >= imports.globals.len() {
                        return Err(vec![LinkError::Generic {
                            message: "incorrect global index for initializer".to_string(),
                        }]);
                    }
                    if let Value::I32(x) = imports.globals[import_global_index].get() {
                        x as u32
                    } else {
                        return Err(vec![LinkError::Generic {
                            message: "unsupported global type for initializer".to_string(),
                        }]);
                    }
                }
            } as usize;

            match init.memory_index.local_or_import(&module.info) {
                LocalOrImport::Local(local_memory_index) => {
                    let mem = &memories[local_memory_index];
                    for (mem_byte, data_byte) in mem.view()[init_base..init_base + init.data.len()]
                        .iter()
                        .zip(init.data.iter())
                    {
                        mem_byte.set(*data_byte);
                    }
                }
                LocalOrImport::Import(imported_memory_index) => {
                    // Write the initialization data to the memory that
                    // we think the imported memory is.
                    let memory_slice = unsafe {
                        let local_memory = &*imports.vm_memories[imported_memory_index];
                        slice::from_raw_parts_mut(local_memory.base, local_memory.bound)
                    };

                    let mem_init_view = &mut memory_slice[init_base..init_base + init.data.len()];
                    mem_init_view.copy_from_slice(&init.data);
                }
            }
        }

        Ok(memories
            .iter_mut()
            .map(|(_, mem)| mem.vm_local_memory())
            .collect::<Map<_, _>>()
            .into_boxed_map())
    }

    fn generate_tables(module: &ModuleInner) -> BoxedMap<LocalTableIndex, Table> {
        let mut tables = Map::with_capacity(module.info.tables.len());

        for (_, &table_desc) in module.info.tables.iter() {
            let table = Table::new(table_desc).unwrap();
            tables.push(table);
        }

        tables.into_boxed_map()
    }

    /// This validates all of the locally-defined tables in the Module.
    #[allow(clippy::cast_ptr_alignment)]
    fn validate_tables(
        module: &ModuleInner,
        imports: &ImportBacking,
        tables: &mut SliceMap<LocalTableIndex, Table>,
    ) -> LinkResult<()> {
        for init in &module.info.elem_initializers {
            let init_base = match init.base {
                Initializer::Const(Value::I32(offset)) => offset as u32,
                Initializer::Const(_) => {
                    return Err(vec![LinkError::Generic {
                        message: "a const initializer must be an i32".to_string(),
                    }]);
                }
                Initializer::GetGlobal(import_global_index) => {
                    if import_global_index.index() >= imports.globals.len() {
                        return Err(vec![LinkError::Generic {
                            message: "incorrect global index for initializer".to_string(),
                        }]);
                    }
                    if let Value::I32(x) = imports.globals[import_global_index].get() {
                        x as u32
                    } else {
                        return Err(vec![LinkError::Generic {
                            message: "unsupported global type for initializer".to_string(),
                        }]);
                    }
                }
            } as usize;

            match init.table_index.local_or_import(&module.info) {
                LocalOrImport::Local(local_table_index) => {
                    let table = &tables[local_table_index];

                    if (table.size() as usize) < init_base + init.elements.len() {
                        return Err(vec![LinkError::Generic {
                            message: "elements segment does not fit".to_string(),
                        }]);
                    }
                }
                LocalOrImport::Import(import_table_index) => {
                    let table = &imports.tables[import_table_index];

                    if (table.size() as usize) < init_base + init.elements.len() {
                        return Err(vec![LinkError::Generic {
                            message: "elements segment does not fit".to_string(),
                        }]);
                    }
                }
            }
        }
        Ok(())
    }

    /// This initializes all of the locally-defined tables in the Module, e.g.
    /// putting all the table elements (function pointers)
    /// in the right places.
    #[allow(clippy::cast_ptr_alignment)]
    fn finalize_tables(
        module: &ModuleInner,
        imports: &ImportBacking,
        tables: &mut SliceMap<LocalTableIndex, Table>,
        vmctx: *mut vm::Ctx,
    ) -> LinkResult<BoxedMap<LocalTableIndex, *mut vm::LocalTable>> {
        for init in &module.info.elem_initializers {
            let init_base = match init.base {
                Initializer::Const(Value::I32(offset)) => offset as u32,
                Initializer::Const(_) => {
                    return Err(vec![LinkError::Generic {
                        message: "a const initializer be an i32".to_string(),
                    }]);
                }
                Initializer::GetGlobal(import_global_index) => {
                    if import_global_index.index() >= imports.globals.len() {
                        return Err(vec![LinkError::Generic {
                            message: "incorrect global index for initializer".to_string(),
                        }]);
                    }
                    if let Value::I32(x) = imports.globals[import_global_index].get() {
                        x as u32
                    } else {
                        return Err(vec![LinkError::Generic {
                            message: "unsupported global type for initializer".to_string(),
                        }]);
                    }
                }
            } as usize;

            match init.table_index.local_or_import(&module.info) {
                LocalOrImport::Local(local_table_index) => {
                    let table = &tables[local_table_index];
                    table.anyfunc_direct_access_mut(|elements| {
                        for (i, &func_index) in init.elements.iter().enumerate() {
                            let sig_index = module.info.func_assoc[func_index];
                            // let signature = &module.info.signatures[sig_index];
                            let signature = SigRegistry
                                .lookup_signature_ref(&module.info.signatures[sig_index]);
                            let sig_id =
                                vm::SigId(SigRegistry.lookup_sig_index(signature).index() as u32);

                            let (func, ctx) = match func_index.local_or_import(&module.info) {
                                LocalOrImport::Local(local_func_index) => (
                                    module
                                        .runnable_module
                                        .get_func(&module.info, local_func_index)
                                        .unwrap()
                                        .as_ptr()
                                        as *const vm::Func,
                                    vmctx,
                                ),
                                LocalOrImport::Import(imported_func_index) => {
                                    let vm::ImportedFunc { func, func_ctx } =
                                        imports.vm_functions[imported_func_index];
                                    (func, unsafe { func_ctx.as_ref() }.vmctx.as_ptr())
                                }
                            };

                            elements[init_base + i] = vm::Anyfunc { func, ctx, sig_id };
                        }
                    });
                }
                LocalOrImport::Import(import_table_index) => {
                    let table = &imports.tables[import_table_index];

                    table.anyfunc_direct_access_mut(|elements| {
                        for (i, &func_index) in init.elements.iter().enumerate() {
                            let sig_index = module.info.func_assoc[func_index];
                            let signature = SigRegistry
                                .lookup_signature_ref(&module.info.signatures[sig_index]);
                            // let signature = &module.info.signatures[sig_index];
                            let sig_id =
                                vm::SigId(SigRegistry.lookup_sig_index(signature).index() as u32);

                            let (func, ctx) = match func_index.local_or_import(&module.info) {
                                LocalOrImport::Local(local_func_index) => (
                                    module
                                        .runnable_module
                                        .get_func(&module.info, local_func_index)
                                        .unwrap()
                                        .as_ptr()
                                        as *const vm::Func,
                                    vmctx,
                                ),
                                LocalOrImport::Import(imported_func_index) => {
                                    let vm::ImportedFunc { func, func_ctx } =
                                        imports.vm_functions[imported_func_index];
                                    (func, unsafe { func_ctx.as_ref() }.vmctx.as_ptr())
                                }
                            };

                            elements[init_base + i] = vm::Anyfunc { func, ctx, sig_id };
                        }
                    });
                }
            }
        }

        Ok(tables
            .iter_mut()
            .map(|(_, table)| table.vm_local_table())
            .collect::<Map<_, _>>()
            .into_boxed_map())
    }

    fn generate_globals(
        module: &ModuleInner,
        imports: &ImportBacking,
    ) -> LinkResult<BoxedMap<LocalGlobalIndex, Global>> {
        let mut globals = Map::with_capacity(module.info.globals.len());

        for (_, global_init) in module.info.globals.iter() {
            let value = match &global_init.init {
                Initializer::Const(value) => value.clone(),
                Initializer::GetGlobal(import_global_index) => {
                    if imports.globals.len() <= import_global_index.index() {
                        return Err(vec![LinkError::Generic {
                            message: format!(
                                "Trying to read the `{:?}` global that is not properly initialized.",
                                import_global_index.index()
                            ),
                        }]);
                    }

                    imports.globals[*import_global_index].get()
                }
            };

            let global = if global_init.desc.mutable {
                Global::new_mutable(value)
            } else {
                Global::new(value)
            };

            globals.push(global);
        }

        Ok(globals.into_boxed_map())
    }

    fn finalize_globals(
        globals: &mut SliceMap<LocalGlobalIndex, Global>,
    ) -> BoxedMap<LocalGlobalIndex, *mut vm::LocalGlobal> {
        globals
            .iter_mut()
            .map(|(_, global)| global.vm_local_global())
            .collect::<Map<_, _>>()
            .into_boxed_map()
    }
}

/// The `ImportBacking` stores references to the imported resources of an Instance. This includes
/// imported memories, tables, globals and functions.
#[derive(Debug)]
pub struct ImportBacking {
    pub(crate) memories: BoxedMap<ImportedMemoryIndex, Memory>,
    pub(crate) tables: BoxedMap<ImportedTableIndex, Table>,
    pub(crate) globals: BoxedMap<ImportedGlobalIndex, Global>,

    pub(crate) vm_functions: BoxedMap<ImportedFuncIndex, vm::ImportedFunc>,
    pub(crate) vm_memories: BoxedMap<ImportedMemoryIndex, *mut vm::LocalMemory>,
    pub(crate) vm_tables: BoxedMap<ImportedTableIndex, *mut vm::LocalTable>,
    pub(crate) vm_globals: BoxedMap<ImportedGlobalIndex, *mut vm::LocalGlobal>,
}

// manually implemented because ImportBacking contains raw pointers directly
unsafe impl Send for ImportBacking {}

impl ImportBacking {
    /// Creates a new `ImportBacking` from the given `ModuleInner`, `ImportObject`, and `Ctx`.
    pub fn new(
        module: &ModuleInner,
        imports: &ImportObject,
        vmctx: *mut vm::Ctx,
    ) -> LinkResult<Self> {
        let mut failed = false;
        let mut link_errors = vec![];

        let vm_functions = import_functions(module, imports, vmctx).unwrap_or_else(|le| {
            failed = true;
            link_errors.extend(le);
            Map::new().into_boxed_map()
        });

        let (memories, vm_memories) = import_memories(module, imports).unwrap_or_else(|le| {
            failed = true;
            link_errors.extend(le);
            (Map::new().into_boxed_map(), Map::new().into_boxed_map())
        });

        let (tables, vm_tables) = import_tables(module, imports).unwrap_or_else(|le| {
            failed = true;
            link_errors.extend(le);
            (Map::new().into_boxed_map(), Map::new().into_boxed_map())
        });

        let (globals, vm_globals) = import_globals(module, imports).unwrap_or_else(|le| {
            failed = true;
            link_errors.extend(le);
            (Map::new().into_boxed_map(), Map::new().into_boxed_map())
        });

        if failed {
            Err(link_errors)
        } else {
            Ok(ImportBacking {
                memories,
                tables,
                globals,

                vm_functions,
                vm_memories,
                vm_tables,
                vm_globals,
            })
        }
    }

    /// Gets a `ImportedFunc` from the given `ImportedFuncIndex`.
    pub fn imported_func(&self, index: ImportedFuncIndex) -> vm::ImportedFunc {
        self.vm_functions[index].clone()
    }
}

impl Drop for ImportBacking {
    fn drop(&mut self) {
        // Properly drop the `vm::FuncCtx` in `vm::ImportedFunc`.
        for (_imported_func_index, imported_func) in (*self.vm_functions).iter_mut() {
            let func_ctx_ptr = imported_func.func_ctx.as_ptr();

            if !func_ctx_ptr.is_null() {
                let _: Box<vm::FuncCtx> = unsafe { Box::from_raw(func_ctx_ptr) };
            }
        }
    }
}

fn import_functions(
    module: &ModuleInner,
    imports: &ImportObject,
    vmctx: *mut vm::Ctx,
) -> LinkResult<BoxedMap<ImportedFuncIndex, vm::ImportedFunc>> {
    let mut link_errors = vec![];
    let mut functions = Map::with_capacity(module.info.imported_functions.len());
    for (
        index,
        ImportName {
            namespace_index,
            name_index,
        },
    ) in &module.info.imported_functions
    {
        let sig_index = module.info.func_assoc[index.convert_up(&module.info)];
        let expected_sig = &module.info.signatures[sig_index];

        let namespace = module.info.namespace_table.get(*namespace_index);
        let name = module.info.name_table.get(*name_index);

        let import =
            imports.maybe_with_namespace(namespace, |namespace| namespace.get_export(name));

        match import {
            Some(Export::Function {
                func,
                ctx,
                signature,
            }) => {
                if *expected_sig == *signature {
                    functions.push(vm::ImportedFunc {
                        func: func.inner(),
                        func_ctx: NonNull::new(Box::into_raw(Box::new(vm::FuncCtx {
                            //                      ^^^^^^^^ `vm::FuncCtx` is purposely leaked.
                            //                               It is dropped by the specific `Drop`
                            //                               implementation of `ImportBacking`.
                            vmctx: NonNull::new(match ctx {
                                Context::External(vmctx) => vmctx,
                                Context::ExternalWithEnv(vmctx_, _) => {
                                    if vmctx_.is_null() {
                                        vmctx
                                    } else {
                                        vmctx_
                                    }
                                }
                                Context::Internal => vmctx,
                            })
                            .expect("`vmctx` must not be null."),
                            func_env: match ctx {
                                Context::ExternalWithEnv(_, func_env) => func_env,
                                _ => None,
                            },
                        })))
                        .unwrap(),
                    });
                } else {
                    link_errors.push(LinkError::IncorrectImportSignature {
                        namespace: namespace.to_string(),
                        name: name.to_string(),
                        expected: (*expected_sig).clone(),
                        found: (*signature).clone(),
                    });
                }
            }
            Some(export_type) => {
                let export_type_name = match export_type {
                    Export::Function { .. } => "function",
                    Export::Memory { .. } => "memory",
                    Export::Table { .. } => "table",
                    Export::Global { .. } => "global",
                }
                .to_string();
                link_errors.push(LinkError::IncorrectImportType {
                    namespace: namespace.to_string(),
                    name: name.to_string(),
                    expected: "function".to_string(),
                    found: export_type_name,
                });
            }
            None => {
                if imports.allow_missing_functions {
                    let always_trap = Func::new(always_trap);

                    functions.push(vm::ImportedFunc {
                        func: always_trap.get_vm_func().as_ptr(),
                        func_ctx: NonNull::new(Box::into_raw(Box::new(vm::FuncCtx {
                            //                      ^^^^^^^^ `vm::FuncCtx` is purposely leaked.
                            //                               It is dropped by the specific `Drop`
                            //                               implementation of `ImportBacking`.
                            vmctx: NonNull::new(vmctx).expect("`vmctx` must not be null."),
                            func_env: None,
                        })))
                        .unwrap(),
                    });
                } else {
                    link_errors.push(LinkError::ImportNotFound {
                        namespace: namespace.to_string(),
                        name: name.to_string(),
                    });
                }
            }
        }
    }

    if !link_errors.is_empty() {
        Err(link_errors)
    } else {
        Ok(functions.into_boxed_map())
    }
}

fn import_memories(
    module: &ModuleInner,
    imports: &ImportObject,
) -> LinkResult<(
    BoxedMap<ImportedMemoryIndex, Memory>,
    BoxedMap<ImportedMemoryIndex, *mut vm::LocalMemory>,
)> {
    let mut link_errors = vec![];
    let mut memories = Map::with_capacity(module.info.imported_memories.len());
    let mut vm_memories = Map::with_capacity(module.info.imported_memories.len());
    for (
        _index,
        (
            ImportName {
                namespace_index,
                name_index,
            },
            expected_memory_desc,
        ),
    ) in &module.info.imported_memories
    {
        let namespace = module.info.namespace_table.get(*namespace_index);
        let name = module.info.name_table.get(*name_index);

        let memory_import =
            imports.maybe_with_namespace(namespace, |namespace| namespace.get_export(name));
        match memory_import {
            Some(Export::Memory(memory)) => {
                if expected_memory_desc.fits_in_imported(memory.descriptor()) {
                    memories.push(memory.clone());
                    vm_memories.push(memory.vm_local_memory());
                } else {
                    link_errors.push(LinkError::IncorrectMemoryType {
                        namespace: namespace.to_string(),
                        name: name.to_string(),
                        expected: *expected_memory_desc,
                        found: memory.descriptor(),
                    });
                }
            }
            Some(export_type) => {
                let export_type_name = match export_type {
                    Export::Function { .. } => "function",
                    Export::Memory { .. } => "memory",
                    Export::Table { .. } => "table",
                    Export::Global { .. } => "global",
                }
                .to_string();
                link_errors.push(LinkError::IncorrectImportType {
                    namespace: namespace.to_string(),
                    name: name.to_string(),
                    expected: "memory".to_string(),
                    found: export_type_name,
                });
            }
            None => {
                link_errors.push(LinkError::ImportNotFound {
                    namespace: namespace.to_string(),
                    name: name.to_string(),
                });
            }
        }
    }

    if !link_errors.is_empty() {
        Err(link_errors)
    } else {
        Ok((memories.into_boxed_map(), vm_memories.into_boxed_map()))
    }
}

fn import_tables(
    module: &ModuleInner,
    imports: &ImportObject,
) -> LinkResult<(
    BoxedMap<ImportedTableIndex, Table>,
    BoxedMap<ImportedTableIndex, *mut vm::LocalTable>,
)> {
    let mut link_errors = vec![];
    let mut tables = Map::with_capacity(module.info.imported_tables.len());
    let mut vm_tables = Map::with_capacity(module.info.imported_tables.len());
    for (
        _index,
        (
            ImportName {
                namespace_index,
                name_index,
            },
            expected_table_desc,
        ),
    ) in &module.info.imported_tables
    {
        let namespace = module.info.namespace_table.get(*namespace_index);
        let name = module.info.name_table.get(*name_index);

        let table_import =
            imports.maybe_with_namespace(namespace, |namespace| namespace.get_export(name));
        match table_import {
            Some(Export::Table(mut table)) => {
                if expected_table_desc.fits_in_imported(table.descriptor()) {
                    vm_tables.push(table.vm_local_table());
                    tables.push(table);
                } else {
                    link_errors.push(LinkError::IncorrectTableType {
                        namespace: namespace.to_string(),
                        name: name.to_string(),
                        expected: *expected_table_desc,
                        found: table.descriptor(),
                    });
                }
            }
            Some(export_type) => {
                let export_type_name = match export_type {
                    Export::Function { .. } => "function",
                    Export::Memory { .. } => "memory",
                    Export::Table { .. } => "table",
                    Export::Global { .. } => "global",
                }
                .to_string();
                link_errors.push(LinkError::IncorrectImportType {
                    namespace: namespace.to_string(),
                    name: name.to_string(),
                    expected: "table".to_string(),
                    found: export_type_name,
                });
            }
            None => {
                link_errors.push(LinkError::ImportNotFound {
                    namespace: namespace.to_string(),
                    name: name.to_string(),
                });
            }
        }
    }

    if link_errors.len() > 0 {
        Err(link_errors)
    } else {
        Ok((tables.into_boxed_map(), vm_tables.into_boxed_map()))
    }
}

fn import_globals(
    module: &ModuleInner,
    imports: &ImportObject,
) -> LinkResult<(
    BoxedMap<ImportedGlobalIndex, Global>,
    BoxedMap<ImportedGlobalIndex, *mut vm::LocalGlobal>,
)> {
    let mut link_errors = vec![];
    let mut globals = Map::with_capacity(module.info.imported_globals.len());
    let mut vm_globals = Map::with_capacity(module.info.imported_globals.len());
    for (
        _,
        (
            ImportName {
                namespace_index,
                name_index,
            },
            imported_global_desc,
        ),
    ) in &module.info.imported_globals
    {
        let namespace = module.info.namespace_table.get(*namespace_index);
        let name = module.info.name_table.get(*name_index);
        let import =
            imports.maybe_with_namespace(namespace, |namespace| namespace.get_export(name));
        match import {
            Some(Export::Global(mut global)) => {
                if global.descriptor() == *imported_global_desc {
                    vm_globals.push(global.vm_local_global());
                    globals.push(global);
                } else {
                    link_errors.push(LinkError::IncorrectGlobalType {
                        namespace: namespace.to_string(),
                        name: name.to_string(),
                        expected: *imported_global_desc,
                        found: global.descriptor(),
                    });
                }
            }
            Some(export_type) => {
                let export_type_name = match export_type {
                    Export::Function { .. } => "function",
                    Export::Memory { .. } => "memory",
                    Export::Table { .. } => "table",
                    Export::Global { .. } => "global",
                }
                .to_string();
                link_errors.push(LinkError::IncorrectImportType {
                    namespace: namespace.to_string(),
                    name: name.to_string(),
                    expected: "global".to_string(),
                    found: export_type_name,
                });
            }
            None => {
                link_errors.push(LinkError::ImportNotFound {
                    namespace: namespace.to_string(),
                    name: name.to_string(),
                });
            }
        }
    }

    if !link_errors.is_empty() {
        Err(link_errors)
    } else {
        Ok((globals.into_boxed_map(), vm_globals.into_boxed_map()))
    }
}
