use crate::{
    error::{LinkError, LinkResult},
    export::{Context, Export},
    import::Imports,
    memory::LinearMemory,
    module::{ImportName, ModuleInner},
    structures::{BoxedMap, Map, SliceMap, TypedIndex},
    table::{TableBacking, TableElements},
    types::{
        ElementType, ImportedFuncIndex, ImportedGlobalIndex, ImportedMemoryIndex,
        ImportedTableIndex, Initializer, LocalGlobalIndex, LocalMemoryIndex, LocalOrImport,
        LocalTableIndex, Type, Value,
    },
    vm,
};
use std::{mem, slice};

#[derive(Debug)]
pub struct LocalBacking {
    pub(crate) memories: BoxedMap<LocalMemoryIndex, LinearMemory>,
    pub(crate) tables: BoxedMap<LocalTableIndex, TableBacking>,

    pub(crate) vm_memories: BoxedMap<LocalMemoryIndex, vm::LocalMemory>,
    pub(crate) vm_tables: BoxedMap<LocalTableIndex, vm::LocalTable>,
    pub(crate) vm_globals: BoxedMap<LocalGlobalIndex, vm::LocalGlobal>,
}

impl LocalBacking {
    pub fn memory(&mut self, local_memory_index: LocalMemoryIndex) -> &mut LinearMemory {
        &mut self.memories[local_memory_index]
    }

    pub fn table(&mut self, local_table_index: LocalTableIndex) -> &mut TableBacking {
        &mut self.tables[local_table_index]
    }
}

impl LocalBacking {
    pub(crate) fn new(module: &ModuleInner, imports: &ImportBacking, vmctx: *mut vm::Ctx) -> Self {
        let mut memories = Self::generate_memories(module);
        let mut tables = Self::generate_tables(module);
        let globals = Self::generate_globals(module);

        let vm_memories = Self::finalize_memories(module, imports, &mut memories);
        let vm_tables = Self::finalize_tables(module, imports, &mut tables, vmctx);
        let vm_globals = Self::finalize_globals(module, imports, globals);

        Self {
            memories,
            tables,

            vm_memories,
            vm_tables,
            vm_globals,
        }
    }

    fn generate_memories(module: &ModuleInner) -> BoxedMap<LocalMemoryIndex, LinearMemory> {
        let mut memories = Map::with_capacity(module.memories.len());

        for (_, mem) in &module.memories {
            // If we use emscripten, we set a fixed initial and maximum
            debug!("Instance - init memory ({}, {:?})", mem.min, mem.max);
            // let memory = if options.abi == InstanceABI::Emscripten {
            //     // We use MAX_PAGES, so at the end the result is:
            //     // (initial * LinearMemory::PAGE_SIZE) == LinearMemory::DEFAULT_HEAP_SIZE
            //     // However, it should be: (initial * LinearMemory::PAGE_SIZE) == 16777216
            //     LinearMemory::new(LinearMemory::MAX_PAGES, None)
            // } else {
            //     LinearMemory::new(memory.minimum, memory.maximum.map(|m| m as u32))
            // };
            let memory = LinearMemory::new(mem);
            memories.push(memory);
        }

        memories.into_boxed_map()
    }

    fn finalize_memories(
        module: &ModuleInner,
        imports: &ImportBacking,
        memories: &mut SliceMap<LocalMemoryIndex, LinearMemory>,
    ) -> BoxedMap<LocalMemoryIndex, vm::LocalMemory> {
        // For each init that has some data...
        for init in module
            .data_initializers
            .iter()
            .filter(|init| init.data.len() > 0)
        {
            let init_base = match init.base {
                Initializer::Const(Value::I32(offset)) => offset as u32,
                Initializer::Const(_) => panic!("a const initializer must be the i32 type"),
                Initializer::GetGlobal(imported_global_index) => {
                    if module.imported_globals[imported_global_index].1.ty == Type::I32 {
                        unsafe { (*imports.globals[imported_global_index].global).data as u32 }
                    } else {
                        panic!("unsupported global type for initialzer")
                    }
                }
            } as usize;

            match init.memory_index.local_or_import(module) {
                LocalOrImport::Local(local_memory_index) => {
                    let memory_desc = &module.memories[local_memory_index];
                    let data_top = init_base + init.data.len();
                    assert!((memory_desc.min * LinearMemory::PAGE_SIZE) as usize >= data_top);
                    let mem: &mut LinearMemory = &mut memories[local_memory_index];

                    let mem_init_view = &mut mem[init_base..init_base + init.data.len()];
                    mem_init_view.copy_from_slice(&init.data);
                }
                LocalOrImport::Import(imported_memory_index) => {
                    let vm_imported_memory = imports.imported_memory(imported_memory_index);
                    unsafe {
                        let local_memory = &(*vm_imported_memory.memory);
                        let memory_slice =
                            slice::from_raw_parts_mut(local_memory.base, local_memory.size);

                        let mem_init_view =
                            &mut memory_slice[init_base..init_base + init.data.len()];
                        mem_init_view.copy_from_slice(&init.data);
                    }
                }
            }
        }

        memories
            .iter_mut()
            .map(|(index, mem)| mem.into_vm_memory(index))
            .collect::<Map<_, _>>()
            .into_boxed_map()
    }

