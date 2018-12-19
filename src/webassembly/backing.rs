use super::vm;
use super::module::Module;
use super::table::{TableBacking, TableScheme};
use super::memory::LinearMemory;
use super::instance::{InstanceOptions, InstanceABI};
use super::ImportObject;
use cranelift_entity::EntityRef;

#[derive(Debug)]
pub struct Backing {
    memories: Box<[LinearMemory]>,
    tables: Box<[TableBacking]>,

    vm_memories: Box<[vm::LocalMemory]>,
    vm_tables: Box<[vm::LocalTable]>,
    vm_globals: Box<[vm::LocalGlobal]>,
}

impl Backing {
    pub fn new(module: &Module, options: &InstanceOptions, imports: &ImportObject) -> Self {
        let memories = Backing::generate_memories(module, options);
        let tables = Backing::generate_tables(module, options);

        Backing {
            memories,
            tables,

            vm_memories: Backing::finalize_memories(module, &memories, options),
            vm_tables: Backing::finalize_tables(module, &tables, options, imports),
            vm_globals: Backing::generate_globals(module),
        }
    }

    fn generate_memories(module: &Module, options: &InstanceOptions) -> Box<[LinearMemory]> {
        let memories = Vec::with_capacity(module.info.memories.len());

        for mem in &module.info.memories {
            let memory = mem.entity;
            // If we use emscripten, we set a fixed initial and maximum
            debug!(
                "Instance - init memory ({}, {:?})",
                memory.minimum, memory.maximum
            );
            let memory = if options.abi == InstanceABI::Emscripten {
                // We use MAX_PAGES, so at the end the result is:
                // (initial * LinearMemory::PAGE_SIZE) == LinearMemory::DEFAULT_HEAP_SIZE
                // However, it should be: (initial * LinearMemory::PAGE_SIZE) == 16777216
                LinearMemory::new(LinearMemory::MAX_PAGES, None)
            } else {
                LinearMemory::new(memory.minimum, memory.maximum.map(|m| m as u32))
            };
            memories.push(memory);
        }

        memories.into_boxed_slice()
    }

    fn finalize_memories(module: &Module, memories: &[LinearMemory], options: &InstanceOptions) -> Box<[vm::LocalMemory]> {
        for init in &module.info.data_initializers {
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

        if options.abi == InstanceABI::Emscripten {
            debug!("emscripten::setup memory");
            crate::apis::emscripten::emscripten_set_up_memory(&mut memories[0]);
            debug!("emscripten::finish setup memory");
        }

        memories.iter().map(|mem| mem.into_vm_memory()).collect::<Vec<_>>().into_boxed_slice()
    }

    fn generate_tables(module: &Module, options: &InstanceOptions) -> Box<[TableBacking]> {
        let mut tables = Vec::with_capacity(module.info.tables.len());

        for table in &module.info.tables {
             let scheme = TableScheme::from_table(table.entity);
             let table_backing = TableBacking::new(&scheme);
             tables.push(table_backing);
        }

        tables.into_boxed_slice()
    }

    fn finalize_tables(module: &Module, tables: &[TableBacking], options: &InstanceOptions, imports: &ImportObject) -> Box<[vm::LocalTable]> {
        tables.iter().map(|table| table.into_vm_table()).collect::<Vec<_>>().into_boxed_slice()
    }

    fn generate_globals(module: &Module) -> Box<[vm::LocalGlobal]> {
        let mut globals = Vec::with_capacity(module.info.globals.len());

        for global in module.info.globals.iter().map(|mem| mem.entity) {
            
        }

        globals.into_boxed_slice()
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
pub struct ImportsBacking {
    functions: Box<[vm::ImportedFunc]>,
    memories: Box<[vm::ImportedMemory]>,
    tables: Box<[vm::ImportedTable]>,
    globals: Box<[vm::ImportedGlobal]>,
}