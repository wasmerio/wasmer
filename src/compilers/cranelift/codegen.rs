use crate::runtime::{
    backend::FuncResolver,
    memory::LinearMemory,
    module::{DataInitializer, Export, ImportName, Module as WasmerModule, TableInitializer},
    types::{
        ElementType as WasmerElementType, FuncIndex as WasmerFuncIndex, FuncSig as WasmerSignature,
        Global as WasmerGlobal, GlobalDesc as WasmerGlobalDesc, GlobalIndex as WasmerGlobalIndex,
        Initializer as WasmerInitializer, Map, MapIndex, Memory as WasmerMemory,
        MemoryIndex as WasmerMemoryIndex, SigIndex as WasmerSignatureIndex, Table as WasmerTable,
        TableIndex as WasmerTableIndex, Type as WasmerType,
    },
    vm::{self, Ctx as WasmerVMContext},
    SigRegistry,
};
use crate::webassembly::errors::ErrorKind;
use cranelift_codegen::cursor::FuncCursor;
use cranelift_codegen::ir::immediates::{Offset32, Uimm64};
use cranelift_codegen::ir::types::{self, *};
use cranelift_codegen::ir::{
    self, AbiParam, ArgumentPurpose, ExtFuncData, ExternalName, FuncRef, InstBuilder, Signature,
    TrapCode,
};
use cranelift_codegen::isa::TargetFrontendConfig;
use cranelift_entity::{EntityRef, PrimaryMap};
use cranelift_wasm::{
    translate_module, DefinedFuncIndex, FuncEnvironment as FuncEnvironmentTrait, FuncIndex,
    FuncTranslator, Global, GlobalIndex, GlobalVariable, Memory, MemoryIndex, ModuleEnvironment,
    ReturnMode, SignatureIndex, Table, TableIndex, WasmResult,
};
use hashbrown::HashMap;
use std::ptr::NonNull;
use target_lexicon;

/// The converter namespace contains functions for converting a Cranelift module
/// to a Wasmer module.
pub mod converter {
    use super::*;

