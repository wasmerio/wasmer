//! A webassembly::Instance object is a stateful, executable instance of a
//! webassembly::Module.  Instance objects contain all the Exported
//! WebAssembly functions that allow calling into WebAssembly code.

//! The webassembly::Instance() constructor function can be called to
//! synchronously instantiate a given webassembly::Module object. However, the
//! primary way to get an Instance is through the asynchronous
//! webassembly::instantiate_streaming() function.
use cranelift_codegen::ir::LibCall;
use cranelift_codegen::{binemit, Context};
use cranelift_entity::EntityRef;
use cranelift_wasm::{FuncIndex, GlobalInit};
use cranelift_codegen::isa::TargetIsa;
use region;
use std::iter::Iterator;
use std::ptr::write_unaligned;
use std::slice;
use std::sync::Arc;
use std::mem::size_of;

use super::super::common::slice::{BoundedSlice, UncheckedSlice};
use super::errors::ErrorKind;
use super::import_object::{ImportObject, ImportValue};
use super::memory::LinearMemory;
use super::module::Export;
use super::module::Module;
use super::relocation::{Reloc, RelocSink, RelocationType};
use super::math_intrinsics;

type TablesSlice = UncheckedSlice<BoundedSlice<usize>>;
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

/// An Instance of a WebAssembly module
/// NOTE: There is an assumption that data_pointers is always the
///      first field
#[repr(C)]
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
}

/// Contains pointers to data (heaps, globals, tables) needed
/// by Cranelift.
/// NOTE: Rearranging the fields will break the memory arrangement model
#[repr(C)]
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
    pub isa: Box<TargetIsa>,
}

