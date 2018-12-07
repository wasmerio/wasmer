//! A webassembly::Instance object is a stateful, executable instance of a
//! webassembly::Module.  Instance objects contain all the Exported
//! WebAssembly functions that allow calling into WebAssembly code.

//! The webassembly::Instance() constructor function can be called to
//! synchronously instantiate a given webassembly::Module object. However, the
//! primary way to get an Instance is through the asynchronous
//! webassembly::instantiate_streaming() function.
use cranelift_codegen::ir::{Function, LibCall};
use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::{binemit, Context};
use cranelift_entity::EntityRef;
use cranelift_wasm::{FuncIndex, GlobalInit};
use rayon::prelude::*;
use indicatif::{ProgressBar, ProgressStyle};
use console::style;

use region;
use std::iter::FromIterator;
use std::iter::Iterator;
use std::mem::size_of;
use std::ptr::write_unaligned;
use std::sync::Arc;
use std::{fmt, mem, slice};

use super::super::common::slice::{BoundedSlice, UncheckedSlice};
use super::errors::ErrorKind;
use super::import_object::{ImportObject, ImportValue};
use super::math_intrinsics;
use super::memory::LinearMemory;
use super::module::{Export, ImportableExportable, Module};
use super::relocation::{Reloc, RelocSink, RelocationType};

type TablesSlice = UncheckedSlice<BoundedSlice<usize>>;
// TODO: this should be `type MemoriesSlice = UncheckedSlice<UncheckedSlice<u8>>;`, but that crashes for some reason.
type MemoriesSlice = UncheckedSlice<BoundedSlice<u8>>;
type GlobalsSlice = UncheckedSlice<u8>;

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

fn get_function_addr(
    func_index: &FuncIndex,
    import_functions: &Vec<*const u8>,
    functions: &Vec<Vec<u8>>,
) -> *const u8 {
    let index = func_index.index();
    let len = import_functions.len();
    let func_pointer = if index < len {
        import_functions[index]
    } else {
        (functions[index - len]).as_ptr()
    };
    func_pointer
}

pub struct EmscriptenData {
    pub malloc: extern "C" fn(i32, &Instance) -> u32,
    pub free: extern "C" fn(i32, &mut Instance),
    pub memalign: extern "C" fn (u32, u32, &mut Instance) -> u32,
    pub memset: extern "C" fn(u32, i32, u32, &mut Instance) -> u32,
    pub stack_alloc: extern "C" fn (u32, &Instance) -> u32,
}

impl fmt::Debug for EmscriptenData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("EmscriptenData")
            .field("malloc", &(self.malloc as usize))
            .field("free", &(self.free as usize))
            .finish()
    }
}

/// An Instance of a WebAssembly module
/// NOTE: There is an assumption that data_pointers is always the
///      first field
#[derive(Debug)]
#[repr(C)]
pub struct Instance {
    // C-like pointers to data (heaps, globals, tables)
    pub data_pointers: DataPointers,

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

    /// Imported functions
    import_functions: Vec<*const u8>,

    /// The module start function
    pub start_func: Option<FuncIndex>,
    // Region start memory location
    // code_base: *const (),
    pub emscripten_data: Option<EmscriptenData>,
}

/// Contains pointers to data (heaps, globals, tables) needed
/// by Cranelift.
/// NOTE: Rearranging the fields will break the memory arrangement model
#[derive(Debug)]
#[repr(C)]
pub struct DataPointers {
    // Pointer to tables
    pub tables: TablesSlice,

    // Pointer to memories
    pub memories: MemoriesSlice,

    // Pointer to globals
    pub globals: GlobalsSlice,
}

pub struct InstanceOptions {
    // Shall we mock automatically the imported functions if they don't exist?
    pub mock_missing_imports: bool,
    pub mock_missing_globals: bool,
    pub mock_missing_tables: bool,
    pub use_emscripten: bool,
    pub show_progressbar: bool,
    pub isa: Box<TargetIsa>,
}