    /// Converts a Cranelift module to a Wasmer module.
    pub fn convert_module(cranelift_module: CraneliftModule) -> WasmerModule {
        // Convert Cranelift globals to Wasmer globals
        let mut globals: Map<WasmerGlobalIndex, WasmerGlobal> =
            Map::with_capacity(cranelift_module.globals.len());
        for global in cranelift_module.globals {
            globals.push(convert_global(global));
        }

        // Convert Cranelift memories to Wasmer memories.
        let mut memories: Map<WasmerMemoryIndex, WasmerMemory> =
            Map::with_capacity(cranelift_module.memories.len());
        for memory in cranelift_module.memories {
            memories.push(convert_memory(memory));
        }

        // Convert Cranelift tables to Wasmer tables.
        let mut tables: Map<WasmerTableIndex, WasmerTable> =
            Map::with_capacity(cranelift_module.tables.len());
        for table in cranelift_module.tables {
            tables.push(convert_table(table));
        }

        let mut sig_registry = SigRegistry::new();
        for signature in cranelift_module.signatures {
            let func_sig = convert_signature(signature);
            sig_registry.register(func_sig);
        }

        // Convert Cranelift signature indices to Wasmer signature indices.
        let mut func_assoc: Map<WasmerFuncIndex, WasmerSignatureIndex> =
            Map::with_capacity(cranelift_module.functions.len());
        for (_, signature_index) in cranelift_module.functions.iter() {
            func_assoc.push(WasmerSignatureIndex::new(signature_index.index()));
        }

        // Compile functions.
        // TODO: Rearrange, Abstract
        use crate::runtime::vmcalls::{memory_grow_static, memory_size};
        use crate::webassembly::{
            get_isa, libcalls,
            relocation::{Reloc, RelocSink, Relocation, RelocationType},
        };
        use cranelift_codegen::{binemit::NullTrapSink, ir::LibCall, Context};
        use std::ptr::write_unaligned;

        // Get the machine ISA.
        let isa = &*get_isa();
        let functions_length = cranelift_module.function_bodies.len();

        // Compiles internally defined functions only
        let mut compiled_functions: Vec<Vec<u8>> = Vec::with_capacity(functions_length);
        let mut relocations: Vec<Vec<Relocation>> = Vec::with_capacity(functions_length);

        for function_body in cranelift_module.function_bodies.iter() {
            let mut func_context = Context::for_function(function_body.1.to_owned());
            let mut code_buf: Vec<u8> = Vec::new();
            let mut reloc_sink = RelocSink::new();
            let mut trap_sink = NullTrapSink {};

            // Compile IR to machine code.
            let result = func_context.compile_and_emit(isa, &mut code_buf, &mut reloc_sink, &mut trap_sink);
            if result.is_err() {
                panic!("CompileError: {}", result.unwrap_err().to_string());
            }

            unsafe {
                // Make code buffer executable.
                let result = region::protect(
                    code_buf.as_ptr(),
                    code_buf.len(),
                    region::Protection::ReadWriteExecute,
                );

                if result.is_err() {
                    panic!(
                        "failed to give executable permission to code: {}",
                        result.unwrap_err().to_string()
                    );
                }
            }

            // Push compiled functions and relocations
            compiled_functions.push(code_buf);
            relocations.push(reloc_sink.func_relocs);
        }

        // Apply relocations.
        for (index, relocs) in relocations.iter().enumerate() {
            for ref reloc in relocs {
                let target_func_address: isize = match reloc.target {
                    RelocationType::Normal(func_index) => {
                        compiled_functions[func_index as usize].as_ptr() as isize
                    }
                    RelocationType::CurrentMemory => memory_size as isize,
                    RelocationType::GrowMemory => memory_grow_static as isize,
                    RelocationType::LibCall(libcall) => match libcall {
                        LibCall::CeilF32 => libcalls::ceilf32 as isize,
                        LibCall::FloorF32 => libcalls::floorf32 as isize,
                        LibCall::TruncF32 => libcalls::truncf32 as isize,
                        LibCall::NearestF32 => libcalls::nearbyintf32 as isize,
                        LibCall::CeilF64 => libcalls::ceilf64 as isize,
                        LibCall::FloorF64 => libcalls::floorf64 as isize,
                        LibCall::TruncF64 => libcalls::truncf64 as isize,
                        LibCall::NearestF64 => libcalls::nearbyintf64 as isize,
                        LibCall::Probestack => libcalls::__rust_probestack as isize,
                        _ => {
                            panic!("unexpected libcall {}", libcall);
                        }
                    },
                    RelocationType::Intrinsic(ref name) => {
                        panic!("unexpected intrinsic {}", name);
                    }
                };

                let func_addr = compiled_functions[index].as_ptr();

                // Determine relocation type and apply relocations.
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

        // Get other fields from the cranelift_module.
        let CraneliftModule {
            imported_functions,
            imported_memories,
            imported_tables,
            imported_globals,
            exports,
            data_initializers,
            table_initializers,
            start_func,
            ..
        } = cranelift_module;

        // Create Wasmer module from data above
        WasmerModule {
            func_resolver: Box::new(CraneliftFunctionResolver::new(compiled_functions)),
            memories,
            globals,
            tables,
            imported_functions,
            imported_memories,
            imported_tables,
            imported_globals,
            exports,
            data_initializers,
            table_initializers,
            start_func,
            func_assoc,
            sig_registry,
        }
    }

    /// Converts from Cranelift type to a Wasmer type.
    pub fn convert_type(ty: types::Type) -> WasmerType {
        match ty {
            I32 => WasmerType::I32,
            I64 => WasmerType::I64,
            F32 => WasmerType::F32,
            F64 => WasmerType::F64,
            _ => unimplemented!("unsupported wasm type!"),
        }
    }

    /// Converts a Cranelift global to a Wasmer global.
    pub fn convert_global(global: Global) -> WasmerGlobal {
        let desc = WasmerGlobalDesc {
            mutable: global.mutability,
            ty: convert_type(global.ty),
        };

        use self::WasmerInitializer::*;
        use cranelift_wasm::GlobalInit::{self, *};

        // TODO: WasmerGlobal does not support `Import` as Global values.
        let init = match global.initializer {
            I32Const(val) => Const(val.into()),
            I64Const(val) => Const(val.into()),
            F32Const(val) => Const((val as f32).into()),
            F64Const(val) => Const((val as f64).into()),
            GlobalInit::GetGlobal(index) => {
                WasmerInitializer::GetGlobal(WasmerGlobalIndex::new(index.index()))
            }
            Import => unimplemented!("TODO: imported globals are not supported yet!"),
        };

        WasmerGlobal { desc, init }
    }

    /// Converts a Cranelift table to a Wasmer table.
    pub fn convert_table(table: Table) -> WasmerTable {
        use cranelift_wasm::TableElementType::*;

        let ty = match table.ty {
            Func => WasmerElementType::Anyfunc,
            Val(_) => unimplemented!("non-function table elements are not supported yet!"),
        };

        WasmerTable {
            ty,
            min: table.minimum,
            max: table.maximum,
        }
    }

    /// Converts a Cranelift table to a Wasmer table.
    pub fn convert_memory(memory: Memory) -> WasmerMemory {
        WasmerMemory {
            shared: memory.shared,
            min: memory.minimum,
            max: memory.maximum,
        }
    }

    /// Converts a Cranelift signature to a Wasmer signature.
    pub fn convert_signature(sig: ir::Signature) -> WasmerSignature {
        WasmerSignature {
            params: sig
                .params
                .iter()
                .map(|param| convert_type(param.value_type))
                .collect(),
            returns: sig
                .returns
                .iter()
                .map(|ret| convert_type(ret.value_type))
                .collect(),
        }
    }
}

// Cranelift module for generating cranelift IR and the generic module
pub struct CraneliftModule {
    /// Target description relevant to frontends producing Cranelift IR.
    pub config: TargetFrontendConfig,

    /// Signatures as provided by `declare_signature`.
    pub signatures: Vec<ir::Signature>,

    /// Functions, imported and local.
    pub functions: PrimaryMap<FuncIndex, SignatureIndex>,

    /// Function bodies.
    pub function_bodies: PrimaryMap<DefinedFuncIndex, ir::Function>,

    /// The base of tables.
    pub tables_base: Option<ir::GlobalValue>,

    /// The Cranelift global holding the base address of the memories vector.
    pub memories_base: Option<ir::GlobalValue>,

    /// The Cranelift global holding the base address of the globals vector.
    pub globals_base: Option<ir::GlobalValue>,

    /// The external function declaration for implementing wasm's `current_memory`.
    pub current_memory_extfunc: Option<FuncRef>,

    /// The external function declaration for implementing wasm's `grow_memory`.
    pub grow_memory_extfunc: Option<FuncRef>,

    /// A function that takes a Wasmer module and resolves a function index to a vm::Func.
    pub func_resolver: Option<Box<dyn FuncResolver>>,

    // An array holding information about the wasm instance memories.
    pub memories: Vec<Memory>,

    // An array holding information about the wasm instance globals.
    pub globals: Vec<Global>,

    // An array holding information about the wasm instance tables.
    pub tables: Vec<Table>,

    // An array holding information about the wasm instance imported functions.
    pub imported_functions: Map<WasmerFuncIndex, ImportName>,

    // An array holding information about the wasm instance imported memories.
    pub imported_memories: Map<WasmerMemoryIndex, (ImportName, WasmerMemory)>,

    // An array holding information about the wasm instance imported tables.
    pub imported_tables: Map<WasmerTableIndex, (ImportName, WasmerTable)>,

    // An array holding information about the wasm instance imported globals.
    pub imported_globals: Map<WasmerGlobalIndex, (ImportName, WasmerGlobalDesc)>,

    // An hash map holding information about the wasm instance exports.
    pub exports: HashMap<String, Export>,

    // Data to initialize in memory.
    pub data_initializers: Vec<DataInitializer>,

    // Function indices to add to table.
    pub table_initializers: Vec<TableInitializer>,

    // The start function index.
    pub start_func: Option<WasmerFuncIndex>,
}

impl CraneliftModule {
    /// Translates wasm bytes into a Cranelift module
    pub fn from_bytes(
        buffer_source: &Vec<u8>,
        config: TargetFrontendConfig,
    ) -> Result<Self, ErrorKind> {
        // Create a cranelift module
        let mut cranelift_module = CraneliftModule {
            config,
            signatures: Vec::new(),
            functions: PrimaryMap::new(),
            function_bodies: PrimaryMap::new(),
            globals_base: None,
            tables_base: None,
            memories_base: None,
            current_memory_extfunc: None,
            grow_memory_extfunc: None,
            func_resolver: None,
            memories: Vec::new(),
            globals: Vec::new(),
            tables: Vec::new(),
            imported_functions: Map::new(),
            imported_memories: Map::new(),
            imported_tables: Map::new(),
            imported_globals: Map::new(),
            exports: HashMap::new(),
            data_initializers: Vec::new(),
            table_initializers: Vec::new(),
            start_func: None,
        };

        // Translate wasm to cranelift IR.
        translate_module(&buffer_source, &mut cranelift_module)
            .map_err(|e| ErrorKind::CompileError(e.to_string()))?;

        // Return translated module.
        Ok(cranelift_module)
    }
}

// Resolves a function index to a function address.
pub struct CraneliftFunctionResolver {
    compiled_functions: Vec<Vec<u8>>,
}

impl CraneliftFunctionResolver {
    fn new(compiled_functions: Vec<Vec<u8>>) -> Self {
        Self {
            compiled_functions,
        }
    }
}

// Implements FuncResolver trait.
impl FuncResolver for CraneliftFunctionResolver {
    // NOTE: This gets internal defined functions only. Will need access to vmctx to return imported function address.
    fn get(&self, module: &WasmerModule, index: WasmerFuncIndex) -> Option<NonNull<vm::Func>> {
        let index = index.index();
        let imported_functions_length = module.imported_functions.len();
        let internal_functions_length = self.compiled_functions.len();
        let limit = imported_functions_length + internal_functions_length;

        // Making sure it is not an imported function.
        if index >= imported_functions_length && index < limit {
            Some(NonNull::new(self.compiled_functions[index].as_ptr() as *mut _).unwrap())
        } else {
            None
        }
    }
}

/// The `FuncEnvironment` implementation for use by the `CraneliftModule`.
pub struct FuncEnvironment<'environment> {
    pub module: &'environment CraneliftModule,
}

impl<'environment> FuncEnvironment<'environment> {
    pub fn new(module: &'environment CraneliftModule) -> Self {
        Self { module }
    }

