//! A webassembly::Instance object is a stateful, executable instance of a
//! webassembly::Module.  Instance objects contain all the Exported
//! WebAssembly functions that allow calling into WebAssembly code.

//! The webassembly::Instance() constructor function can be called to
//! synchronously instantiate a given webassembly::Module object. However, the
//! primary way to get an Instance is through the asynchronous
//! webassembly::instantiateStreaming() function.
use cranelift_codegen::{binemit, isa, Context};
use cranelift_entity::EntityRef;
use cranelift_wasm::{FuncIndex, GlobalInit};
use memmap::MmapMut;
use region;
use spin::RwLock;
use std::iter::Iterator;
use std::marker::PhantomData;
use std::ptr::{self, write_unaligned};
use std::sync::Arc;
use std::{mem, slice};

use super::super::common::slice::{BoundedSlice, UncheckedSlice};
use super::errors::ErrorKind;
use super::memory::LinearMemory;
use super::module::Module;
use super::module::{DataInitializer, Export, Exportable};
use super::relocation::{Reloc, RelocSink, RelocationType, TrapSink};

pub fn protect_codebuf(code_buf: &Vec<u8>) -> Result<(), String> {
    match unsafe {
        region::protect(
            code_buf.as_ptr(),
            code_buf.len(),
            region::Protection::ReadWriteExecute,
        )
    } {
        Err(err) => {
            return Err(format!(
                "failed to give executable permission to code: {}",
                err
            ))
        }
        Ok(()) => Ok(()),
    }
}

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
    // pub tables: Arc<Vec<RwLock<Vec<usize>>>>,
    pub tables: Arc<Vec<Vec<usize>>>,

    /// WebAssembly linear memory data
    pub memories: Arc<Vec<LinearMemory>>,

    /// WebAssembly global variable data
    pub globals: Vec<u8>,

    /// Webassembly functions
    // functions: Vec<usize>,
    functions: Vec<Vec<u8>>,

    /// The module start function
    start_func: Option<FuncIndex>,
    // Region start memory location
    // code_base: *const (),
}

// pub fn make_vmctx(instance: &mut Instance, mem_base_addrs: &mut [*mut u8]) -> Vec<*mut u8> {
//     debug_assert!(
//         instance.tables.len() <= 1,
//         "non-default tables is not supported"
//     );

//     let (default_table_ptr, default_table_len) = instance
//         .tables
//         .get_mut(0)
//         .map(|table| (table.as_mut_ptr() as *mut u8, table.len()))
//         .unwrap_or((ptr::null_mut(), 0));

//     let mut vmctx = Vec::new();
//     vmctx.push(instance.globals.as_mut_ptr());
//     vmctx.push(mem_base_addrs.as_mut_ptr() as *mut u8);
//     vmctx.push(default_table_ptr);
//     vmctx.push(default_table_len as *mut u8);
//     vmctx.push(instance as *mut Instance as *mut u8);

//     vmctx
// }