extern "C" fn mock_fn() -> i32 {
    debug!("CALLING MOCKED FUNC");
    return 0;
}

#[allow(dead_code)]
struct CompiledFunction {
    code_buf: Vec<u8>,
    reloc_sink: RelocSink,
    trap_sink: binemit::NullTrapSink,
}

fn compile_function(
    isa: &TargetIsa,
    function_body: &Function,
) -> Result<CompiledFunction, ErrorKind> {
    let mut func_context = Context::for_function(function_body.to_owned());

    let mut code_buf: Vec<u8> = Vec::new();
    let mut reloc_sink = RelocSink::new();
    let mut trap_sink = binemit::NullTrapSink {};

    func_context
        .compile_and_emit(isa, &mut code_buf, &mut reloc_sink, &mut trap_sink)
        .map_err(|e| {
            debug!("CompileError: {}", e.to_string());
            ErrorKind::CompileError(e.to_string())
        })?;

    Ok(CompiledFunction {
        code_buf,
        reloc_sink,
        trap_sink,
    })
}

impl Instance {
    pub const TABLES_OFFSET: usize = 0; // 0 on 64-bit | 0 on 32-bit
    pub const MEMORIES_OFFSET: usize = size_of::<TablesSlice>(); // 8 on 64-bit | 4 on 32-bit
    pub const GLOBALS_OFFSET: usize = Instance::MEMORIES_OFFSET + size_of::<MemoriesSlice>(); // 16 on 64-bit | 8 on 32-bit