    /// Creates a signature with VMContext as the last param
    pub fn generate_signature(&self, sigidx: SignatureIndex) -> ir::Signature {
        // Get signature
        let mut signature = self.module.signatures[sigidx.index()].clone();

        // Add the vmctx parameter type to it
        signature.params.push(ir::AbiParam::special(
            self.pointer_type(),
            ir::ArgumentPurpose::VMContext,
        ));

        // Return signature
        signature
    }
}

///
impl<'environment> FuncEnvironmentTrait for FuncEnvironment<'environment> {
    /// Gets configuration information needed for compiling functions
    fn target_config(&self) -> TargetFrontendConfig {
        self.module.config
    }

    /// Gets native pointers types.
    ///
    /// `I64` on 64-bit arch; `I32` on 32-bit arch.
    fn pointer_type(&self) -> ir::Type {
        ir::Type::int(u16::from(self.module.config.pointer_bits())).unwrap()
    }

    /// Gets the size of a native pointer in bytes.
    fn pointer_bytes(&self) -> u8 {
        self.module.config.pointer_bytes()
    }

    /// Sets up the necessary preamble definitions in `func` to access the global identified
    /// by `index`.
    ///
    /// The index space covers both imported and locally declared globals.
    fn make_global(&mut self, func: &mut ir::Function, index: GlobalIndex) -> GlobalVariable {
        // Create VMContext value.
        let vmctx = func.create_global_value(ir::GlobalValueData::VMContext);
        let ptr_size = self.pointer_bytes();
        let globals_offset = WasmerVMContext::offset_globals();

        // Load value at (vmctx + globals_offset), i.e. the address at Ctx.globals.
        let globals_base_addr = func.create_global_value(ir::GlobalValueData::Load {
            base: vmctx,
            offset: Offset32::new(globals_offset as i32),
            global_type: self.pointer_type(),
            readonly: false,
        });

        // *Ctx.globals -> [ u8, u8, .. ]
        // Based on the index provided, we need to know the offset into globals array
        let offset = index.index() * ptr_size as usize;

        // Create global variable based on the data above.
        GlobalVariable::Memory {
            gv: globals_base_addr,
            offset: (offset as i32).into(),
            ty: self.module.get_global(index).ty,
        }
    }