impl Instance {
    /// Create a new `Instance`.
    pub fn new(module: &Module) -> Result<Instance, ErrorKind> {
        let mut tables: Vec<Vec<usize>> = Vec::new();
        let mut memories: Vec<LinearMemory> = Vec::new();
        let mut globals: Vec<u8> = Vec::new();
        let mut functions: Vec<Vec<u8>> = Vec::new();
        // let mut code_base: *const () = ptr::null();

        // Instantiate functions
        {
            functions.reserve_exact(module.info.function_bodies.len());
            let isa = isa::lookup(module.info.triple.clone())
                .unwrap()
                .finish(module.info.flags.clone());

            // let mut total_size: usize = 0;
            let mut context_and_offsets = Vec::with_capacity(module.info.function_bodies.len());
            let mut relocations = Vec::new();
            // Compile the functions (from cranelift IR to machine code)
            for function_body in module.info.function_bodies.values() {
                let mut func_context = Context::for_function(function_body.to_owned());
                // func_context
                //     .verify(&*isa)
                //     .map_err(|e| ErrorKind::CompileError(e.to_string()))?;
                // func_context
                //     .verify_locations(&*isa)
                //     .map_err(|e| ErrorKind::CompileError(e.to_string()))?;
                // let code_size_offset = func_context
                //     .compile(&*isa)
                //     .map_err(|e| ErrorKind::CompileError(e.to_string()))?
                //     as usize;

                let mut code_buf: Vec<u8> = Vec::new();
                let mut reloc_sink = RelocSink::new();
                let mut trap_sink = binemit::NullTrapSink {};

                func_context
                    .compile_and_emit(&*isa, &mut code_buf, &mut reloc_sink, &mut trap_sink)
                    .map_err(|e| ErrorKind::CompileError(e.to_string()))?;
                protect_codebuf(&code_buf);

                let func_offset = code_buf;
                functions.push(func_offset);

                context_and_offsets.push(func_context);
                relocations.push(reloc_sink.func_relocs);
                // println!("FUNCTION RELOCATIONS {:?}", reloc_sink.func_relocs)
                // total_size += code_size_offset;
            }

            // For each of the functions used, we see what are the calls inside this functions
            // and relocate each call to the proper memory address.
            // The relocations are relative to the relocation's address plus four bytes
            // TODO: Support architectures other than x64, and other reloc kinds.
            for (i, function_relocs) in relocations.iter().enumerate() {
                // for r in function_relocs {
                for (ref reloc, ref reloc_type) in function_relocs {
                    let target_func_address: isize = match reloc_type {
                        RelocationType::Normal(func_index) => {
                            functions[*func_index as usize].as_ptr() as isize
                        },
                        _ => unimplemented!()
                        // RelocationType::Intrinsic(name) => {
                        //     get_abi_intrinsic(name)?
                        // },
                        // RelocationTarget::UserFunc(index) => {
                        //     functions[module.defined_func_index(index).expect(
                        //         "relocation to imported function not supported yet",
                        //     )].as_ptr() as isize
                        // }
                        // RelocationTarget::GrowMemory => grow_memory as isize,
                        // RelocationTarget::CurrentMemory => current_memory as isize,
                    };
                    // print!("FUNCTION {:?}", target_func_address);
                    let body = &mut functions[i];
                    match reloc.reloc {
                        Reloc::Abs8 => unsafe {
                            let reloc_address =
                                body.as_mut_ptr().offset(reloc.offset as isize) as i64;
                            let reloc_addend = reloc.addend;
                            let reloc_abs = target_func_address as i64 + reloc_addend;
                            write_unaligned(reloc_address as *mut i64, reloc_abs);
                        },
                        Reloc::X86PCRel4 => unsafe {
                            let reloc_address =
                                body.as_mut_ptr().offset(reloc.offset as isize) as isize;
                            let reloc_addend = reloc.addend as isize;
                            // TODO: Handle overflow.
                            let reloc_delta_i32 =
                                (target_func_address - reloc_address + reloc_addend) as i32;
                            write_unaligned(reloc_address as *mut i32, reloc_delta_i32);
                        },
                        _ => panic!("unsupported reloc kind"),
                    }
                    // let reloc_address = unsafe {
                    //     (target_func_address.to_owned().as_mut_ptr() as *const u8).offset(reloc.offset as isize)
                    // };

                    // match reloc.reloc {
                    //     Reloc::Abs8 => {
                    //         unsafe {
                    //             // (reloc_address as *mut usize).write(target_func_address.to_owned().as_ptr() as usize);
                    //         }
                    //     }
                    //     _ => unimplemented!()
                    // }

                    // let target_func_address: isize = match r.reloc_target {
                    //     RelocationTarget::UserFunc(index) => {
                    //         functions[module.defined_func_index(index).expect(
                    //             "relocation to imported function not supported yet",
                    //         )].as_ptr() as isize
                    //     }
                    //     RelocationTarget::GrowMemory => grow_memory as isize,
                    //     RelocationTarget::CurrentMemory => current_memory as isize,
                    // };

                    // let body = &mut functions[i];
                    // match r.reloc {
                    //     Reloc::Abs8 => unsafe {
                    //         let reloc_address = body.as_mut_ptr().offset(r.offset as isize) as i64;
                    //         let reloc_addend = r.addend;
                    //         let reloc_abs = target_func_address as i64 + reloc_addend;
                    //         write_unaligned(reloc_address as *mut i64, reloc_abs);
                    //     },
                    //     Reloc::X86PCRel4 => unsafe {
                    //         let reloc_address = body.as_mut_ptr().offset(r.offset as isize) as isize;
                    //         let reloc_addend = r.addend as isize;
                    //         // TODO: Handle overflow.
                    //         let reloc_delta_i32 =
                    //             (target_func_address - reloc_address + reloc_addend) as i32;
                    //         write_unaligned(reloc_address as *mut i32, reloc_delta_i32);
                    //     },
                    //     _ => panic!("unsupported reloc kind"),
                    // }
                }
            }

            // We only want to allocate in memory if there is more than
            // 0 functions. Otherwise reserving a 0-sized memory region
            // cause a panic error
            // if total_size > 0 {
            //     // Allocate the total memory for this functions
            //     // let map = MmapMut::map_anon(total_size).unwrap();
            //     // let region_start = map.as_ptr() as usize;
            //     // code_base = map.as_ptr() as *const ();

            //     // // Emit this functions to memory
            //     for (ref func_context, func_offset) in context_and_offsets.iter() {
            //         let mut trap_sink = TrapSink::new(*func_offset);
            //         let mut reloc_sink = RelocSink::new();
            //         let mut code_buf: Vec<u8> = Vec::new();

            //         // let mut func_pointer =  as *mut u8;
            //         unsafe {
            //             func_context.emit_to_memory(
            //                 &*isa,
            //                 &mut code_buf,
            //                 &mut reloc_sink,
            //                 &mut trap_sink,
            //             );
            //         };
            //         let func_offset = code_buf.as_ptr() as usize;
            //         functions.push(*func_offset);
            //     }

            //     // Set protection of this memory region to Read + Execute
            //     // so we are able to execute the functions emitted to memory
            //     // unsafe {
            //     //     region::protect(region_start as *mut u8, total_size, region::Protection::ReadExecute)
            //     //         .expect("unable to make memory readable+executable");
            //     // }
            // }
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
                    let func_addr = functions[func_index.index()].as_ptr();
                    // let func_addr = get_function_addr(code_base, &functions, *&func_index);
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

        let start_func: Option<FuncIndex> =
            module
                .info
                .start_func
                .or_else(|| match module.info.exports.get("main") {
                    Some(Export::Function(index)) => Some(index.to_owned()),
                    _ => None,
                });

        Ok(Instance {
            tables: Arc::new(tables.into_iter().collect()), // tables.into_iter().map(|table| RwLock::new(table)).collect()),
            memories: Arc::new(memories.into_iter().collect()),
            globals: globals,
            functions: functions,
            start_func: start_func
            // code_base: code_base,
        })
    }

    pub fn memories(&self) -> Arc<Vec<LinearMemory>> {
        self.memories.clone()
    }

    /// Invoke a WebAssembly function given a FuncIndex and the
    /// arguments that the function should be called with
    pub fn get_function<T>(&self, func_index: FuncIndex) -> (fn() -> T) {
        // let mut mem_base_addrs = self
        //     .memories
        //     .iter_mut()
        //     .map(LinearMemory::base_addr)
        //     .collect::<Vec<_>>();
        // let vmctx = make_vmctx(&mut self, &mut mem_base_addrs);

        // let vmctx = make_vmctx(instance, &mut mem_base_addrs);
        // let vmctx = ptr::null();
        // Rather than writing inline assembly to jump to the code region, we use the fact that
        // the Rust ABI for calling a function with no arguments and no return matches the one of
        // the generated code. Thanks to this, we can transmute the code region into a first-class
        // Rust function and call it.
        // let func_pointer = get_function_addr(self.code_base, &self.functions, &func_index);
        let func_pointer = &self.functions[func_index.index()];
        unsafe {
            let func = mem::transmute::<_, fn() -> T>(func_pointer.as_ptr());
            func
            // let result = func(2);
            // println!("FUNCTION INVOKED, result {:?}", result);

            // start_func(vmctx.as_ptr());
        }
    }

    pub fn invoke(&self, func_index: FuncIndex, _args: Vec<u8>) -> i32 {
        let func: fn() -> i32 = self.get_function(func_index);
        let result = func();
        println!("RESULT {:?}", result);
        result
    }

    pub fn start(&self) {
        if let Some(func_index) = self.start_func {
            // let vmctx: &VmCtx = ptr::null();
            let func: fn() = self.get_function(func_index);
            func()
        }
    }

    // pub fn generate_context(&mut self) -> &VmCtx {
    //     let memories: Vec<UncheckedSlice<u8>> = self.memories.iter()
    //         .map(|mem| mem.into())
    //         .collect();

    //     let tables: Vec<BoundedSlice<usize>> = self.tables.iter()
    //         .map(|table| table.write()[..].into())
    //         .collect();

    //     let globals: UncheckedSlice<u8> = self.globals[..].into();

    //     assert!(memories.len() >= 1, "modules must have at least one memory");
    //     // the first memory has a space of `mem::size_of::<VmCtxData>()` rounded
    //     // up to the 4KiB before it. We write the VmCtxData into that.
    //     let data = VmCtxData {
    //         globals: globals,
    //         memories: memories[1..].into(),
    //         tables: tables[..].into(),
    //         user_data: UserData {
    //             // process,
    //             instance,
    //         },
    //         phantom: PhantomData,
    //     };

    //     let main_heap_ptr = memories[0].as_mut_ptr() as *mut VmCtxData;
    //     unsafe {
    //         main_heap_ptr
    //             .sub(1)
    //             .write(data);
    //         &*(main_heap_ptr as *const VmCtx)
    //     }
    // }

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
            functions: self.functions.clone(),
            start_func: self.start_func.clone(),
            // code_base: self.code_base,
        }
    }
}

extern "C" fn grow_memory(size: u32, memory_index: u32, vmctx: *mut *mut u8) -> u32 {
    unimplemented!();
    // unsafe {
    //     let instance = (*vmctx.offset(4)) as *mut Instance;
    //     (*instance)
    //         .memory_mut(memory_index as MemoryIndex)
    //         .grow(size)
    //         .unwrap_or(u32::max_value())
    // }
}

extern "C" fn current_memory(memory_index: u32, vmctx: *mut *mut u8) -> u32 {
    unimplemented!();
    // unsafe {
    //     let instance = (*vmctx.offset(4)) as *mut Instance;
    //     (*instance)
    //         .memory_mut(memory_index as MemoryIndex)
    //         .current_size()
    // }
}