    /// Create a new `Instance`.
    /// TODO: Raise an error when expected import is not part of imported object
    ///     Also make sure imports that are not declared do not get added to the instance
    pub fn new(
        module: &Module,
        import_object: ImportObject<&str, &str>,
        options: InstanceOptions,
    ) -> Result<Instance, ErrorKind> {
        let mut tables: Vec<Vec<usize>> = Vec::new();
        let mut memories: Vec<LinearMemory> = Vec::new();
        let mut globals: Vec<u8> = Vec::new();

        let mut functions: Vec<Vec<u8>> = Vec::new();
        let mut import_functions: Vec<*const u8> = Vec::new();

        debug!("Instance - Instantiating functions");
        // Instantiate functions
        {
            functions.reserve_exact(module.info.functions.len());
            let mut relocations = Vec::new();

            // let imported_functions: Vec<String> = module.info.imported_funcs.iter().map(|(module, field)| {
            //     format!(" * {}.{}", module, field)
            // }).collect();

            // println!("Instance imported functions: \n{}", imported_functions.join("\n"));

            // We walk through the imported functions and set the relocations
            // for each of this functions to be an empty vector (as is defined outside of wasm)
            for (module, field) in module.info.imported_funcs.iter() {
                let imported = import_object.get(&module.as_str(), &field.as_str());
                let function: &*const u8 = match imported {
                    Some(ImportValue::Func(f)) => f,
                    None => {
                        if options.mock_missing_imports {
                            debug!(
                                "The import {}.{} is not provided, therefore will be mocked.",
                                module, field
                            );
                            &(mock_fn as _)
                        } else {
                            return Err(ErrorKind::LinkError(format!(
                                "Imported function {}.{} was not provided in the import_functions",
                                module, field
                            )));
                        }
                    }
                    other => panic!("Expected function import, received {:?}", other),
                };
                // println!("GET FUNC {:?}", function);
                import_functions.push(*function);
                relocations.push(vec![]);
            }

            debug!("Instance - Compiling functions");
            // Compile the functions (from cranelift IR to machine code)
            let values: Vec<&Function> = Vec::from_iter(module.info.function_bodies.values());
            // let isa: &TargetIsa = &*options.isa;

            let progress_bar_option = if options.show_progressbar {
                let progress_bar = ProgressBar::new(module.info.functions.len() as u64);
                progress_bar.set_style(ProgressStyle::default_bar()
                    .template(&format!("{{spinner:.green}} {} [{{bar:40}}] {} {{msg}}", style("Compiling").bold(), style("{percent}%").bold().dim()))
                    .progress_chars("=> "));
                Some(progress_bar)
            } else {
                None
            };

            let compiled_funcs: Vec<CompiledFunction> = values
                .par_iter()
                .map(|function_body| -> CompiledFunction {
                    // let r = *Arc::from_raw(isa_ptr);
                    let func = compile_function(&*options.isa, function_body).unwrap();
                    if let Some(ref progress_bar) = progress_bar_option {
                        progress_bar.inc(1);
                    };
                    func
                    // unimplemented!()
                }).collect();

            if let Some(ref progress_bar) = progress_bar_option {
                progress_bar.set_style(ProgressStyle::default_bar()
                    .template(&format!("{} {{msg}}", style("[{elapsed_precise}]").bold().dim())));
            };

            for compiled_func in compiled_funcs.into_iter() {
                let CompiledFunction {
                    code_buf,
                    reloc_sink,
                    ..
                } = compiled_func;

                // let func_offset = code_buf;
                protect_codebuf(&code_buf).unwrap();
                functions.push(code_buf);

                // context_and_offsets.push(func_context);
                relocations.push(reloc_sink.func_relocs);
            }

            // compiled_funcs?;

            debug!("Instance - Relocating functions");
            // For each of the functions used, we see what are the calls inside this functions
            // and relocate each call to the proper memory address.
            // The relocations are relative to the relocation's address plus four bytes
            // TODO: Support architectures other than x64, and other reloc kinds.
            for (i, function_relocs) in relocations.iter().enumerate() {
                for ref reloc in function_relocs {
                    let target_func_address: isize = match reloc.target {
                        RelocationType::Normal(func_index) => {
                            get_function_addr(&FuncIndex::new(func_index as usize), &import_functions, &functions) as isize
                        },
                        RelocationType::CurrentMemory => {
                            current_memory as isize
                        },
                        RelocationType::GrowMemory => {
                            grow_memory as isize
                        },
                        RelocationType::LibCall(LibCall::CeilF32) => {
                            math_intrinsics::ceilf32 as isize
                        },
                        RelocationType::LibCall(LibCall::FloorF32) => {
                            math_intrinsics::floorf32 as isize
                        },
                        RelocationType::LibCall(LibCall::TruncF32) => {
                            math_intrinsics::truncf32 as isize
                        },
                        RelocationType::LibCall(LibCall::NearestF32) => {
                            math_intrinsics::nearbyintf32 as isize
                        },
                        RelocationType::LibCall(LibCall::CeilF64) => {
                            math_intrinsics::ceilf64 as isize
                        },
                        RelocationType::LibCall(LibCall::FloorF64) => {
                            math_intrinsics::floorf64 as isize
                        },
                        RelocationType::LibCall(LibCall::TruncF64) => {
                            math_intrinsics::truncf64 as isize
                        },
                        RelocationType::LibCall(LibCall::NearestF64) => {
                            math_intrinsics::nearbyintf64 as isize
                        },
                        _ => unimplemented!()
                        // RelocationType::Intrinsic(name) => {
                        //     get_abi_intrinsic(name)?
                        // },
                    };

                    let func_addr =
                        get_function_addr(&FuncIndex::new(i), &import_functions, &functions);
                    match reloc.reloc {
                        Reloc::Abs8 => unsafe {
                            let reloc_address = func_addr.offset(reloc.offset as isize) as i64;
                            let reloc_addend = reloc.addend;
                            let reloc_abs = target_func_address as i64 + reloc_addend;
                            write_unaligned(reloc_address as *mut i64, reloc_abs);
                        },
                        Reloc::X86PCRel4 => unsafe {
                            let reloc_address = func_addr.offset(reloc.offset as isize) as isize;
                            let reloc_addend = reloc.addend as isize;
                            // TODO: Handle overflow.
                            let reloc_delta_i32 =
                                (target_func_address - reloc_address + reloc_addend) as i32;
                            write_unaligned(reloc_address as *mut i32, reloc_delta_i32);
                        },
                        _ => panic!("unsupported reloc kind"),
                    }
                }
            }
        }

        debug!("Instance - Instantiating globals");
        // Instantiate Globals
        let globals_data = {
            let globals_count = module.info.globals.len();
            // Allocate the underlying memory and initialize it to zeros
            let globals_data_size = globals_count * 8;
            globals.resize(globals_data_size, 0);

            // cast the globals slice to a slice of i64.
            let globals_data = unsafe {
                slice::from_raw_parts_mut(globals.as_mut_ptr() as *mut i64, globals_count)
            };

            for (i, global) in module.info.globals.iter().enumerate() {
                let ImportableExportable {
                    entity,
                    import_name,
                    ..
                } = global;
                let value: i64 = match entity.initializer {
                    GlobalInit::I32Const(n) => n as _,
                    GlobalInit::I64Const(n) => n,
                    GlobalInit::F32Const(f) => f as _, // unsafe { mem::transmute(f as f64) },
                    GlobalInit::F64Const(f) => f as _, // unsafe { mem::transmute(f) },
                    GlobalInit::GlobalRef(global_index) => globals_data[global_index.index()],
                    GlobalInit::Import() => {
                        let (module_name, field_name) = import_name
                            .as_ref()
                            .expect("Expected a import name for the global import");
                        let imported =
                            import_object.get(&module_name.as_str(), &field_name.as_str());
                        match imported {
                            Some(ImportValue::Global(value)) => *value,
                            None => {
                                if options.mock_missing_globals {
                                    0
                                } else {
                                    panic!(
                                        "Imported global value was not provided ({}.{})",
                                        module_name, field_name
                                    )
                                }
                            }
                            _ => panic!(
                                "Expected global import, but received {:?} ({}.{})",
                                imported, module_name, field_name
                            ),
                        }
                    }
                };
                globals_data[i] = value;
            }
            globals_data
        };

        debug!("Instance - Instantiating tables");
        // Instantiate tables
        {
            // Reserve space for tables
            tables.reserve_exact(module.info.tables.len());

            // Get tables in module
            for table in &module.info.tables {
                let table: Vec<usize> = match table.import_name.as_ref() {
                    Some((module_name, field_name)) => {
                        let imported =
                            import_object.get(&module_name.as_str(), &field_name.as_str());
                        match imported {
                            Some(ImportValue::Table(t)) => t.to_vec(),
                            None => {
                                if options.mock_missing_tables {
                                    let len = table.entity.size;
                                    let mut v = Vec::with_capacity(len);
                                    v.resize(len, 0);
                                    v
                                } else {
                                    panic!(
                                        "Imported table value was not provided ({}.{})",
                                        module_name, field_name
                                    )
                                }
                            }
                            _ => panic!(
                                "Expected global table, but received {:?} ({}.{})",
                                imported, module_name, field_name
                            ),
                        }
                    }
                    None => {
                        let len = table.entity.size;
                        let mut v = Vec::with_capacity(len);
                        v.resize(len, 0);
                        v
                    }
                };
                tables.push(table);
            }

            // instantiate tables
            for table_element in &module.info.table_elements {
                let base = match table_element.base {
                    Some(global_index) => globals_data[global_index.index()] as usize,
                    None => 0,
                };

                let table = &mut tables[table_element.table_index.index()];
                for (i, func_index) in table_element.elements.iter().enumerate() {
                    // since the table just contains functions in the MVP
                    // we get the address of the specified function indexes
                    // to populate the table.

                    // let func_index = *elem_index - module.info.imported_funcs.len() as u32;
                    // let func_addr = functions[func_index.index()].as_ptr();
                    let func_addr = get_function_addr(&func_index, &import_functions, &functions);
                    table[base + table_element.offset + i] = func_addr as _;
                }
            }
        }

        debug!("Instance - Instantiating memories");
        // Instantiate memories
        {
            // Reserve space for memories
            memories.reserve_exact(module.info.memories.len());

            // Get memories in module
            for memory in &module.info.memories {
                let memory = memory.entity;
                // If we use emscripten, we set a fixed initial and maximum
                debug!("Instance - init memory ({}, {:?})", memory.pages_count, memory.maximum);
                let memory = if options.use_emscripten {
                    // We use MAX_PAGES, so at the end the result is:
                    // (initial * LinearMemory::PAGE_SIZE) == LinearMemory::DEFAULT_HEAP_SIZE
                    // However, it should be: (initial * LinearMemory::PAGE_SIZE) == 16777216
                    LinearMemory::new(LinearMemory::MAX_PAGES as u32, None)
                }
                else {
                    LinearMemory::new(memory.pages_count as u32, memory.maximum.map(|m| m as u32))
                };
                memories.push(memory);
            }

            for init in &module.info.data_initializers {
                debug_assert!(init.base.is_none(), "globalvar base not supported yet");
                let offset = init.offset;
                let mem = &mut memories[init.memory_index.index()];
                let end_of_init = offset + init.data.len();
                if end_of_init > mem.current_size() {
                    let grow_pages = (end_of_init / LinearMemory::WASM_PAGE_SIZE) + 1;
                    mem.grow(grow_pages as u32)
                        .expect("failed to grow memory for data initializers");
                }
                let to_init = &mut mem[offset..offset + init.data.len()];
                to_init.copy_from_slice(&init.data);
            }
            if options.use_emscripten {
                debug!("emscripten::setup memory");
                crate::apis::emscripten::emscripten_set_up_memory(&mut memories[0]);
                debug!("emscripten::finish setup memory");
            }
        }

        let start_func: Option<FuncIndex> =
            module
                .info
                .start_func
                .or_else(|| match module.info.exports.get("main") {
                    Some(Export::Function(index)) => Some(*index),
                    _ => None,
                });

        let tables_pointer: Vec<BoundedSlice<usize>> =
            tables.iter().map(|table| table[..].into()).collect();
        let memories_pointer: Vec<BoundedSlice<u8>> = memories
            .iter()
            .map(|mem| BoundedSlice::new(&mem[..], mem.current_size()))
            .collect();
        let globals_pointer: GlobalsSlice = globals[..].into();

        let data_pointers = DataPointers {
            memories: memories_pointer[..].into(),
            globals: globals_pointer,
            tables: tables_pointer[..].into(),
        };

        let emscripten_data = if options.use_emscripten {
            unsafe {
                debug!("emscripten::initiating data");
                let malloc_export = module.info.exports.get("_malloc");
                let free_export = module.info.exports.get("_free");
                let memalign_export = module.info.exports.get("_memalign");
                let memset_export = module.info.exports.get("_memset");
                let stack_alloc_export = module.info.exports.get("stackAlloc");

                let mut malloc_addr = 0 as *const u8;
                let mut free_addr = 0 as *const u8;
                let mut memalign_addr = 0 as *const u8;
                let mut memset_addr = 0 as *const u8;
                let mut stack_alloc_addr = 0 as _;

                if malloc_export.is_none() && free_export.is_none() && memalign_export.is_none() && memset_export.is_none() {
                    None
                } else {
                    if let Some(Export::Function(malloc_index)) = malloc_export {
                        malloc_addr = get_function_addr(&malloc_index, &import_functions, &functions);
                    }

                    if let Some(Export::Function(free_index)) = free_export {
                        free_addr = get_function_addr(&free_index, &import_functions, &functions);
                    }

                    if let Some(Export::Function(memalign_index)) = memalign_export {
                        memalign_addr = get_function_addr(&memalign_index, &import_functions, &functions);
                    }

                    if let Some(Export::Function(memset_index)) = memset_export {
                        memset_addr = get_function_addr(&memset_index, &import_functions, &functions);
                    }

                    if let Some(Export::Function(stack_alloc_index)) = stack_alloc_export {
                        stack_alloc_addr = get_function_addr(&stack_alloc_index, &import_functions, &functions);
                    }

                    Some(EmscriptenData {
                        malloc: mem::transmute(malloc_addr),
                        free: mem::transmute(free_addr),
                        memalign: mem::transmute(memalign_addr),
                        memset: mem::transmute(memset_addr),
                        stack_alloc: mem::transmute(stack_alloc_addr),
                    })
                }
            }
        } else {
            None
        };

        Ok(Instance {
            data_pointers,
            tables: Arc::new(tables.into_iter().collect()), // tables.into_iter().map(|table| RwLock::new(table)).collect()),
            memories: Arc::new(memories.into_iter().collect()),
            globals,
            functions,
            import_functions,
            start_func,
            emscripten_data,
        })
    }

