//! A webassembly::Instance object is a stateful, executable instance of a
//! webassembly::Module.  Instance objects contain all the Exported
//! WebAssembly functions that allow calling into WebAssembly code.

//! The webassembly::Instance() constructor function can be called to
//! synchronously instantiate a given webassembly::Module object. However, the
//! primary way to get an Instance is through the asynchronous
//! webassembly::instantiateStreaming() function.
use cranelift_codegen::{isa, Context};
use cranelift_entity::EntityRef;
use cranelift_wasm::{FuncIndex, GlobalInit};
use memmap::MmapMut;
use region;
use spin::RwLock;
use std::marker::PhantomData;
use std::sync::Arc;
use std::{mem, slice};

use super::super::common::slice::{BoundedSlice, UncheckedSlice};
use super::errors::ErrorKind;
use super::memory::LinearMemory;
use super::module::Module;
use super::module::{DataInitializer, Exportable};
use super::relocation::{RelocSink, TrapSink};

pub fn get_function_addr(
    base: *const (),
    functions: &[usize],
    func_index: &FuncIndex,
) -> *const () {
    let offset = functions[func_index.index()];
    (base as usize + offset) as _
}

/// Zero-sized, non-instantiable type.
pub enum VmCtx {}

impl VmCtx {
    pub fn data(&self) -> &VmCtxData {
        let heap_ptr = self as *const _ as *const VmCtxData;
        unsafe { &*heap_ptr.sub(1) }
    }

    /// This is safe because the offset is 32 bits and thus
    /// cannot extend out of the guarded wasm memory.
    pub fn fastpath_offset_ptr<T>(&self, offset: u32) -> *const T {
        let heap_ptr = self as *const _ as *const u8;
        unsafe { heap_ptr.add(offset as usize) as *const T }
    }
}

#[repr(C)]
pub struct VmCtxData<'phantom> {
    pub user_data: UserData,
    globals: UncheckedSlice<u8>,
    memories: UncheckedSlice<UncheckedSlice<u8>>,
    tables: UncheckedSlice<BoundedSlice<usize>>,
    phantom: PhantomData<&'phantom ()>,
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
    pub fn new(module: &Module, code_base: *const ()) -> Result<Instance, ErrorKind> {
        let mut tables: Vec<Vec<usize>> = Vec::new();
        let mut memories: Vec<LinearMemory> = Vec::new();
        let mut globals: Vec<u8> = Vec::new();

        let mut functions: Vec<usize> = Vec::with_capacity(module.info.function_bodies.len());
        // Instantiate functions
        {
            let isa = isa::lookup(module.info.triple.clone())
                .unwrap()
                .finish(module.info.flags.clone());

            let mut total_size: usize = 0;
            let mut context_and_offsets = Vec::with_capacity(module.info.function_bodies.len());

            // Compile the functions (from cranelift IR to machine code)
            for function_body in module.info.function_bodies.values() {
                let mut func_context = Context::for_function(function_body.to_owned());
                func_context.verify(&*isa).map_err(|e| ErrorKind::CompileError(e.to_string()))?;
                func_context.verify_locations(&*isa).map_err(|e| ErrorKind::CompileError(e.to_string()))?;
                let code_size_offset = func_context
                    .compile(&*isa)
                    .map_err(|e| ErrorKind::CompileError(e.to_string()))?
                    as usize;
                total_size += code_size_offset;
                context_and_offsets.push((func_context, code_size_offset));
            }

            // We only want to allocate in memory if there is more than
            // 0 functions. Otherwise reserving a 0-sized memory
            // cause a panic error
            if total_size > 0 {
                // Allocate the total memory for this functions
                let map = MmapMut::map_anon(total_size).unwrap();
                let region_start = map.as_ptr();

                // // Emit this functions to memory
                for (ref func_context, func_offset) in context_and_offsets.iter() {
                    let mut trap_sink = TrapSink::new(*func_offset);
                    let mut reloc_sink = RelocSink::new();
                    unsafe {
                        func_context.emit_to_memory(
                            &*isa,
                            (region_start as usize + func_offset) as *mut u8,
                            &mut reloc_sink,
                            &mut trap_sink,
                        );
                    };
                }

                // Set protection of this memory region to Read + Execute
                // so we are able to execute the functions emitted to memory
                unsafe {
                    region::protect(region_start, total_size, region::Protection::ReadExecute)
                        .expect("unable to make memory readable+executable");
                }
            }
        }

        // Instantiate tables
        {
            // Reserve table space
            tables.reserve_exact(module.info.tables.len());
            for table in &module.info.tables {
                let len = table.entity.size;
                let mut v = Vec::with_capacity(len);
                v.resize(len, 0);
                tables.push(v);
            }
            // instantiate tables
            for table_element in &module.info.table_elements {
                assert!(
                    table_element.base.is_none(),
                    "globalvalue base not supported yet."
                );
                let base = 0;

                let table = &mut tables[table_element.table_index];
                for (i, func_index) in table_element.elements.iter().enumerate() {
                    // since the table just contains functions in the MVP
                    // we get the address of the specified function indexes
                    // to populate the table.

                    // let func_index = *elem_index - module.info.imported_funcs.len() as u32;
                    let func_addr = get_function_addr(code_base, &functions, *&func_index);
                    table[base + table_element.offset + i] = func_addr as _;
                }
            }
        }

        // Instantiate memories
        {
            // Allocate the underlying memory and initialize it to all zeros.
            memories.reserve_exact(module.info.memories.len());
            for memory in &module.info.memories {
                let memory = memory.entity;
                let v =
                    LinearMemory::new(memory.pages_count as u32, memory.maximum.map(|m| m as u32));
                memories.push(v);
            }
            for init in &module.info.data_initializers {
                debug_assert!(init.base.is_none(), "globalvar base not supported yet");
                let mem_mut = memories[init.memory_index].as_mut();
                let to_init = &mut mem_mut[init.offset..init.offset + init.data.len()];
                to_init.copy_from_slice(&init.data);
            }
        }

        // Instantiate Globals
        {
            let globals_count = module.info.globals.len();
            // Allocate the underlying memory and initialize it to zeros
            let globals_data_size = globals_count * 8;
            globals.resize(globals_data_size, 0);

            // cast the globals slice to a slice of i64.
            let globals_data = unsafe {
                slice::from_raw_parts_mut(globals.as_mut_ptr() as *mut i64, globals_count)
            };
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
        }

        Ok(Instance {
            tables: Arc::new(tables.into_iter().map(|table| RwLock::new(table)).collect()),
            memories: Arc::new(memories.into_iter().collect()),
            globals: globals,
        })
    }

    pub fn memories(&self) -> Arc<Vec<LinearMemory>> {
        self.memories.clone()
    }
    
    /// Invoke a WebAssembly function given a FuncIndex and the
    /// arguments that the function should be called with
    pub fn invoke(&self, func_index: FuncIndex, args: Vec<i32>) {
        unimplemented!()
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