    /// Sets up the necessary preamble definitions in `func` to access the linear memory identified
    /// by `index`.
    ///
    /// The index space covers both imported and locally declared memories.
    fn make_heap(&mut self, func: &mut ir::Function, index: MemoryIndex) -> ir::Heap {
        // Only the first memory is supported for now.
        debug_assert_eq!(index.index(), 0, "non-default memories not supported yet");

        // Create VMContext value.
        let vmctx = func.create_global_value(ir::GlobalValueData::VMContext);
        let ptr_size = self.pointer_bytes();
        let memories_offset = WasmerVMContext::offset_memories();

        // Load value at (vmctx + memories_offset), i.e. the address at Ctx.memories.
        let base = func.create_global_value(ir::GlobalValueData::Load {
            base: vmctx,
            offset: Offset32::new(memories_offset as i32),
            global_type: self.pointer_type(),
            readonly: true,
        });

        // *Ctx.memories -> [ {data: *usize, len: usize}, {data: *usize, len: usize}, ... ]
        // Based on the index provided, we need to know the offset into memories array.
        let memory_data_offset = (index.as_u32() as i32) * (ptr_size as i32) * 2;

        // Load value at the (base + memory_data_offset), i.e. the address at Ctx.memories[index].data.
        let heap_base = func.create_global_value(ir::GlobalValueData::Load {
            base,
            offset: Offset32::new(memory_data_offset),
            global_type: self.pointer_type(),
            readonly: true,
        });

        // Create heap based on the data above.
        func.create_heap(ir::HeapData {
            base: heap_base,
            min_size: 0.into(),
            offset_guard_size: Uimm64::new(LinearMemory::DEFAULT_GUARD_SIZE as u64),
            style: ir::HeapStyle::Static {
                bound: Uimm64::new(LinearMemory::DEFAULT_HEAP_SIZE as u64),
            },
            index_type: I32,
        })
    }