    pub fn memory_mut(&mut self, memory_index: usize) -> &mut LinearMemory {
        let memories = Arc::get_mut(&mut self.memories).unwrap_or_else(|| {
            panic!("Can't get memories as a mutable pointer (there might exist more mutable pointers to the memories)")
        });
        memories
            .get_mut(memory_index)
            .unwrap_or_else(|| panic!("no memory for index {}", memory_index))
    }

    pub fn memories(&self) -> Arc<Vec<LinearMemory>> {
        self.memories.clone()
    }

    pub fn get_function_pointer(&self, func_index: FuncIndex) -> *const u8 {
        get_function_addr(&func_index, &self.import_functions, &self.functions)
    }

    pub fn start(&self) -> Result<(), ErrorKind> {
        if let Some(func_index) = self.start_func {
            let func: fn(&Instance) = get_instance_function!(&self, func_index);
            call_protected!(func(self))
        } else {
            Ok(())
        }
    }

    /// Returns a slice of the contents of allocated linear memory.
    pub fn inspect_memory(&self, memory_index: usize, address: usize, len: usize) -> &[u8] {
        &self
            .memories
            .get(memory_index)
            .unwrap_or_else(|| panic!("no memory for index {}", memory_index))
            .as_ref()[address..address + len]
    }

    pub fn memory_offset_addr(&self, index: usize, offset: usize) -> *const usize {
        let memories: &[LinearMemory] = &self.memories[..];
        let mem = &memories[index];
        unsafe { mem[..].as_ptr().add(offset) as *const usize }
    }

    // Shows the value of a global variable.
    // pub fn inspect_global(&self, global_index: GlobalIndex, ty: ir::Type) -> &[u8] {
    //     let offset = global_index * 8;
    //     let len = ty.bytes() as usize;
    //     &self.globals[offset..offset + len]
    // }

    // pub fn start_func(&self) -> extern fn(&VmCtx) {
    //     self.start_func
    // }
}

// TODO: Needs to be moved to more appropriate place
extern "C" fn grow_memory(size: u32, memory_index: u32, instance: &mut Instance) -> i32 {
    // TODO: Support for only one LinearMemory for now.
    debug_assert_eq!(
        memory_index, 0,
        "non-default memory_index (0) not supported yet"
    );

    let old_mem_size = instance
        .memory_mut(memory_index as usize)
        .grow(size)
        .unwrap_or(-1);

    old_mem_size
}

extern "C" fn current_memory(memory_index: u32, instance: &mut Instance) -> u32 {
    let memory = &instance.memories[memory_index as usize];
    memory.current_pages() as u32
}
