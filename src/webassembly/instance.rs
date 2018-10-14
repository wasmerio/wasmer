//! An 'Instance' contains all the runtime state used by execution of a wasm module

use cranelift_wasm::{GlobalInit, FuncIndex};
use super::env::Module;
use super::env::{DataInitializer, Exportable};
use cranelift_entity::EntityRef;

use super::memory::LinearMemory;
use std::marker::PhantomData;
use std::{slice, mem};
use std::sync::Arc;

use spin::RwLock;
use super::super::common::slice::{BoundedSlice, UncheckedSlice};

pub fn get_function_addr(base: *const (), functions: &[usize], func_index: &FuncIndex) -> *const () {
    let offset = functions[func_index.index()];
    (base as usize + offset) as _
}

/// Zero-sized, non-instantiable type.
pub enum VmCtx {}

impl VmCtx {
    pub fn data(&self) -> &VmCtxData {
        let heap_ptr = self as *const _ as *const VmCtxData;
        unsafe {
            &*heap_ptr.sub(1)
        }
    }

    /// This is safe because the offset is 32 bits and thus
    /// cannot extend out of the guarded wasm memory.
    pub fn fastpath_offset_ptr<T>(&self, offset: u32) -> *const T {
        let heap_ptr = self as *const _ as *const u8;
        unsafe {
            heap_ptr.add(offset as usize) as *const T
        }
    }
}

#[repr(C)]
pub struct VmCtxData<'a> {
    pub user_data: UserData,
    globals: UncheckedSlice<u8>,
    memories: UncheckedSlice<UncheckedSlice<u8>>,
    tables: UncheckedSlice<BoundedSlice<usize>>,
    phantom: PhantomData<&'a ()>,
}

#[repr(C)]
pub struct UserData {
    // pub process: Dispatch<Process>,
    pub instance: Instance,
}


/// An Instance of a WebAssembly module
#[derive(Debug)]
pub struct Instance {
    /// WebAssembly table data
    pub tables: Arc<Vec<RwLock<Vec<usize>>>>,

    /// WebAssembly linear memory data
    pub memories: Arc<Vec<LinearMemory>>,

    /// WebAssembly global variable data
    pub globals: Vec<u8>,
}

impl Instance {
    /// Create a new `Instance`.
    pub fn new(module: &Module, data_initializers: &[DataInitializer], code_base: *const (), functions: &[usize]) -> Instance {
        let mut tables: Vec<Vec<usize>> = Vec::new();
        let mut memories: Vec<LinearMemory> = Vec::new();
        let mut globals: Vec<u8> = Vec::new();

        // instantiate_tables
        {
            tables.reserve_exact(module.info.tables.len());
            for table in &module.info.tables {
                let len = table.entity.size;
                let mut v = Vec::with_capacity(len);
                v.resize(len, 0);
                tables.push(v);
            }
            // instantiate tables
            for table_element in &module.info.table_elements {
                assert!(table_element.base.is_none(), "globalvalue base not supported yet.");
                let base = 0;

                let table = &mut tables[table_element.table_index];
                for (i, func_index) in table_element.elements.iter().enumerate() {
                    // since the table just contains functions in the MVP
                    // we get the address of the specified function indexes
                    // to populate the table.

                    // let func_index = *elem_index - module.info.imported_funcs.len() as u32;

                    let func_addr = get_function_addr(code_base, functions, *&func_index);
                    table[base + table_element.offset + i] = func_addr as _;
                }
            }
        };

        // instantiate_memories
        {
            // Allocate the underlying memory and initialize it to all zeros.
            memories.reserve_exact(module.info.memories.len());
            for memory in &module.info.memories {
                let memory = memory.entity;
                let v = LinearMemory::new(memory.pages_count as u32, memory.maximum.map(|m| m as u32));
                memories.push(v);
            }
            for init in data_initializers {
                debug_assert!(init.base.is_none(), "globalvar base not supported yet");
                let mem_mut = memories[init.memory_index].as_mut();
                let to_init = &mut mem_mut[init.offset..init.offset + init.data.len()];
                to_init.copy_from_slice(&init.data);
            }
        };

        // instantiate_globals
        {
            let globals_count = module.info.globals.len();
            // Allocate the underlying memory and initialize it to zeros
            let globals_data_size = globals_count * 8;
            globals.resize(globals_data_size, 0);

            // cast the globals slice to a slice of i64.
            let globals_data = unsafe { slice::from_raw_parts_mut(globals.as_mut_ptr() as *mut i64, globals_count) };
            for (i, global) in module.info.globals.iter().enumerate() {
                let value: i64 = match global.entity.initializer {
                    GlobalInit::I32Const(n) => n as _,
                    GlobalInit::I64Const(n) => n,
                    GlobalInit::F32Const(f) => unsafe { mem::transmute(f as f64) },
                    GlobalInit::F64Const(f) => unsafe { mem::transmute(f) },
                    _ => unimplemented!(),
                };
                
                globals_data[i] = value;
            }
        };

        Instance {
            tables: Arc::new(tables.into_iter().map(|table| RwLock::new(table)).collect()),
            memories: Arc::new(memories.into_iter().collect()),
            globals: globals,
        }
    }

    pub fn memories(&self) -> Arc<Vec<LinearMemory>> {
        self.memories.clone()
    }

    // pub fn start_func(&self) -> extern fn(&VmCtx) {
    //     self.start_func
    // }

}

impl Clone for Instance {
    fn clone(&self) -> Instance {
        Instance {
            tables: Arc::clone(&self.tables),
            memories: Arc::clone(&self.memories),
            globals: self.globals.clone(),
        }
    }
}