    /// Sets up the necessary preamble definitions in `func` to access the table identified
    /// by `index`.
    ///
    /// The index space covers both imported and locally declared tables.
    fn make_table(&mut self, func: &mut ir::Function, index: TableIndex) -> ir::Table {
        // Only the first table is supported for now.
        debug_assert_eq!(index.index(), 0, "non-default tables not supported yet");

        // Create VMContext value.
        let vmctx = func.create_global_value(ir::GlobalValueData::VMContext);
        let ptr_size = self.pointer_bytes();
        let tables_offset = WasmerVMContext::offset_tables();

        // Load value at (vmctx + memories_offset) which is the address at Ctx.tables.
        let base = func.create_global_value(ir::GlobalValueData::Load {
            base: vmctx,
            offset: Offset32::new(tables_offset as i32),
            global_type: self.pointer_type(),
            readonly: true,
        });

        // *Ctx.tables -> [ {data: *usize, len: usize}, {data: *usize, len: usize}, ... ]
        // Based on the index provided, we need to know the offset into tables array.
        let table_data_offset = (index.as_u32() as i32) * (ptr_size as i32) * 2;

        // Load value at (base + table_data_offset), i.e. the address at Ctx.tables[index].data
        let base_gv = func.create_global_value(ir::GlobalValueData::Load {
            base,
            offset: Offset32::new(table_data_offset),
            global_type: self.pointer_type(),
            readonly: false,
        });

        // Load value at (base + table_data_offset), i.e. the value at Ctx.tables[index].len
        let bound_gv = func.create_global_value(ir::GlobalValueData::Load {
            base,
            offset: Offset32::new(table_data_offset + ptr_size as i32),
            global_type: self.pointer_type(),
            readonly: false,
        });

        // Create table based on the data above
        func.create_table(ir::TableData {
            base_gv,
            min_size: Uimm64::new(0),
            bound_gv,
            element_size: Uimm64::new(u64::from(self.pointer_bytes())),
            index_type: I64,
        })
    }

    /// Sets up a signature definition in `func`'s preamble.
    ///
    /// Signature may contain additional argument, but arguments marked as ArgumentPurpose::Normal`
    /// must correspond to the arguments in the wasm signature
    fn make_indirect_sig(&mut self, func: &mut ir::Function, index: SignatureIndex) -> ir::SigRef {
        // Create a signature reference out of specified signature (with VMContext param added).
        func.import_signature(self.generate_signature(index))
    }

    /// Sets up an external function definition in the preamble of `func` that can be used to
    /// directly call the function `index`.
    ///
    /// The index space covers both imported functions and functions defined in the current module.
    fn make_direct_func(&mut self, func: &mut ir::Function, index: FuncIndex) -> ir::FuncRef {
        // Get signature of function.
        let signature_index = self.module.get_func_type(index);

        // Create a signature reference from specified signature (with VMContext param added).
        let signature = func.import_signature(self.generate_signature(signature_index));

        // Get name of function.
        let name = ExternalName::user(0, index.as_u32());

        // Create function reference from fuction data.
        func.import_function(ir::ExtFuncData {
            name,
            signature,
            colocated: false,
        })
    }

