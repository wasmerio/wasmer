use crate::{
    export::{Context, Export},
    import::ImportResolver,
    memory::LinearMemory,
    module::{ImportName, Module},
    table::{TableBacking, TableElements},
    types::{Initializer, MapIndex, Value},
    vm,
};

#[derive(Debug)]
pub struct LocalBacking {
    pub memories: Box<[LinearMemory]>,
    pub tables: Box<[TableBacking]>,

    pub vm_memories: Box<[vm::LocalMemory]>,
    pub vm_tables: Box<[vm::LocalTable]>,
    pub vm_globals: Box<[vm::LocalGlobal]>,
}

impl LocalBacking {
    pub fn new(module: &Module, imports: &ImportBacking, vmctx: *mut vm::Ctx) -> Self {
        let mut memories = Self::generate_memories(module);
        let mut tables = Self::generate_tables(module);
        let globals = Self::generate_globals(module);

        let vm_memories = Self::finalize_memories(module, &mut memories[..]);
        let vm_tables = Self::finalize_tables(module, imports, &mut tables[..], vmctx);
        let vm_globals = Self::finalize_globals(module, imports, globals);

        Self {
            memories,
            tables,

            vm_memories,
            vm_tables,
            vm_globals,
        }
    }

    fn generate_memories(module: &Module) -> Box<[LinearMemory]> {
        let mut memories = Vec::with_capacity(module.memories.len());

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

        memories.into_boxed_slice()
    }

    fn finalize_memories(module: &Module, memories: &mut [LinearMemory]) -> Box<[vm::LocalMemory]> {
        for init in &module.data_initializers {
            assert!(init.base.is_none(), "global base not supported yet");
            assert!(init.offset + init.data.len() <= memories[init.memory_index.index()].size());
            let offset = init.offset;
            let mem: &mut LinearMemory = &mut memories[init.memory_index.index()];
            // let end_of_init = offset + init.data.len();
            // if end_of_init > mem.current_size() {
            //     let grow_pages = (end_of_init / LinearMemory::PAGE_SIZE as usize) + 1;
            //     mem.grow(grow_pages as u32)
            //         .expect("failed to grow memory for data initializers");
            // }
            let to_init = &mut mem[offset..offset + init.data.len()];
            to_init.copy_from_slice(&init.data);
        }

        memories
            .iter_mut()
            .map(|mem| mem.into_vm_memory())
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    fn generate_tables(module: &Module) -> Box<[TableBacking]> {
        let mut tables = Vec::with_capacity(module.tables.len());

        for (_, table) in &module.tables {
            let table_backing = TableBacking::new(table);
            tables.push(table_backing);
        }

        tables.into_boxed_slice()
    }

    fn finalize_tables(
        module: &Module,
        imports: &ImportBacking,
        tables: &mut [TableBacking],
        vmctx: *mut vm::Ctx,
    ) -> Box<[vm::LocalTable]> {
        for init in &module.table_initializers {
            assert!(init.base.is_none(), "global base not supported yet");
            let table = &mut tables[init.table_index.index()];
            match table.elements {
                TableElements::Anyfunc(ref mut elements) => {
                    for (i, &func_index) in init.elements.iter().enumerate() {
                        let sig_index = module.func_assoc[func_index];
                        let sig_id = vm::SigId(sig_index.index() as u32);

                        let func_data = if module.is_imported_function(func_index) {
                            imports.functions[func_index.index()].clone()
                        } else {
                            vm::ImportedFunc {
                                func: module
                                    .func_resolver
                                    .get(module, func_index)
                                    .unwrap()
                                    .as_ptr(),
                                vmctx,
                            }
                        };

                        elements[init.offset + i] = vm::Anyfunc { func_data, sig_id };
                    }
                }
            }
        }

        tables
            .iter_mut()
            .map(|table| table.into_vm_table())
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    fn generate_globals(module: &Module) -> Box<[vm::LocalGlobal]> {
        let globals = vec![vm::LocalGlobal::null(); module.globals.len()];

        globals.into_boxed_slice()
    }

    fn finalize_globals(
        module: &Module,
        imports: &ImportBacking,
        mut globals: Box<[vm::LocalGlobal]>,
    ) -> Box<[vm::LocalGlobal]> {
        for (to, (_, from)) in globals.iter_mut().zip(module.globals.into_iter()) {
            to.data = match from.init {
                Initializer::Const(Value::I32(x)) => x as u64,
                Initializer::Const(Value::I64(x)) => x as u64,
                Initializer::Const(Value::F32(x)) => x.to_bits() as u64,
                Initializer::Const(Value::F64(x)) => x.to_bits(),
                Initializer::GetGlobal(index) => unsafe {
                    (*imports.globals[index.index()].global).data
                },
            };
        }

        globals
    }
}

#[derive(Debug)]
pub struct ImportBacking {
    pub functions: Box<[vm::ImportedFunc]>,
    pub memories: Box<[vm::ImportedMemory]>,
    pub tables: Box<[vm::ImportedTable]>,
    pub globals: Box<[vm::ImportedGlobal]>,
}

impl ImportBacking {
    pub fn new(
        module: &Module,
        imports: &dyn ImportResolver,
        vmctx: *mut vm::Ctx,
    ) -> Result<Self, String> {
        assert!(
            module.imported_tables.len() == 0,
            "imported tables not yet supported"
        );

        Ok(ImportBacking {
            functions: import_functions(module, imports, vmctx)?,
            memories: import_memories(module, imports, vmctx)?,
            tables: vec![].into_boxed_slice(),
            globals: import_globals(module, imports)?,
        })
    }
}

fn import_memories(
    module: &Module,
    imports: &dyn ImportResolver,
    vmctx: *mut vm::Ctx,
) -> Result<Box<[vm::ImportedMemory]>, String> {
    let mut memories = Vec::with_capacity(module.imported_memories.len());
    for (_index, (ImportName { namespace, name }, expected_memory_desc)) in
        &module.imported_memories
    {
        let memory_import = imports.get(namespace, name);
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
    Ok(memories.into_boxed_slice())
}

fn import_functions(
    module: &Module,
    imports: &dyn ImportResolver,
    vmctx: *mut vm::Ctx,
) -> Result<Box<[vm::ImportedFunc]>, String> {
    let mut functions = Vec::with_capacity(module.imported_functions.len());
    for (index, ImportName { namespace, name }) in &module.imported_functions {
        let sig_index = module.func_assoc[index];
        let expected_sig = module.sig_registry.lookup_func_sig(sig_index);
        let import = imports.get(namespace, name);
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
    Ok(functions.into_boxed_slice())
}

fn import_globals(
    module: &Module,
    imports: &dyn ImportResolver,
) -> Result<Box<[vm::ImportedGlobal]>, String> {
    let mut globals = Vec::with_capacity(module.imported_globals.len());
    for (_, (ImportName { namespace, name }, global_desc)) in &module.imported_globals {
        let import = imports.get(namespace, name);
        match import {
            Some(Export::Global { local, global }) => {
                if &global == global_desc {
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
    Ok(globals.into_boxed_slice())
}
