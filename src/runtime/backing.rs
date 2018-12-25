use crate::runtime::{
    vm,
    module::Module,
    table::TableBacking,
    types::{Val, GlobalInit, MapIndex},
    memory::LinearMemory,
    instance::{Imports, Import},
};


#[derive(Debug)]
pub struct LocalBacking {
    memories: Box<[LinearMemory]>,
    tables: Box<[TableBacking]>,

    pub vm_memories: Box<[vm::LocalMemory]>,
    pub vm_tables: Box<[vm::LocalTable]>,
    pub vm_globals: Box<[vm::LocalGlobal]>,
}

impl LocalBacking {
    pub fn new(module: &Module, imports: &ImportBacking) -> Self {
        let mut memories = Self::generate_memories(module);
        let mut tables = Self::generate_tables(module);
        let globals = Self::generate_globals(module);

        Self {
            memories,
            tables,

            vm_memories: Self::finalize_memories(module, &mut memories[..]),
            vm_tables: Self::finalize_tables(module, &mut tables[..]),
            vm_globals: Self::finalize_globals(module, imports, globals),
        }
    }

    fn generate_memories(module: &Module) -> Box<[LinearMemory]> {
        let mut memories = Vec::with_capacity(module.memories.len());

        for (_, mem) in &module.memories {
            // If we use emscripten, we set a fixed initial and maximum
            debug!(
                "Instance - init memory ({}, {:?})",
                mem.min, mem.max
            );
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
            debug_assert!(init.base.is_none(), "globalvar base not supported yet");
            let offset = init.offset;
            let mem: &mut LinearMemory = &mut memories[init.memory_index.index()];
            let end_of_init = offset + init.data.len();
            if end_of_init > mem.current_size() {
                let grow_pages = (end_of_init / LinearMemory::PAGE_SIZE as usize) + 1;
                mem.grow(grow_pages as u32)
                    .expect("failed to grow memory for data initializers");
            }
            let to_init = &mut mem[offset..offset + init.data.len()];
            to_init.copy_from_slice(&init.data);
        }

        memories.iter_mut().map(|mem| mem.into_vm_memory()).collect::<Vec<_>>().into_boxed_slice()
    }

    fn generate_tables(module: &Module) -> Box<[TableBacking]> {
        let mut tables = Vec::with_capacity(module.tables.len());

        for (_, table) in &module.tables {
            let table_backing = TableBacking::new(table);
            tables.push(table_backing);
        }

        tables.into_boxed_slice()
    }

    fn finalize_tables(module: &Module, tables: &[TableBacking]) -> Box<[vm::LocalTable]> {
        tables.iter().map(|table| table.into_vm_table()).collect::<Vec<_>>().into_boxed_slice()
    }

    fn generate_globals(module: &Module) -> Box<[vm::LocalGlobal]> {
        let mut globals = vec![vm::LocalGlobal::null(); module.globals.len()];

        globals.into_boxed_slice()
    }

    fn finalize_globals(module: &Module, imports: &ImportBacking, globals: Box<[vm::LocalGlobal]>) -> Box<[vm::LocalGlobal]> {
        for (to, (_, from)) in globals.iter_mut().zip(module.globals.into_iter()) {
            to.data = match from.init {
                GlobalInit::Val(Val::I32(x)) => x as u64,
                GlobalInit::Val(Val::I64(x)) => x as u64,
                GlobalInit::Val(Val::F32(x)) => x as u64,
                GlobalInit::Val(Val::F64(x)) => x,
                GlobalInit::GetGlobal(index) => unsafe { (imports.globals[index.index()].global).data },
            };
        }

        globals
    }

    // fn generate_tables(module: &Module, _options: &InstanceOptions) -> (Box<[TableBacking]>, Box<[vm::LocalTable]>) {
    //     let mut tables = Vec::new();
    //     // Reserve space for tables
    //     tables.reserve_exact(module.info.tables.len());