extern fn mock_fn() -> i32 {
    return 0;
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

        let mut imported_memories: Vec<LinearMemory> = Vec::new();
        let mut imported_tables: Vec<Vec<usize>> = Vec::new();

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
                let imported = import_object
                    .get(&module.as_str(), &field.as_str());
                let function: &*const u8 = match imported {
                    Some(ImportValue::Func(f)) => f,
                    None => {
                        if options.mock_missing_imports {
                            debug!("The import {}.{} is not provided, therefore will be mocked.", module, field);
                            &(mock_fn as *const u8)
                        }
                        else {
                            return Err(ErrorKind::LinkError(format!(
                                "Imported function {}.{} was not provided in the import_functions",
                                module, field
                            )));
                        }
                    },
                    other => panic!("Expected function import, received {:?}", other)
                };
                // println!("GET FUNC {:?}", function);
                import_functions.push(*function);
                relocations.push(vec![]);
            }

            debug!("Instance - Compiling functions");
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
                //     .map_err(|e| ErrorKind::CompileError(e.to_string()))?;
                //     as usize;

                let mut code_buf: Vec<u8> = Vec::new();
                let mut reloc_sink = RelocSink::new();
                let mut trap_sink = binemit::NullTrapSink {};
                // This will compile a cranelift ir::Func into a code buffer (stored in memory)
                // and will push any inner function calls to the reloc sync.
                // In case traps need to be triggered, they will go to trap_sink
                func_context
                    .compile_and_emit(&*options.isa, &mut code_buf, &mut reloc_sink, &mut trap_sink)
                    .map_err(|e| {
                        debug!("CompileError: {}", e.to_string());
                        ErrorKind::CompileError(e.to_string())
                    })?;
                // We set this code_buf to be readable & executable
                protect_codebuf(&code_buf).unwrap();

                let func_offset = code_buf;
                functions.push(func_offset);

                // context_and_offsets.push(func_context);
                relocations.push(reloc_sink.func_relocs);
                // println!("FUNCTION RELOCATIONS {:?}", reloc_sink.func_relocs)
                // total_size += code_size_offset;
            }

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

        // Looping through and getting the imported objects
        for (_key, value) in import_object.map {
            match value {
                ImportValue::Memory(value) =>
                    imported_memories.push(value),
                ImportValue::Table(value) =>
                    imported_tables.push(value),
                _ => (),
            }
        }

        debug!("Instance - Instantiating tables");
        // Instantiate tables
        {
            // Reserve space for tables
            tables.reserve_exact(imported_tables.len() + module.info.tables.len());

            // Get imported tables
            for table in imported_tables {
                tables.push(table);
            }

            // Get tables in module
            for table in &module.info.tables {
                let len = table.entity.size;
                let mut v = Vec::with_capacity(len);
                v.resize(len, 0);
                tables.push(v);
            }

            // instantiate tables
            for table_element in &module.info.table_elements {
                // TODO: We shouldn't assert here since we are returning a Result<Instance, ErrorKind>
                assert!(
                    table_element.base.is_none(),
                    "globalvalue base not supported yet."
                );

                let base = 0;

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
            memories.reserve_exact(imported_memories.len() + module.info.memories.len());

            // Get imported memories
            for memory in imported_memories {
                memories.push(memory);
            }

            // Get memories in module
            for memory in &module.info.memories {
                let memory = memory.entity;
                let v = LinearMemory::new(
                    memory.pages_count as u32,
                    memory.maximum.map(|m| m as u32),
                );
                memories.push(v);
            }

            for init in &module.info.data_initializers {
                debug_assert!(init.base.is_none(), "globalvar base not supported yet");
                let offset = init.offset;
                let mem_mut = memories[init.memory_index.index()].as_mut();
                let to_init = &mut mem_mut[offset..offset + init.data.len()];
                to_init.copy_from_slice(&init.data);
            }
        }

        debug!("Instance - Instantiating globals");
        // TODO: Fix globals import
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
                    GlobalInit::F32Const(f) => f.to_bits() as _,
                    GlobalInit::F64Const(f) => f.to_bits(),
                    GlobalInit::GlobalRef(_global_index) => {
                        unimplemented!("GlobalInit::GlobalRef is not yet supported")
                    }
                    GlobalInit::Import() => {
                        // Right now (because there is no module/field fields on the Import
                        // https://github.com/CraneStation/cranelift/blob/5cabce9b58ff960534d4017fad11f2e78c72ceab/lib/wasm/src/sections_translator.rs#L90-L99 )
                        // It's impossible to know where to take the global from.
                        // This should be fixed in Cranelift itself.
                        unimplemented!("GlobalInit::Import is not yet supported")
                    }
                };
                globals_data[i] = value;
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

        // TODO: Refactor repetitive code
        let tables_pointer: Vec<BoundedSlice<usize>> =
            tables.iter().map(|table| table[..].into()).collect();
        let memories_pointer: Vec<BoundedSlice<u8>> =
            memories.iter().map(
                |mem| BoundedSlice::new(&mem[..], mem.current as usize * LinearMemory::WASM_PAGE_SIZE),
            ).collect();
        let globals_pointer: GlobalsSlice = globals[..].into();

        let data_pointers = DataPointers {
            memories: memories_pointer[..].into(),
            globals: globals_pointer,
            tables: tables_pointer[..].into(),
        };

        // let mem = data_pointers.memories;

        Ok(Instance {
            data_pointers,
            tables: Arc::new(tables.into_iter().collect()), // tables.into_iter().map(|table| RwLock::new(table)).collect()),
            memories: Arc::new(memories.into_iter().collect()),
            globals,
            functions,
            import_functions,
            start_func,
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

    pub fn start(&self) {
        if let Some(func_index) = self.start_func {
            let func: fn(&Instance) = get_instance_function!(&self, func_index);
            func(self)
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

    if old_mem_size != -1 {
        // Get new memory bytes
        let new_mem_bytes = (old_mem_size as usize + size  as usize) * LinearMemory::WASM_PAGE_SIZE;
        // Update data_pointer
        instance.data_pointers.memories.get_unchecked_mut(memory_index  as usize).len = new_mem_bytes;
    }

    old_mem_size
}

extern "C" fn current_memory(memory_index: u32, instance: &mut Instance) -> u32 {
    let memory = &instance.memories[memory_index as usize];
    memory.current_size() as u32
}
