use crate::resolver::FuncResolverBuilder;
use cranelift_codegen::cursor::FuncCursor;
use cranelift_codegen::ir::immediates::{Offset32, Uimm64};
use cranelift_codegen::ir::types::{self, *};
use cranelift_codegen::ir::{
    self, condcodes::IntCC, AbiParam, ArgumentPurpose, ExtFuncData, ExternalName, FuncRef,
    InstBuilder, Signature, TrapCode,
};
use cranelift_codegen::isa::TargetFrontendConfig;
use cranelift_entity::{EntityRef, PrimaryMap};
use cranelift_wasm::{
    translate_module, DefinedFuncIndex, FuncEnvironment as FuncEnvironmentTrait, FuncIndex,
    FuncTranslator, Global, GlobalIndex, GlobalVariable, Memory, MemoryIndex, ModuleEnvironment,
    ReturnMode, SignatureIndex, Table, TableIndex, WasmResult,
};
use hashbrown::HashMap;
use std::mem;
use target_lexicon;
use wasmer_runtime::{
    backend::SigRegistry,
    memory::LinearMemory,
    module::{
        DataInitializer, ExportIndex, ImportName, ModuleInner as WasmerModule, TableInitializer,
    },
    types::{
        ElementType as WasmerElementType, FuncIndex as WasmerFuncIndex, FuncSig as WasmerSignature,
        Global as WasmerGlobal, GlobalDesc as WasmerGlobalDesc, GlobalIndex as WasmerGlobalIndex,
        GlobalInit as WasmerGlobalInit, Initializer as WasmerInitializer,
        Memory as WasmerMemory, MemoryIndex as WasmerMemoryIndex, SigIndex as WasmerSignatureIndex,
        Table as WasmerTable, TableIndex as WasmerTableIndex, Type as WasmerType,
    },
    structures::{TypedIndex, Map},
    vm::{self, Ctx as WasmerVMContext},
};

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

        // Convert Cranelift signature indices to Wasmer signature indices.
        let mut func_assoc: Map<WasmerFuncIndex, WasmerSignatureIndex> =
            Map::with_capacity(cranelift_module.functions.len());
        for (_, signature_index) in cranelift_module.functions.iter() {
            func_assoc.push(
                cranelift_module.sig_registry.lookup_deduplicated_sigindex(
                    WasmerSignatureIndex::new(signature_index.index()),
                ),
            );
        }

        let function_bodies: Vec<_> = cranelift_module
            .function_bodies
            .into_iter()
            .map(|(_, v)| v.clone())
            .collect();
        let func_resolver_builder = FuncResolverBuilder::new(
            &*crate::get_isa(),
            function_bodies,
            cranelift_module.imported_functions.len(),
        )
        .unwrap();

        // Create func_resolver.
        let func_resolver = Box::new(func_resolver_builder.finalize().unwrap());

        // Get other fields from the cranelift_module.
        let CraneliftModule {
            imported_functions,
            imported_memories,
            imported_tables,
            imported_globals,
            exports,
            data_initializers,
            elem_initializers,
            start_func,
            sig_registry,
            ..
        } = cranelift_module;

        // Create Wasmer module from data above
        WasmerModule {
            func_resolver,
            memories,
            globals,
            tables,
            imported_functions,
            imported_memories,
            imported_tables,
            imported_globals,
            exports,
            data_initializers,
            elem_initializers,
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
            I32Const(val) => WasmerGlobalInit::Init(Const(val.into())),
            I64Const(val) => WasmerGlobalInit::Init(Const(val.into())),
            F32Const(val) => WasmerGlobalInit::Init(Const(f32::from_bits(val).into())),
            F64Const(val) => WasmerGlobalInit::Init(Const(f64::from_bits(val).into())),
            GlobalInit::GetGlobal(index) => WasmerGlobalInit::Init(WasmerInitializer::GetGlobal(
                WasmerGlobalIndex::new(index.index()),
            )),
            GlobalInit::Import => WasmerGlobalInit::Import,
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
        println!("codegen memory: {:?}", memory);
        WasmerMemory {
            shared: memory.shared,
            min: memory.minimum,
            max: memory.maximum,
        }
    }

    /// Converts a Cranelift signature to a Wasmer signature.
    pub fn convert_signature(sig: &ir::Signature) -> WasmerSignature {
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
    pub exports: HashMap<String, ExportIndex>,

    // Data to initialize in memory.
    pub data_initializers: Vec<DataInitializer>,

    // Function indices to add to table.
    pub elem_initializers: Vec<TableInitializer>,

    // The start function index.
    pub start_func: Option<WasmerFuncIndex>,

    pub sig_registry: SigRegistry,
}

