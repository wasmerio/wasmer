use crate::{
    error::{LinkError, LinkResult},
    export::{Context, Export},
    global::Global,
    import::ImportObject,
    memory::Memory,
    module::{ImportName, ModuleInner},
    sig_registry::SigRegistry,
    structures::{BoxedMap, Map, SliceMap, TypedIndex},
    table::Table,
    types::{
        ImportedFuncIndex, ImportedGlobalIndex, ImportedMemoryIndex, ImportedTableIndex,
        Initializer, LocalGlobalIndex, LocalMemoryIndex, LocalOrImport, LocalTableIndex, Value,
    },
    vm,
};
use std::{slice, sync::Arc};

#[derive(Debug)]
pub struct LocalBacking {
    pub(crate) memories: BoxedMap<LocalMemoryIndex, Memory>,
    pub(crate) tables: BoxedMap<LocalTableIndex, Table>,
    pub(crate) globals: BoxedMap<LocalGlobalIndex, Global>,

    pub(crate) vm_memories: BoxedMap<LocalMemoryIndex, *mut vm::LocalMemory>,
    pub(crate) vm_tables: BoxedMap<LocalTableIndex, *mut vm::LocalTable>,
    pub(crate) vm_globals: BoxedMap<LocalGlobalIndex, *mut vm::LocalGlobal>,
}

// impl LocalBacking {
//     pub fn memory(&mut self, local_memory_index: LocalMemoryIndex) -> &mut Memory {
//         &mut self.memories[local_memory_index]
//     }

//     pub fn table(&mut self, local_table_index: LocalTableIndex) -> &mut TableBacking {
//         &mut self.tables[local_table_index]
//     }
// }

impl LocalBacking {
    pub(crate) fn new(module: &ModuleInner, imports: &ImportBacking, vmctx: *mut vm::Ctx) -> Self {
        let mut memories = Self::generate_memories(module);
        let mut tables = Self::generate_tables(module);
        let mut globals = Self::generate_globals(module, imports);

        let vm_memories = Self::finalize_memories(module, imports, &mut memories);
        let vm_tables = Self::finalize_tables(module, imports, &mut tables, vmctx);
        let vm_globals = Self::finalize_globals(&mut globals);

        Self {
            memories,
            tables,
            globals,

            vm_memories,
            vm_tables,
            vm_globals,
        }
    }

    fn generate_memories(module: &ModuleInner) -> BoxedMap<LocalMemoryIndex, Memory> {
        let mut memories = Map::with_capacity(module.info.memories.len());
        for (_, &desc) in &module.info.memories {
            memories.push(Memory::new(desc).expect("unable to create memory"));
        }

        memories.into_boxed_map()
    }

    fn finalize_memories(
        module: &ModuleInner,
        imports: &ImportBacking,
        memories: &mut SliceMap<LocalMemoryIndex, Memory>,
    ) -> BoxedMap<LocalMemoryIndex, *mut vm::LocalMemory> {
        // For each init that has some data...
        for init in module
            .info
            .data_initializers
            .iter()
            .filter(|init| init.data.len() > 0)
        {
            let init_base = match init.base {
                Initializer::Const(Value::I32(offset)) => offset as u32,
                Initializer::Const(_) => panic!("a const initializer must be the i32 type"),
                Initializer::GetGlobal(import_global_index) => {
                    if let Value::I32(x) = imports.globals[import_global_index].get() {
                        x as u32
                    } else {
                        panic!("unsupported global type for initialzer")
                    }
                }
            } as usize;

            match init.memory_index.local_or_import(&module.info) {
                LocalOrImport::Local(local_memory_index) => {
                    let memory_desc = module.info.memories[local_memory_index];
                    let data_top = init_base + init.data.len();
                    assert!(memory_desc.minimum.bytes().0 >= data_top);

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
                    unsafe {
                        let local_memory = &*imports.vm_memories[imported_memory_index];
                        let memory_slice =
                            slice::from_raw_parts_mut(local_memory.base, local_memory.bound);

                        let mem_init_view =
                            &mut memory_slice[init_base..init_base + init.data.len()];
                        mem_init_view.copy_from_slice(&init.data);
                    }
                }
            }
        }

        memories
            .iter_mut()
            .map(|(_, mem)| mem.vm_local_memory())
            .collect::<Map<_, _>>()
            .into_boxed_map()
    }

    fn generate_tables(module: &ModuleInner) -> BoxedMap<LocalTableIndex, Table> {
        let mut tables = Map::with_capacity(module.info.tables.len());

        for (_, &table_desc) in module.info.tables.iter() {
            let table = Table::new(table_desc).unwrap();
            tables.push(table);
        }

        tables.into_boxed_map()
    }

