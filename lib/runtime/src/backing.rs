use crate::{
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
use std::slice;

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
                    println!("data_top: {}", data_top);
                    assert!((memory_desc.min * LinearMemory::PAGE_SIZE) as usize >= data_top);
                    let mem: &mut LinearMemory = &mut memories[local_memory_index];

                    let to_init = &mut mem[init_base..init_base + init.data.len()];
                    to_init.copy_from_slice(&init.data);
                }
                LocalOrImport::Import(imported_memory_index) => {
                    let _ = imported_memory_index;
                    let _ = imports;
                    unimplemented!()
                }
            }
        }

        memories
            .iter_mut()
            .map(|(_, mem)| mem.into_vm_memory())
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

            assert!(
                init_base + init.elements.len()
                    <= match init.table_index.local_or_import(module) {
                        LocalOrImport::Local(local_table_index) => {
                            module.tables[local_table_index].min
                        }
                        LocalOrImport::Import(imported_table_index) => {
                            let (_, table_desc) = module.imported_tables[imported_table_index];
                            table_desc.min
                        }
                    } as usize
            );

            match init.table_index.local_or_import(module) {
                LocalOrImport::Local(local_table_index) => {
                    let table = &mut tables[local_table_index];
                    match table.elements {
                        TableElements::Anyfunc(ref mut elements) => {
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
                    let imported_table = &imports.tables[imported_table_index];

                    let imported_local_table_slice = unsafe {
                        let imported_local_table = (*imported_table).table;

                        slice::from_raw_parts_mut(
                            (*imported_local_table).base as *mut vm::Anyfunc,
                            (*imported_local_table).current_elements,
                        )
                    };

                    let (_, table_description) = module.imported_tables[imported_table_index];
                    match table_description.ty {
                        ElementType::Anyfunc => {
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

                                imported_local_table_slice[init_base + i] =
                                    vm::Anyfunc { func_data, sig_id };
                            }
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
    pub functions: BoxedMap<ImportedFuncIndex, vm::ImportedFunc>,
    pub memories: BoxedMap<ImportedMemoryIndex, vm::ImportedMemory>,
    pub tables: BoxedMap<ImportedTableIndex, vm::ImportedTable>,
    pub globals: BoxedMap<ImportedGlobalIndex, vm::ImportedGlobal>,
}

impl ImportBacking {
    pub fn new(
        module: &ModuleInner,
        imports: &mut Imports,
        vmctx: *mut vm::Ctx,
    ) -> Result<Self, String> {
        Ok(ImportBacking {
            functions: import_functions(module, imports, vmctx)?,
            memories: import_memories(module, imports, vmctx)?,
            tables: import_tables(module, imports, vmctx)?,
            globals: import_globals(module, imports)?,
        })
    }
}

fn import_functions(
    module: &ModuleInner,
    imports: &mut Imports,
    vmctx: *mut vm::Ctx,
) -> Result<BoxedMap<ImportedFuncIndex, vm::ImportedFunc>, String> {
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
                    return Err(format!(
                        "unexpected signature for {:?}:{:?}",
                        namespace, name
                    ));
                }
            }
            Some(_) => {
                return Err(format!("incorrect import type for {}:{}", namespace, name));
            }
            None => {
                return Err(format!("import not found: {}:{}", namespace, name));
            }
        }
    }
    Ok(functions.into_boxed_map())
}

fn import_memories(
    module: &ModuleInner,
    imports: &mut Imports,
    vmctx: *mut vm::Ctx,
) -> Result<BoxedMap<ImportedMemoryIndex, vm::ImportedMemory>, String> {
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
                    return Err(format!(
                        "incorrect memory description for {}:{}",
                        namespace, name,
                    ));
                }
            }
            Some(_) => {
                return Err(format!("incorrect import type for {}:{}", namespace, name));
            }
            None => {
                return Err(format!("import not found: {}:{}", namespace, name));
            }
        }
    }
    Ok(memories.into_boxed_map())
}

fn import_tables(
    module: &ModuleInner,
    imports: &mut Imports,
    vmctx: *mut vm::Ctx,
) -> Result<BoxedMap<ImportedTableIndex, vm::ImportedTable>, String> {
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
                    return Err(format!(
                        "incorrect table description for {}:{}",
                        namespace, name,
                    ));
                }
            }
            Some(_) => {
                return Err(format!("incorrect import type for {}:{}", namespace, name));
            }
            None => {
                return Err(format!("import not found: {}:{}", namespace, name));
            }
        }
    }
    Ok(tables.into_boxed_map())
}

fn import_globals(
    module: &ModuleInner,
    imports: &mut Imports,
) -> Result<BoxedMap<ImportedGlobalIndex, vm::ImportedGlobal>, String> {
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
                    return Err(format!(
                        "unexpected global description for {:?}:{:?}",
                        namespace, name
                    ));
                }
            }
            Some(_) => {
                return Err(format!("incorrect import type for {}:{}", namespace, name));
            }
            None => {
                return Err(format!("import not found: {}:{}", namespace, name));
            }
        }
    }
    Ok(globals.into_boxed_map())
}