    //     // Get tables in module
    //     for table in &module.info.tables {
    //         let table: Vec<usize> = match table.import_name.as_ref() {
    //             Some((module_name, field_name)) => {
    //                 let imported =
    //                     import_object.get(&module_name.as_str(), &field_name.as_str());
    //                 match imported {
    //                     Some(ImportValue::Table(t)) => t.to_vec(),
    //                     None => {
    //                         if options.mock_missing_tables {
    //                             debug!(
    //                                 "The Imported table {}.{} is not provided, therefore will be mocked.",
    //                                 module_name, field_name
    //                             );
    //                             let len = table.entity.minimum as usize;
    //                             let mut v = Vec::with_capacity(len);
    //                             v.resize(len, 0);
    //                             v
    //                         } else {
    //                             panic!(
    //                                 "Imported table value was not provided ({}.{})",
    //                                 module_name, field_name
    //                             )
    //                         }
    //                     }
    //                     _ => panic!(
    //                         "Expected global table, but received {:?} ({}.{})",
    //                         imported, module_name, field_name
    //                     ),
    //                 }
    //             }
    //             None => {
    //                 let len = table.entity.minimum as usize;
    //                 let mut v = Vec::with_capacity(len);
    //                 v.resize(len, 0);
    //                 v
    //             }
    //         };
    //         tables.push(table);
    //     }

    //     // instantiate tables
    //     for table_element in &module.info.table_elements {
    //         let base = match table_element.base {
    //             Some(global_index) => globals_data[global_index.index()] as usize,
    //             None => 0,
    //         };

    //         let table = &mut tables[table_element.table_index.index()];
    //         for (i, func_index) in table_element.elements.iter().enumerate() {
    //             // since the table just contains functions in the MVP
    //             // we get the address of the specified function indexes
    //             // to populate the table.

    //             // let func_index = *elem_index - module.info.imported_funcs.len() as u32;
    //             // let func_addr = functions[func_index.index()].as_ptr();
    //             let func_addr = get_function_addr(&func_index, &import_functions, &functions);
    //             table[base + table_element.offset + i] = func_addr as _;
    //         }
    //     }
    // }
}

#[derive(Debug)]
pub struct ImportBacking {
    pub functions: Box<[vm::ImportedFunc]>,
    pub memories: Box<[vm::ImportedMemory]>,
    pub tables: Box<[vm::ImportedTable]>,
    pub globals: Box<[vm::ImportedGlobal]>,
}

impl ImportBacking {
    pub fn new(module: &Module, imports: &Imports) -> Result<Self, String> {
        assert!(module.imported_memories.len() == 0, "imported memories not yet supported");
        assert!(module.imported_tables.len() == 0, "imported tables not yet supported");

        let mut functions = Vec::with_capacity(module.imported_functions.len());
        for (index, (mod_name, item_name)) in &module.imported_functions {
            let expected_sig_index = module.signature_assoc[index];
            let expected_sig = module.signatures[expected_sig_index];
            let import = imports.get(mod_name, item_name);
            if let Some(Import::Func(func, signature)) = import {
                if &expected_sig == signature {
                    functions.push(vm::ImportedFunc {
                        func: *func,
                        // vmctx: ptr::null_mut(),
                    });
                } else {
                    return Err(format!("unexpected signature for {:?}:{:?}", mod_name, item_name));
                }
            } else {
                return Err(format!("incorrect type for {:?}:{:?}", mod_name, item_name));
            }
        }

        let mut globals = Vec::with_capacity(module.imported_globals.len());
        for (_, ((mod_name, item_name), global_desc)) in &module.imported_globals {
            let import = imports.get(mod_name, item_name);
            if let Some(Import::Global(val)) = import {
                if val.ty() == global_desc.ty {
                    globals.push(vm::ImportedGlobal {
                        global: vm::LocalGlobal {
                            data: match val {
                                Val::I32(n) => *n as u64,
                                Val::I64(n) => *n as u64,
                                Val::F32(n) => *n as u64,
                                Val::F64(n) => *n,
                            },
                        },
                    });
                } else {
                    return Err(format!("unexpected global type for {:?}:{:?}", mod_name, item_name));
                }
            } else {
                return Err(format!("incorrect type for {:?}:{:?}", mod_name, item_name));
            }
        }

        Ok(ImportBacking {
            functions: functions.into_boxed_slice(),
            memories: vec![].into_boxed_slice(),
            tables: vec![].into_boxed_slice(),
            globals: globals.into_boxed_slice(),
        })
    }
}