impl CraneliftModule {
    /// Translates wasm bytes into a Cranelift module
    pub fn from_bytes(
        buffer_source: &Vec<u8>,
        config: TargetFrontendConfig,
    ) -> Result<Self, String> {
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
            memories: Vec::new(),
            globals: Vec::new(),
            tables: Vec::new(),
            imported_functions: Map::new(),
            imported_memories: Map::new(),
            imported_tables: Map::new(),
            imported_globals: Map::new(),
            exports: HashMap::new(),
            data_initializers: Vec::new(),
            elem_initializers: Vec::new(),
            start_func: None,
            sig_registry: SigRegistry::new(),
        };

        // Translate wasm to cranelift IR.
        translate_module(&buffer_source, &mut cranelift_module).map_err(|e| e.to_string())?;

        // Return translated module.
        Ok(cranelift_module)
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
    pub fn generate_signature(&self, sig_index: SignatureIndex) -> ir::Signature {
        // Get signature
        let mut signature = self.module.signatures[sig_index.index()].clone();

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
    fn make_global(
        &mut self,
        func: &mut ir::Function,
        global_index: GlobalIndex,
    ) -> GlobalVariable {
        // Create VMContext value.
        let vmctx = func.create_global_value(ir::GlobalValueData::VMContext);

        if global_index.index() < self.module.imported_globals.len() {
            // imported global

            let imported_globals_base_addr = func.create_global_value(ir::GlobalValueData::Load {
                base: vmctx,
                offset: (vm::Ctx::offset_imported_globals() as i32).into(),
                global_type: self.pointer_type(),
                readonly: true,
            });

            let offset = global_index.index() * vm::ImportedGlobal::size() as usize;

            let imported_global_addr = func.create_global_value(ir::GlobalValueData::IAddImm {
                base: imported_globals_base_addr,
                offset: (offset as i64).into(),
                global_type: self.pointer_type(),
            });

            let local_global_addr = func.create_global_value(ir::GlobalValueData::Load {
                base: imported_global_addr,
                offset: (vm::ImportedGlobal::offset_global() as i32).into(),
                global_type: self.pointer_type(),
                readonly: true,
            });

            GlobalVariable::Memory {
                gv: local_global_addr,
                offset: (vm::LocalGlobal::offset_data() as i32).into(),
                ty: self.module.get_global(global_index).ty,
            }
        } else {
            // locally defined global

            let globals_base_addr = func.create_global_value(ir::GlobalValueData::Load {
                base: vmctx,
                offset: (vm::Ctx::offset_globals() as i32).into(),
                global_type: self.pointer_type(),
                readonly: true,
            });

            // *Ctx.globals -> [ u8, u8, .. ]
            // Based on the index provided, we need to know the offset into globals array
            let offset = (global_index.index() - self.module.imported_globals.len())
                * vm::LocalGlobal::size() as usize;

            let local_global_addr = func.create_global_value(ir::GlobalValueData::IAddImm {
                base: globals_base_addr,
                offset: (offset as i64).into(),
                global_type: self.pointer_type(),
            });

            // Create global variable based on the data above.
            GlobalVariable::Memory {
                gv: local_global_addr,
                offset: (vm::LocalGlobal::offset_data() as i32).into(),
                ty: self.module.get_global(global_index).ty,
            }
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
    fn make_table(&mut self, func: &mut ir::Function, table_index: TableIndex) -> ir::Table {
        // Only the first table is supported for now.
        debug_assert_eq!(
            table_index.index(),
            0,
            "non-default tables not supported yet"
        );

        // Create VMContext value.
        let vmctx = func.create_global_value(ir::GlobalValueData::VMContext);

        // Load value at (vmctx + memories_offset) which is the address at Ctx.tables.
        let base = func.create_global_value(ir::GlobalValueData::Load {
            base: vmctx,
            offset: (vm::Ctx::offset_tables() as i32).into(),
            global_type: self.pointer_type(),
            readonly: true,
        });

        // *Ctx.tables -> [ {data: *usize, len: usize}, {data: *usize, len: usize}, ... ]
        // Based on the index provided, we need to know the offset into tables array.
        let table_data_offset = table_index.index() * mem::size_of::<vm::LocalTable>();

        let table_data_struct = func.create_global_value(ir::GlobalValueData::IAddImm {
            base,
            offset: (table_data_offset as i64).into(),
            global_type: self.pointer_type(),
        });

        // Load value at (base + table_data_offset), i.e. the address at Ctx.tables[index].data
        let base_gv = func.create_global_value(ir::GlobalValueData::Load {
            base: table_data_struct,
            offset: (vm::LocalTable::offset_base() as i32).into(),
            global_type: self.pointer_type(),
            readonly: false,
        });

        // Load value at (base + table_data_offset), i.e. the value at Ctx.tables[index].len
        let bound_gv = func.create_global_value(ir::GlobalValueData::Load {
            base: table_data_struct,
            offset: (vm::LocalTable::offset_current_elements() as i32).into(),
            global_type: self.pointer_type(),
            readonly: false,
        });

        // Create table based on the data above
        func.create_table(ir::TableData {
            base_gv,
            min_size: Uimm64::new(0),
            bound_gv,
            element_size: Uimm64::new(mem::size_of::<vm::Anyfunc>() as u64),
            index_type: I32,
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
    #[cfg_attr(feature = "cargo-clippy", allow(clippy::too_many_arguments))]
    fn translate_call_indirect(
        &mut self,
        mut pos: FuncCursor,
        _table_index: TableIndex,
        table: ir::Table,
        sig_index: SignatureIndex,
        sig_ref: ir::SigRef,
        callee: ir::Value,
        call_args: &[ir::Value],
    ) -> WasmResult<ir::Inst> {
        // Get the pointer type based on machine's pointer size.
        let ptr_type = self.pointer_type();

        // The `callee` value is an index into a table of Anyfunc structures.
        let entry_addr = pos.ins().table_addr(ptr_type, table, callee, 0);

        let mflags = ir::MemFlags::trusted();

        let func_ptr = pos.ins().load(
            ptr_type,
            mflags,
            entry_addr,
            vm::Anyfunc::offset_func() as i32,
        );
        let vmctx_ptr = pos.ins().load(
            ptr_type,
            mflags,
            entry_addr,
            vm::Anyfunc::offset_vmctx() as i32,
        );
        let found_sig =
            pos.ins()
                .load(I32, mflags, entry_addr, vm::Anyfunc::offset_sig_id() as i32);

        pos.ins().trapz(func_ptr, TrapCode::IndirectCallToNull);

        let deduplicated_sig_index = self
            .module
            .sig_registry
            .lookup_deduplicated_sigindex(WasmerSignatureIndex::new(sig_index.index()));
        let expected_sig = pos.ins().iconst(I32, deduplicated_sig_index.index() as i64);
        let not_equal_flags = pos.ins().ifcmp(found_sig, expected_sig);

        pos.ins()
            .trapif(IntCC::NotEqual, not_equal_flags, TrapCode::BadSignature);

        // Build a value list for the indirect call instruction containing the call_args
        // and the vmctx parameter.
        let mut args = Vec::with_capacity(call_args.len() + 1);
        args.extend(call_args.iter().cloned());
        args.push(vmctx_ptr);

        Ok(pos.ins().call_indirect(sig_ref, func_ptr, &args))
    }

    /// Generates a call IR with `callee` and `call_args` and inserts it at `pos`
    /// TODO: add support for imported functions
    fn translate_call(
        &mut self,
        mut pos: FuncCursor,
        callee_index: FuncIndex,
        callee: ir::FuncRef,
        call_args: &[ir::Value],
    ) -> WasmResult<ir::Inst> {
        // Insert call instructions for `callee`.
        if callee_index.index() < self.module.imported_functions.len() {
            // this is an imported function
            let vmctx = pos.func.create_global_value(ir::GlobalValueData::VMContext);

            let imported_funcs = pos.func.create_global_value(ir::GlobalValueData::Load {
                base: vmctx,
                offset: (WasmerVMContext::offset_imported_funcs() as i32).into(),
                global_type: self.pointer_type(),
                readonly: true,
            });

            let imported_func_struct_addr =
                pos.func.create_global_value(ir::GlobalValueData::IAddImm {
                    base: imported_funcs,
                    offset: (callee_index.index() as i64 * vm::ImportedFunc::size() as i64).into(),
                    global_type: self.pointer_type(),
                });

            let imported_func_addr = pos.func.create_global_value(ir::GlobalValueData::Load {
                base: imported_func_struct_addr,
                offset: (vm::ImportedFunc::offset_func() as i32).into(),
                global_type: self.pointer_type(),
                readonly: true,
            });

            let imported_vmctx_addr = pos.func.create_global_value(ir::GlobalValueData::Load {
                base: imported_func_struct_addr,
                offset: (vm::ImportedFunc::offset_vmctx() as i32).into(),
                global_type: self.pointer_type(),
                readonly: true,
            });

            let imported_func_addr = pos
                .ins()
                .global_value(self.pointer_type(), imported_func_addr);
            let imported_vmctx_addr = pos
                .ins()
                .global_value(self.pointer_type(), imported_vmctx_addr);

            let sig_ref = pos.func.dfg.ext_funcs[callee].signature;

            let mut args = Vec::with_capacity(call_args.len() + 1);
            args.extend(call_args.iter().cloned());
            args.push(imported_vmctx_addr);

            Ok(pos
                .ins()
                .call_indirect(sig_ref, imported_func_addr, &args[..]))
        } else {
            // this is an internal function
            let vmctx = pos
                .func
                .special_param(ir::ArgumentPurpose::VMContext)
                .expect("missing vmctx parameter");

            let mut args = Vec::with_capacity(call_args.len() + 1);
            args.extend(call_args.iter().cloned());
            args.push(vmctx);

            Ok(pos.ins().call(callee, &args))
        }
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
        _heap: ir::Heap,
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
                    // Param for memory index.
                    AbiParam::new(I32),
                    // Param for new size.
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
        let call_inst = pos.ins().call(grow_mem_func, &[memory_index, val, vmctx]);

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
        _heap: ir::Heap,
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
        let memory_index = pos.ins().iconst(I32, to_imm64(index.index()));

        // Create a VMContext value.
        let vmctx = pos.func.special_param(ArgumentPurpose::VMContext).unwrap();

        // Insert call instructions for `current_memory`.
        let call_inst = pos.ins().call(cur_mem_func, &[memory_index, vmctx]);

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
        let wasmer_sig = converter::convert_signature(sig);
        self.sig_registry.register(wasmer_sig);
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
        // let base = base.map(|index| WasmerGlobalIndex::new(index.index()));
        let base = match base {
            Some(global_index) => {
                WasmerInitializer::GetGlobal(WasmerGlobalIndex::new(global_index.index()))
            }
            None => WasmerInitializer::Const((offset as i32).into()),
        };

        // Add table initializer to list of table initializers
        self.elem_initializers.push(TableInitializer {
            table_index: WasmerTableIndex::new(table_index.index()),
            base,
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
            ExportIndex::Func(WasmerFuncIndex::new(func_index.index())),
        );
    }
    /// Declares a table export to the environment.
    fn declare_table_export(&mut self, table_index: TableIndex, name: &'data str) {
        self.exports.insert(
            String::from(name),
            ExportIndex::Table(WasmerTableIndex::new(table_index.index())),
        );
    }
    /// Declares a memory export to the environment.
    fn declare_memory_export(&mut self, memory_index: MemoryIndex, name: &'data str) {
        self.exports.insert(
            String::from(name),
            ExportIndex::Memory(WasmerMemoryIndex::new(memory_index.index())),
        );
    }
    /// Declares a global export to the environment.
    fn declare_global_export(&mut self, global_index: GlobalIndex, name: &'data str) {
        self.exports.insert(
            String::from(name),
            ExportIndex::Global(WasmerGlobalIndex::new(global_index.index())),
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