    /// Generates an indirect call IR with `callee` and `call_args`.
    ///
    /// Inserts instructions at `pos` to the function `callee` in the table
    /// `table_index` with WebAssembly signature `sig_index`
    /// TODO: Generate bounds checking code.
    #[cfg_attr(feature = "cargo-clippy", allow(clippy::too_many_arguments))]
    fn translate_call_indirect(
        &mut self,
        mut pos: FuncCursor,
        table_index: TableIndex,
        table: ir::Table,
        sig_index: SignatureIndex,
        sig_ref: ir::SigRef,
        callee: ir::Value,
        call_args: &[ir::Value],
    ) -> WasmResult<ir::Inst> {
        // Create a VMContext value.
        let vmctx = pos
            .func
            .special_param(ir::ArgumentPurpose::VMContext)
            .expect("missing vmctx parameter");

        // Get the pointer type based on machine's pointer size.
        let ptr_type = self.pointer_type();

        // The `callee` value is an index into a table of function pointers.
        // Set callee to an appropriate type based on machine's pointer size.
        let callee_offset = if ptr_type == I32 {
            callee
        } else {
            pos.ins().uextend(ptr_type, callee)
        };

        // The `callee` value is an index into a table of function pointers.
        let entry_addr = pos.ins().table_addr(ptr_type, table, callee_offset, 0);

        let mut mflags = ir::MemFlags::new();
        mflags.set_notrap();
        mflags.set_aligned();

        let func_ptr = pos.ins().load(ptr_type, mflags, entry_addr, 0);

        pos.ins().trapz(func_ptr, TrapCode::IndirectCallToNull);

        // Build a value list for the indirect call instruction containing the callee, call_args,
        // and the vmctx parameter.
        let mut args = ir::ValueList::default();
        args.push(func_ptr, &mut pos.func.dfg.value_lists);
        args.extend(call_args.iter().cloned(), &mut pos.func.dfg.value_lists);
        args.push(vmctx, &mut pos.func.dfg.value_lists);

        let inst = pos
            .ins()
            .CallIndirect(ir::Opcode::CallIndirect, INVALID, sig_ref, args)
            .0;

        Ok(inst)
    }

    /// Generates a call IR with `callee` and `call_args` and inserts it at `pos`
    /// TODO: add support for imported functions
    fn translate_call(
        &mut self,
        mut pos: FuncCursor,
        _callee_index: FuncIndex,
        callee: ir::FuncRef,
        call_args: &[ir::Value],
    ) -> WasmResult<ir::Inst> {
        // Insert call instructions for `callee`.
        Ok(pos.ins().call(callee, call_args))
    }

    /// Generates code corresponding to wasm `memory.grow`.
    ///
    /// `index` refers to the linear memory to query.
    ///
    /// `heap` refers to the IR generated by `make_heap`.
    ///
    /// `val`  refers the value to grow the memory by.
    fn translate_memory_grow(
        &mut self,
        mut pos: FuncCursor,
        index: MemoryIndex,
        heap: ir::Heap,
        val: ir::Value,
    ) -> WasmResult<ir::Value> {
        // Only the first memory is supported for now.
        let grow_mem_func = self.module.grow_memory_extfunc.unwrap_or_else(|| {
            // Create signature reference from specified signature.
            let signature_ref = pos.func.import_signature(Signature {
                // Get the default calling convention of the isa.
                call_conv: self.module.config.default_call_conv,
                // Paramters types.
                params: vec![
                    // Param for new size.
                    AbiParam::new(I32),
                    // Param for memory index.
                    AbiParam::new(I32),
                    // Param for VMcontext.
                    AbiParam::special(self.pointer_type(), ArgumentPurpose::VMContext),
                ],
                // Return type for previous memory size.
                returns: vec![AbiParam::new(I32)],
            });

            // Create function reference to a linked `grow_memory` function.
            pos.func.import_function(ExtFuncData {
                name: ExternalName::testcase("grow_memory"),
                signature: signature_ref,
                colocated: false,
            })
        });

        // Create a memory index value.
        let memory_index = pos.ins().iconst(I32, to_imm64(index.index()));

        // Create a VMContext value.
        let vmctx = pos
            .func
            .special_param(ArgumentPurpose::VMContext)
            .expect("missing vmctx parameter");

        // Insert call instructions for `grow_memory`.
        let call_inst = pos.ins().call(grow_mem_func, &[val, memory_index, vmctx]);

        // Return value.
        Ok(*pos.func.dfg.inst_results(call_inst).first().unwrap())
    }