    #[allow(clippy::cast_ptr_alignment)]
    fn finalize_tables(
        module: &ModuleInner,
        imports: &ImportBacking,
        tables: &mut SliceMap<LocalTableIndex, Table>,
        vmctx: *mut vm::Ctx,
    ) -> BoxedMap<LocalTableIndex, *mut vm::LocalTable> {
        for init in &module.info.elem_initializers {
            let init_base = match init.base {
                Initializer::Const(Value::I32(offset)) => offset as u32,
                Initializer::Const(_) => panic!("a const initializer must be the i32 type"),
                Initializer::GetGlobal(import_global_index) => {
                    if let Value::I32(x) = imports.globals[import_global_index].get() {
                        x as u32
                    } else {
                        panic!("unsupported global type for initialzer")
                    }
                }
            } as usize;

            match init.table_index.local_or_import(&module.info) {
                LocalOrImport::Local(local_table_index) => {
                    let table = &tables[local_table_index];

                    if (table.size() as usize) < init_base + init.elements.len() {
                        let delta = (init_base + init.elements.len()) - table.size() as usize;
                        // Grow the table if it's too small.
                        table.grow(delta as u32).expect("couldn't grow table");
                    }

                    table.anyfunc_direct_access_mut(|elements| {
                        for (i, &func_index) in init.elements.iter().enumerate() {
                            let sig_index = module.info.func_assoc[func_index];
                            let signature = &module.info.signatures[sig_index];
                            let sig_id = vm::SigId(
                                SigRegistry.lookup_sig_index(Arc::clone(&signature)).index() as u32,
                            );

                            let (func, ctx) = match func_index.local_or_import(&module.info) {
                                LocalOrImport::Local(local_func_index) => (
                                    module
                                        .func_resolver
                                        .get(module, local_func_index)
                                        .unwrap()
                                        .as_ptr()
                                        as *const vm::Func,
                                    vmctx,
                                ),
                                LocalOrImport::Import(imported_func_index) => {
                                    let vm::ImportedFunc { func, vmctx } =
                                        imports.vm_functions[imported_func_index];
                                    (func, vmctx)
                                }
                            };

                            elements[init_base + i] = vm::Anyfunc { func, ctx, sig_id };
                        }
                    });
                }
                LocalOrImport::Import(import_table_index) => {
                    let table = &imports.tables[import_table_index];

                    if (table.size() as usize) < init_base + init.elements.len() {
                        let delta = (init_base + init.elements.len()) - table.size() as usize;
                        // Grow the table if it's too small.
                        table.grow(delta as u32).expect("couldn't grow table");
                    }

                    table.anyfunc_direct_access_mut(|elements| {
                        for (i, &func_index) in init.elements.iter().enumerate() {
                            let sig_index = module.info.func_assoc[func_index];
                            let signature = &module.info.signatures[sig_index];
                            let sig_id = vm::SigId(
                                SigRegistry.lookup_sig_index(Arc::clone(&signature)).index() as u32,
                            );

                            let (func, ctx) = match func_index.local_or_import(&module.info) {
                                LocalOrImport::Local(local_func_index) => (
                                    module
                                        .func_resolver
                                        .get(module, local_func_index)
                                        .unwrap()
                                        .as_ptr()
                                        as *const vm::Func,
                                    vmctx,
                                ),
                                LocalOrImport::Import(imported_func_index) => {
                                    let vm::ImportedFunc { func, vmctx } =
                                        imports.vm_functions[imported_func_index];
                                    (func, vmctx)
                                }
                            };

                            elements[init_base + i] = vm::Anyfunc { func, ctx, sig_id };
                        }
                    });
                }
            }
        }

        tables
            .iter_mut()
            .map(|(_, table)| table.vm_local_table())
            .collect::<Map<_, _>>()
            .into_boxed_map()
    }

    fn generate_globals(
        module: &ModuleInner,
        imports: &ImportBacking,
    ) -> BoxedMap<LocalGlobalIndex, Global> {
        let mut globals = Map::with_capacity(module.info.globals.len());

        for (_, global_init) in module.info.globals.iter() {
            let value = match &global_init.init {
                Initializer::Const(value) => value.clone(),
                Initializer::GetGlobal(import_global_index) => {
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

        globals.into_boxed_map()
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

impl ImportBacking {
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

    pub fn imported_func(&self, index: ImportedFuncIndex) -> vm::ImportedFunc {
        self.vm_functions[index].clone()
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

        let import = imports
            .get_namespace(namespace)
            .and_then(|namespace| namespace.get_export(name));
        match import {
            Some(Export::Function {
                func,
                ctx,
                signature,
            }) => {
                if *expected_sig == signature {
                    functions.push(vm::ImportedFunc {
                        func: func.inner(),
                        vmctx: match ctx {
                            Context::External(ctx) => ctx,
                            Context::Internal => vmctx,
                        },
                    });
                } else {
                    link_errors.push(LinkError::IncorrectImportSignature {
                        namespace: namespace.to_string(),
                        name: name.to_string(),
                        expected: expected_sig.clone(),
                        found: signature.clone(),
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

        let memory_import = imports
            .get_namespace(&namespace)
            .and_then(|namespace| namespace.get_export(&name));
        match memory_import {
            Some(Export::Memory(memory)) => {
                if expected_memory_desc.fits_in_imported(memory.descriptor()) {
                    memories.push(memory.clone());
                    vm_memories.push(memory.vm_local_memory());
                } else {
                    link_errors.push(LinkError::IncorrectMemoryDescriptor {
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

    if link_errors.len() > 0 {
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

        let table_import = imports
            .get_namespace(&namespace)
            .and_then(|namespace| namespace.get_export(&name));
        match table_import {
            Some(Export::Table(mut table)) => {
                if expected_table_desc.fits_in_imported(table.descriptor()) {
                    vm_tables.push(table.vm_local_table());
                    tables.push(table);
                } else {
                    link_errors.push(LinkError::IncorrectTableDescriptor {
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
        let import = imports
            .get_namespace(namespace)
            .and_then(|namespace| namespace.get_export(name));
        match import {
            Some(Export::Global(mut global)) => {
                if global.descriptor() == *imported_global_desc {
                    vm_globals.push(global.vm_local_global());
                    globals.push(global);
                } else {
                    link_errors.push(LinkError::IncorrectGlobalDescriptor {
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

    if link_errors.len() > 0 {
        Err(link_errors)
    } else {
        Ok((globals.into_boxed_map(), vm_globals.into_boxed_map()))
    }
}