    fn generate_tables(module: &ModuleInner) -> BoxedMap<LocalTableIndex, TableBacking> {
        let mut tables = Map::with_capacity(module.tables.len());

        for (_, table) in &module.tables {
            let table_backing = TableBacking::new(table);
            tables.push(table_backing);
        }

        tables.into_boxed_map()
    }

    #[allow(clippy::cast_ptr_alignment)]
    fn finalize_tables(
        module: &ModuleInner,
        imports: &ImportBacking,
        tables: &mut SliceMap<LocalTableIndex, TableBacking>,
        vmctx: *mut vm::Ctx,
    ) -> BoxedMap<LocalTableIndex, vm::LocalTable> {
        for init in &module.elem_initializers {
            let init_base = match init.base {
                Initializer::Const(Value::I32(offset)) => offset as u32,
                Initializer::Const(_) => panic!("a const initializer must be the i32 type"),
                Initializer::GetGlobal(imported_global_index) => {
                    if module.imported_globals[imported_global_index].1.ty == Type::I32 {
                        unsafe { (*imports.globals[imported_global_index].global).data as u32 }
                    } else {
                        panic!("unsupported global type for initialzer")
                    }
                }
            } as usize;

            match init.table_index.local_or_import(module) {
                LocalOrImport::Local(local_table_index) => {
                    let table = &mut tables[local_table_index];
                    match table.elements {
                        TableElements::Anyfunc(ref mut elements) => {
                            if elements.len() < init_base + init.elements.len() {
                                // Grow the table if it's too small.
                                elements
                                    .resize(init_base + init.elements.len(), vm::Anyfunc::null());
                            }

                            for (i, &func_index) in init.elements.iter().enumerate() {
                                let sig_index = module.func_assoc[func_index];
                                let sig_id = vm::SigId(sig_index.index() as u32);

                                let func_data = match func_index.local_or_import(module) {
                                    LocalOrImport::Local(local_func_index) => vm::ImportedFunc {
                                        func: module
                                            .func_resolver
                                            .get(module, local_func_index)
                                            .unwrap()
                                            .as_ptr(),
                                        vmctx,
                                    },
                                    LocalOrImport::Import(imported_func_index) => {
                                        imports.functions[imported_func_index].clone()
                                    }
                                };

                                elements[init_base + i] = vm::Anyfunc { func_data, sig_id };
                            }
                        }
                    }
                }
                LocalOrImport::Import(imported_table_index) => {
                    let (_, table_description) = module.imported_tables[imported_table_index];
                    match table_description.ty {
                        ElementType::Anyfunc => {
                            let imported_table = &imports.tables[imported_table_index];
                            let imported_local_table = (*imported_table).table;

                            let mut elements = unsafe {
                                Vec::from_raw_parts(
                                    (*imported_local_table).base as *mut vm::Anyfunc,
                                    (*imported_local_table).current_elements,
                                    (*imported_local_table).capacity,
                                )
                            };

                            if elements.len() < init_base + init.elements.len() {
                                // Grow the table if it's too small.
                                elements
                                    .resize(init_base + init.elements.len(), vm::Anyfunc::null());
                                // Since the vector may have changed location after reallocating,
                                // we must fix the base, current_elements, and capacity fields.
                                unsafe {
                                    (*imported_local_table).base = elements.as_mut_ptr() as *mut u8;
                                    (*imported_local_table).current_elements = elements.len();
                                    (*imported_local_table).capacity = elements.capacity();
                                }
                            }

                            for (i, &func_index) in init.elements.iter().enumerate() {
                                let sig_index = module.func_assoc[func_index];
                                let sig_id = vm::SigId(sig_index.index() as u32);

                                let func_data = match func_index.local_or_import(module) {
                                    LocalOrImport::Local(local_func_index) => vm::ImportedFunc {
                                        func: module
                                            .func_resolver
                                            .get(module, local_func_index)
                                            .unwrap()
                                            .as_ptr(),
                                        vmctx,
                                    },
                                    LocalOrImport::Import(imported_func_index) => {
                                        imports.functions[imported_func_index].clone()
                                    }
                                };

                                elements[init_base + i] = vm::Anyfunc { func_data, sig_id };
                            }

                            // println!("imported elements: {:#?}", elements);

                            // THIS IS EXTREMELY IMPORTANT.
                            mem::forget(elements);
                        }
                    }
                }
            }
        }

        tables
            .iter_mut()
            .map(|(_, table)| table.into_vm_table())
            .collect::<Map<_, _>>()
            .into_boxed_map()
    }