    /// Generates code corresponding to wasm `memory.size`.
    ///
    /// `index` refers to the linear memory to query.
    ///
    /// `heap` refers to the IR generated by `make_heap`.
    fn translate_memory_size(
        &mut self,
        mut pos: FuncCursor,
        index: MemoryIndex,
        heap: ir::Heap,
    ) -> WasmResult<ir::Value> {
        debug_assert_eq!(index.index(), 0, "non-default memories not supported yet");
        // Only the first memory is supported for now.
        let cur_mem_func = self.module.current_memory_extfunc.unwrap_or_else(|| {
            // Create signature reference from specified signature.
            let signature_ref = pos.func.import_signature(Signature {
                // Get the default calling convention of the isa.
                call_conv: self.module.config.default_call_conv,
                // Paramters types.
                params: vec![
                    // Param for memory index.
                    AbiParam::new(I32),
                    // Param for VMcontext.
                    AbiParam::special(self.pointer_type(), ArgumentPurpose::VMContext),
                ],
                // Return type for current memory size.
                returns: vec![AbiParam::new(I32)],
            });

            // Create function reference to a linked `current_memory` function.
            pos.func.import_function(ExtFuncData {
                name: ExternalName::testcase("current_memory"),
                signature: signature_ref,
                colocated: false,
            })
        });

        // Create a memory index value.
        let memory_index_value = pos.ins().iconst(I32, to_imm64(index.index()));

        // Create a VMContext value.
        let vmctx = pos.func.special_param(ArgumentPurpose::VMContext).unwrap();

        // Insert call instructions for `current_memory`.
        let call_inst = pos.ins().call(cur_mem_func, &[memory_index_value, vmctx]);

        // Return value.
        Ok(*pos.func.dfg.inst_results(call_inst).first().unwrap())
    }

    /// Generates code at the beginning of loops.
    ///
    /// Currently not used.
    fn translate_loop_header(&mut self, _pos: FuncCursor) {
        // By default, don't emit anything.
    }

    /// Determines the type of return each function should have.
    ///
    /// It is normal returns for now.
    fn return_mode(&self) -> ReturnMode {
        ReturnMode::NormalReturns
    }
}

/// Convert a usize offset into a `Imm64` for an iadd_imm.
fn to_imm64(offset: usize) -> ir::immediates::Imm64 {
    (offset as i64).into()
}

impl<'data> ModuleEnvironment<'data> for CraneliftModule {
    /// Get the information needed to produce Cranelift IR for the current target.
    fn target_config(&self) -> TargetFrontendConfig {
        self.config
    }

    /// Declares a function signature to the environment.
    fn declare_signature(&mut self, sig: &ir::Signature) {
        self.signatures.push(sig.clone());
    }

    /// Return the signature with the given index.
    fn get_signature(&self, sig_index: SignatureIndex) -> &ir::Signature {
        &self.signatures[sig_index.index()]
    }

    /// Declares a function import to the environment.
    fn declare_func_import(
        &mut self,
        sig_index: SignatureIndex,
        module: &'data str,
        field: &'data str,
    ) {
        // Imported functions are always declared first
        // Add signature index to list of functions
        self.functions.push(sig_index);

        // Add import names to list of imported functions
        self.imported_functions
            .push((String::from(module), String::from(field)).into());
    }

    /// Return the number of imported funcs.
    fn get_num_func_imports(&self) -> usize {
        self.imported_functions.len()
    }

    /// Declares the type (signature) of a local function in the module.
    fn declare_func_type(&mut self, sig_index: SignatureIndex) {
        self.functions.push(sig_index);
    }

    /// Return the signature index for the given function index.
    fn get_func_type(&self, func_index: FuncIndex) -> SignatureIndex {
        self.functions[func_index]
    }

    /// Declares a global to the environment.
    fn declare_global(&mut self, global: Global) {
        // Add global ir to the list of globals
        self.globals.push(global);
    }

    /// Declares a global import to the environment.
    fn declare_global_import(&mut self, global: Global, module: &'data str, field: &'data str) {
        // Add global index to list of globals
        self.globals.push(global);

        // Add import names to list of imported globals
        self.imported_globals.push((
            (String::from(module), String::from(field)).into(),
            converter::convert_global(global).desc,
        ));
    }

    /// Return the global for the given global index.
    fn get_global(&self, global_index: GlobalIndex) -> &Global {
        &self.globals[global_index.index()]
    }