    fn generate_globals(module: &ModuleInner) -> BoxedMap<LocalGlobalIndex, vm::LocalGlobal> {
        let mut globals = Map::with_capacity(module.globals.len());

        globals.resize(module.globals.len(), vm::LocalGlobal::null());

        globals.into_boxed_map()
    }

    fn finalize_globals(
        module: &ModuleInner,
        imports: &ImportBacking,
        mut globals: BoxedMap<LocalGlobalIndex, vm::LocalGlobal>,
    ) -> BoxedMap<LocalGlobalIndex, vm::LocalGlobal> {
        for ((_, to), (_, from)) in globals.iter_mut().zip(module.globals.iter()) {
            to.data = match from.init {
                Initializer::Const(ref value) => match value {
                    Value::I32(x) => *x as u64,
                    Value::I64(x) => *x as u64,
                    Value::F32(x) => x.to_bits() as u64,
                    Value::F64(x) => x.to_bits(),
                },
                Initializer::GetGlobal(imported_global_index) => unsafe {
                    (*imports.globals[imported_global_index].global).data
                },
            };
        }

        globals
    }
}

#[derive(Debug)]
pub struct ImportBacking {
    pub(crate) functions: BoxedMap<ImportedFuncIndex, vm::ImportedFunc>,
    pub(crate) memories: BoxedMap<ImportedMemoryIndex, vm::ImportedMemory>,
    pub(crate) tables: BoxedMap<ImportedTableIndex, vm::ImportedTable>,
    pub(crate) globals: BoxedMap<ImportedGlobalIndex, vm::ImportedGlobal>,
}

impl ImportBacking {
    pub fn new(
        module: &ModuleInner,
        imports: &mut Imports,
        vmctx: *mut vm::Ctx,
    ) -> LinkResult<Self> {
        let mut failed = false;
        let mut link_errors = vec![];

        let functions = import_functions(module, imports, vmctx).unwrap_or_else(|le| {
            failed = true;
            link_errors.extend(le);
            Map::new().into_boxed_map()
        });

        let memories = import_memories(module, imports, vmctx).unwrap_or_else(|le| {
            failed = true;
            link_errors.extend(le);
            Map::new().into_boxed_map()
        });

        let tables = import_tables(module, imports, vmctx).unwrap_or_else(|le| {
            failed = true;
            link_errors.extend(le);
            Map::new().into_boxed_map()
        });

        let globals = import_globals(module, imports).unwrap_or_else(|le| {
            failed = true;
            link_errors.extend(le);
            Map::new().into_boxed_map()
        });

        if failed {
            Err(link_errors)
        } else {
            Ok(ImportBacking {
                functions,
                memories,
                tables,
                globals,
            })
        }
    }

    pub fn imported_func(&self, func_index: ImportedFuncIndex) -> vm::ImportedFunc {
        self.functions[func_index].clone()
    }

    pub fn imported_memory(&self, memory_index: ImportedMemoryIndex) -> vm::ImportedMemory {
        self.memories[memory_index].clone()
    }
}