    /// Declares a table to the environment.
    fn declare_table(&mut self, table: Table) {
        // Add table ir to the list of tables
        self.tables.push(table);
    }

    /// Declares a table import to the environment.
    fn declare_table_import(&mut self, table: Table, module: &'data str, field: &'data str) {
        // Add table index to list of tables
        self.tables.push(table);

        // Add import names to list of imported tables
        self.imported_tables.push((
            (String::from(module), String::from(field)).into(),
            converter::convert_table(table),
        ));
    }

    /// Fills a declared table with references to functions in the module.
    fn declare_table_elements(
        &mut self,
        table_index: TableIndex,
        base: Option<GlobalIndex>,
        offset: usize,
        elements: Vec<FuncIndex>,
    ) {
        // Convert Cranelift GlobalIndex to wamser GlobalIndex
        let base = base.map(|index| WasmerGlobalIndex::new(index.index()));

        // Add table initializer to list of table initializers
        self.table_initializers.push(TableInitializer {
            table_index: WasmerTableIndex::new(table_index.index()),
            base,
            offset,
            elements: elements
                .iter()
                .map(|index| WasmerFuncIndex::new(index.index()))
                .collect(),
        });
    }

    /// Declares a memory to the environment
    fn declare_memory(&mut self, memory: Memory) {
        // Add memory index to list of memories
        self.memories.push(memory);
    }

    /// Declares a memory import to the environment.
    fn declare_memory_import(&mut self, memory: Memory, module: &'data str, field: &'data str) {
        // Add memory index to list of memories
        self.memories.push(memory);

        // Add import names to list of imported memories
        self.imported_memories.push((
            (String::from(module), String::from(field)).into(),
            converter::convert_memory(memory),
        ));
    }

    /// Fills a declared memory with bytes at module instantiation.
    fn declare_data_initialization(
        &mut self,
        memory_index: MemoryIndex,
        base: Option<GlobalIndex>,
        offset: usize,
        data: &'data [u8],
    ) {
        // Convert Cranelift GlobalIndex to wamser GlobalIndex
        let base = base.map(|index| WasmerGlobalIndex::new(index.index()));

        // Add data initializer to list of data initializers
        self.data_initializers.push(DataInitializer {
            memory_index: WasmerMemoryIndex::new(memory_index.index()),
            base,
            offset,
            data: data.to_vec(),
        });
    }

    /// Declares a function export to the environment.
    fn declare_func_export(&mut self, func_index: FuncIndex, name: &'data str) {
        self.exports.insert(
            String::from(name),
            Export::Func(WasmerFuncIndex::new(func_index.index())),
        );
    }
    /// Declares a table export to the environment.
    fn declare_table_export(&mut self, table_index: TableIndex, name: &'data str) {
        self.exports.insert(
            String::from(name),
            Export::Table(WasmerTableIndex::new(table_index.index())),
        );
    }
    /// Declares a memory export to the environment.
    fn declare_memory_export(&mut self, memory_index: MemoryIndex, name: &'data str) {
        self.exports.insert(
            String::from(name),
            Export::Memory(WasmerMemoryIndex::new(memory_index.index())),
        );
    }
    /// Declares a global export to the environment.
    fn declare_global_export(&mut self, global_index: GlobalIndex, name: &'data str) {
        self.exports.insert(
            String::from(name),
            Export::Global(WasmerGlobalIndex::new(global_index.index())),
        );
    }

    /// Declares a start function.
    fn declare_start_func(&mut self, index: FuncIndex) {
        self.start_func = Some(WasmerFuncIndex::new(index.index()));
    }

    /// Provides the contents of a function body.
    fn define_function_body(&mut self, body_bytes: &'data [u8]) -> WasmResult<()> {
        // IR of the function body.
        let func_body = {
            // Generate a function environment needed by the function IR.
            let mut func_environ = FuncEnvironment::new(&self);

            // Get function index.
            let func_index =
                FuncIndex::new(self.get_num_func_imports() + self.function_bodies.len());

            // Get the name of function index.
            let name = ExternalName::user(0, func_index.as_u32());

            // Get signature of function and extend with a vmct parameter.
            let sig = func_environ.generate_signature(self.get_func_type(func_index));

            // Create function.
            let mut function = ir::Function::with_name_signature(name, sig);

            // Complete function creation with translated function body.
            FuncTranslator::new().translate(body_bytes, &mut function, &mut func_environ)?;

            function
        };

        // Add function body to list of function bodies.
        self.function_bodies.push(func_body);

        Ok(())
    }
}