fn import_functions(
    module: &ModuleInner,
    imports: &mut Imports,
    vmctx: *mut vm::Ctx,
) -> LinkResult<BoxedMap<ImportedFuncIndex, vm::ImportedFunc>> {
    let mut link_errors = vec![];
    let mut functions = Map::with_capacity(module.imported_functions.len());
    for (index, ImportName { namespace, name }) in &module.imported_functions {
        let sig_index = module.func_assoc[index.convert_up(module)];
        let expected_sig = module.sig_registry.lookup_func_sig(sig_index);
        let import = imports
            .get_namespace(namespace)
            .and_then(|namespace| namespace.get_export(name));
        match import {
            Some(Export::Function {
                func,
                ctx,
                signature,
            }) => {
                if expected_sig == &signature {
                    functions.push(vm::ImportedFunc {
                        func: func.inner(),
                        vmctx: match ctx {
                            Context::External(ctx) => ctx,
                            Context::Internal => vmctx,
                        },
                    });
                } else {
                    link_errors.push(LinkError::IncorrectImportSignature {
                        namespace: namespace.clone(),
                        name: name.clone(),
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
                    namespace: namespace.clone(),
                    name: name.clone(),
                    expected: "function".to_string(),
                    found: export_type_name,
                });
            }
            None => {
                link_errors.push(LinkError::ImportNotFound {
                    namespace: namespace.clone(),
                    name: name.clone(),
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
    imports: &mut Imports,
    vmctx: *mut vm::Ctx,
) -> LinkResult<BoxedMap<ImportedMemoryIndex, vm::ImportedMemory>> {
    let mut link_errors = vec![];
    let mut memories = Map::with_capacity(module.imported_memories.len());
    for (_index, (ImportName { namespace, name }, expected_memory_desc)) in
        &module.imported_memories
    {
        let memory_import = imports
            .get_namespace(namespace)
            .and_then(|namespace| namespace.get_export(name));
        match memory_import {
            Some(Export::Memory {
                local,
                ctx,
                memory: memory_desc,
            }) => {
                if expected_memory_desc.fits_in_imported(&memory_desc) {
                    memories.push(vm::ImportedMemory {
                        memory: local.inner(),
                        vmctx: match ctx {
                            Context::External(ctx) => ctx,
                            Context::Internal => vmctx,
                        },
                    });
                } else {
                    link_errors.push(LinkError::IncorrectMemoryDescription {
                        namespace: namespace.clone(),
                        name: name.clone(),
                        expected: expected_memory_desc.clone(),
                        found: memory_desc.clone(),
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
                    namespace: namespace.clone(),
                    name: name.clone(),
                    expected: "memory".to_string(),
                    found: export_type_name,
                });
            }
            None => {
                link_errors.push(LinkError::ImportNotFound {
                    namespace: namespace.clone(),
                    name: name.clone(),
                });
            }
        }
    }

    if link_errors.len() > 0 {
        Err(link_errors)
    } else {
        Ok(memories.into_boxed_map())
    }
}

fn import_tables(
    module: &ModuleInner,
    imports: &mut Imports,
    vmctx: *mut vm::Ctx,
) -> LinkResult<BoxedMap<ImportedTableIndex, vm::ImportedTable>> {
    let mut link_errors = vec![];
    let mut tables = Map::with_capacity(module.imported_tables.len());
    for (_index, (ImportName { namespace, name }, expected_table_desc)) in &module.imported_tables {
        let table_import = imports
            .get_namespace(namespace)
            .and_then(|namespace| namespace.get_export(name));
        match table_import {
            Some(Export::Table {
                local,
                ctx,
                table: table_desc,
            }) => {
                if expected_table_desc.fits_in_imported(&table_desc) {
                    tables.push(vm::ImportedTable {
                        table: local.inner(),
                        vmctx: match ctx {
                            Context::External(ctx) => ctx,
                            Context::Internal => vmctx,
                        },
                    });
                } else {
                    link_errors.push(LinkError::IncorrectTableDescription {
                        namespace: namespace.clone(),
                        name: name.clone(),
                        expected: expected_table_desc.clone(),
                        found: table_desc.clone(),
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
                    namespace: namespace.clone(),
                    name: name.clone(),
                    expected: "table".to_string(),
                    found: export_type_name,
                });
            }
            None => {
                link_errors.push(LinkError::ImportNotFound {
                    namespace: namespace.clone(),
                    name: name.clone(),
                });
            }
        }
    }

    if link_errors.len() > 0 {
        Err(link_errors)
    } else {
        Ok(tables.into_boxed_map())
    }
}

fn import_globals(
    module: &ModuleInner,
    imports: &mut Imports,
) -> LinkResult<BoxedMap<ImportedGlobalIndex, vm::ImportedGlobal>> {
    let mut link_errors = vec![];
    let mut globals = Map::with_capacity(module.imported_globals.len());
    for (_, (ImportName { namespace, name }, imported_global_desc)) in &module.imported_globals {
        let import = imports
            .get_namespace(namespace)
            .and_then(|namespace| namespace.get_export(name));
        match import {
            Some(Export::Global { local, global }) => {
                if global == *imported_global_desc {
                    globals.push(vm::ImportedGlobal {
                        global: local.inner(),
                    });
                } else {
                    link_errors.push(LinkError::IncorrectGlobalDescription {
                        namespace: namespace.clone(),
                        name: name.clone(),
                        expected: imported_global_desc.clone(),
                        found: global.clone(),
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
                    namespace: namespace.clone(),
                    name: name.clone(),
                    expected: "global".to_string(),
                    found: export_type_name,
                });
            }
            None => {
                link_errors.push(LinkError::ImportNotFound {
                    namespace: namespace.clone(),
                    name: name.clone(),
                });
            }
        }
    }

    if link_errors.len() > 0 {
        Err(link_errors)
    } else {
        Ok(globals.into_boxed_map())
    }
}
