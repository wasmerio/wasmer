// Parts of the following code are Copyright 2018 Cranelift Developers
// and subject to the license https://github.com/CraneStation/cranelift/blob/c47ca7bafc8fc48358f1baa72360e61fc1f7a0f2/cranelift-wasm/LICENSE

use crate::{
    cache::CacheGenerator, get_isa, module, module::Converter, relocation::call_names,
    resolver::FuncResolverBuilder, signal::Caller, trampoline::Trampolines,
};

use cranelift_codegen::entity::EntityRef;
use cranelift_codegen::ir::{self, Block, Function, InstBuilder};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::{cursor::FuncCursor, isa};
use cranelift_frontend::{FunctionBuilder, Position, Variable};
use cranelift_wasm::{self, FuncTranslator, ModuleTranslationState};
use cranelift_wasm::{get_vmctx_value_label, translate_operator};
use cranelift_wasm::{FuncEnvironment, ReturnMode, TargetEnvironment, WasmError};

use std::mem;
use std::sync::{Arc, RwLock};
use wasmer_runtime_core::error::CompileError;
use wasmer_runtime_core::{
    backend::{CacheGen, CompilerConfig, Token},
    cache::{Artifact, Error as CacheError},
    codegen::*,
    memory::BackingMemoryType,
    module::{ModuleInfo, ModuleInner},
    structures::{Map, TypedIndex},
    types::{
        FuncIndex, FuncSig, GlobalIndex, LocalFuncIndex, LocalOrImport, MemoryIndex, SigIndex,
        TableIndex,
    },
    vm,
};
use wasmparser::Type as WpType;

static BACKEND_ID: &str = "cranelift";

pub struct CraneliftModuleCodeGenerator {
    isa: Option<Box<dyn isa::TargetIsa>>,
    signatures: Option<Arc<Map<SigIndex, FuncSig>>>,
    pub clif_signatures: Map<SigIndex, ir::Signature>,
    function_signatures: Option<Arc<Map<FuncIndex, SigIndex>>>,
    functions: Vec<CraneliftFunctionCodeGenerator>,
}

impl ModuleCodeGenerator<CraneliftFunctionCodeGenerator, Caller, CodegenError>
    for CraneliftModuleCodeGenerator
{
    fn new() -> Self {
        CraneliftModuleCodeGenerator {
            isa: None,
            clif_signatures: Map::new(),
            functions: vec![],
            function_signatures: None,
            signatures: None,
        }
    }

    fn new_with_target(_: Option<String>, _: Option<String>, _: Option<String>) -> Self {
        unimplemented!("cross compilation is not available for clif backend")
    }

    fn backend_id() -> &'static str {
        BACKEND_ID
    }

    fn check_precondition(&mut self, _module_info: &ModuleInfo) -> Result<(), CodegenError> {
        Ok(())
    }

    fn next_function(
        &mut self,
        module_info: Arc<RwLock<ModuleInfo>>,
        loc: WasmSpan,
    ) -> Result<&mut CraneliftFunctionCodeGenerator, CodegenError> {
        // define_function_body(

        let func_translator = FuncTranslator::new();

        let func_index = LocalFuncIndex::new(self.functions.len());
        let name = ir::ExternalName::user(0, func_index.index() as u32);

        let sig = generate_signature(
            self,
            self.get_func_type(
                &module_info.read().unwrap(),
                Converter(func_index.convert_up(&module_info.read().unwrap())).into(),
            ),
        );

        let func = ir::Function::with_name_signature(name, sig);

        //func_translator.translate(body_bytes, body_offset, &mut func, &mut func_env)?;

        let mut func_env = CraneliftFunctionCodeGenerator {
            func,
            func_translator,
            next_local: 0,
            position: Position::default(),
            func_env: FunctionEnvironment {
                module_info: Arc::clone(&module_info),
                target_config: self.isa.as_ref().unwrap().frontend_config().clone(),
                clif_signatures: self.clif_signatures.clone(),
            },
            loc,
        };

        let generate_debug_info = module_info.read().unwrap().generate_debug_info;
        if generate_debug_info {
            func_env.func.collect_debug_info();
        }

        debug_assert_eq!(func_env.func.dfg.num_blocks(), 0, "Function must be empty");
        debug_assert_eq!(func_env.func.dfg.num_insts(), 0, "Function must be empty");

        let mut builder = FunctionBuilder::new(
            &mut func_env.func,
            &mut func_env.func_translator.func_ctx,
            &mut func_env.position,
        );

        builder.set_srcloc(ir::SourceLoc::new(loc.start()));

        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block); // This also creates values for the arguments.
        builder.seal_block(entry_block);
        // Make sure the entry block is inserted in the layout before we make any callbacks to
        // `environ`. The callback functions may need to insert things in the entry block.
        builder.ensure_inserted_block();

        declare_wasm_parameters(&mut builder, entry_block);

        // Set up the translation state with a single pushed control block representing the whole
        // function and its return values.
        let exit_block = builder.create_block();
        builder.append_block_params_for_function_returns(exit_block);
        func_env
            .func_translator
            .state
            .initialize(&builder.func.signature, exit_block);

        self.functions.push(func_env);
        Ok(self.functions.last_mut().unwrap())
    }

    fn finalize(
        self,
        module_info: &ModuleInfo,
    ) -> Result<
        (
            Caller,
            Option<wasmer_runtime_core::codegen::DebugMetadata>,
            Box<dyn CacheGen>,
        ),
        CodegenError,
    > {
        let mut func_bodies: Map<LocalFuncIndex, (ir::Function, WasmSpan)> = Map::new();
        for f in self.functions.into_iter() {
            func_bodies.push((f.func, f.loc));
        }

        let (func_resolver_builder, debug_metadata, handler_data) =
            FuncResolverBuilder::new(&**self.isa.as_ref().unwrap(), func_bodies, module_info)?;

        let trampolines = Arc::new(Trampolines::new(&**self.isa.as_ref().unwrap(), module_info));

        let signatures_empty = Map::new();
        let signatures = if self.signatures.is_some() {
            &self.signatures.as_ref().unwrap()
        } else {
            &signatures_empty
        };

        let (func_resolver, backend_cache) = func_resolver_builder.finalize(
            signatures,
            Arc::clone(&trampolines),
            handler_data.clone(),
        )?;

        let cache_gen = Box::new(CacheGenerator::new(
            backend_cache,
            Arc::clone(&func_resolver.memory),
        ));

        Ok((
            Caller::new(handler_data, trampolines, func_resolver),
            debug_metadata,
            cache_gen,
        ))
    }

    fn feed_compiler_config(&mut self, config: &CompilerConfig) -> Result<(), CodegenError> {
        self.isa = Some(get_isa(Some(config)));
        Ok(())
    }

    fn feed_signatures(&mut self, signatures: Map<SigIndex, FuncSig>) -> Result<(), CodegenError> {
        self.signatures = Some(Arc::new(signatures));
        let call_conv = self
            .isa
            .as_ref()
            .unwrap()
            .frontend_config()
            .default_call_conv;
        for (_sig_idx, func_sig) in self.signatures.as_ref().unwrap().iter() {
            self.clif_signatures
                .push(convert_func_sig(func_sig, call_conv));
        }
        Ok(())
    }

    fn feed_function_signatures(
        &mut self,
        assoc: Map<FuncIndex, SigIndex>,
    ) -> Result<(), CodegenError> {
        self.function_signatures = Some(Arc::new(assoc));
        Ok(())
    }

    fn feed_import_function(&mut self, _sigindex: SigIndex) -> Result<(), CodegenError> {
        Ok(())
    }

    unsafe fn from_cache(cache: Artifact, _: Token) -> Result<ModuleInner, CacheError> {
        module::Module::from_cache(cache)
    }
}

fn convert_func_sig(sig: &FuncSig, call_conv: CallConv) -> ir::Signature {
    ir::Signature {
        params: sig
            .params()
            .iter()
            .map(|params| Converter(*params).into())
            .collect::<Vec<_>>(),
        returns: sig
            .returns()
            .iter()
            .map(|returns| Converter(*returns).into())
            .collect::<Vec<_>>(),
        call_conv,
    }
}

impl From<CompileError> for CodegenError {
    fn from(other: CompileError) -> CodegenError {
        CodegenError {
            message: format!("{:?}", other),
        }
    }
}

impl From<WasmError> for CodegenError {
    fn from(other: WasmError) -> CodegenError {
        CodegenError {
            message: format!("{:?}", other),
        }
    }
}

pub struct CraneliftFunctionCodeGenerator {
    func: Function,
    func_translator: FuncTranslator,
    next_local: usize,
    position: Position,
    func_env: FunctionEnvironment,
    /// Where the function lives in the Wasm module as a span of bytes
    loc: WasmSpan,
}

pub struct FunctionEnvironment {
    module_info: Arc<RwLock<ModuleInfo>>,
    target_config: isa::TargetFrontendConfig,
    clif_signatures: Map<SigIndex, ir::Signature>,
}

impl TargetEnvironment for FunctionEnvironment {
    /// Gets configuration information needed for compiling functions
    fn target_config(&self) -> isa::TargetFrontendConfig {
        self.target_config
    }

    /// Gets native pointers types.
    ///
    /// `I64` on 64-bit arch; `I32` on 32-bit arch.
    fn pointer_type(&self) -> ir::Type {
        ir::Type::int(u16::from(self.target_config().pointer_bits())).unwrap()
    }

    /// Gets the size of a native pointer in bytes.
    fn pointer_bytes(&self) -> u8 {
        self.target_config().pointer_bytes()
    }

    /// Return Cranelift reference type.
    fn reference_type(&self) -> ir::Type {
        ir::types::R64
    }
}

impl FuncEnvironment for FunctionEnvironment {
    /// Sets up the necessary preamble definitions in `func` to access the global identified
    /// by `index`.
    ///
    /// The index space covers both imported and locally declared globals.
    fn make_global(
        &mut self,
        func: &mut ir::Function,
        clif_global_index: cranelift_wasm::GlobalIndex,
    ) -> cranelift_wasm::WasmResult<cranelift_wasm::GlobalVariable> {
        let global_index: GlobalIndex = Converter(clif_global_index).into();

        // Create VMContext value.
        let vmctx = func.create_global_value(ir::GlobalValueData::VMContext);
        let ptr_type = self.pointer_type();

        let (local_global_addr, ty) = match global_index
            .local_or_import(&self.module_info.read().unwrap())
        {
            LocalOrImport::Local(local_global_index) => {
                let globals_base_addr = func.create_global_value(ir::GlobalValueData::Load {
                    base: vmctx,
                    offset: (vm::Ctx::offset_globals() as i32).into(),
                    global_type: ptr_type,
                    readonly: true,
                });

                let offset = local_global_index.index() * mem::size_of::<*mut vm::LocalGlobal>();

                let local_global_ptr_ptr = func.create_global_value(ir::GlobalValueData::IAddImm {
                    base: globals_base_addr,
                    offset: (offset as i64).into(),
                    global_type: ptr_type,
                });

                let ty = self.module_info.read().unwrap().globals[local_global_index]
                    .desc
                    .ty;

                (
                    func.create_global_value(ir::GlobalValueData::Load {
                        base: local_global_ptr_ptr,
                        offset: 0.into(),
                        global_type: ptr_type,
                        readonly: true,
                    }),
                    ty,
                )
            }
            LocalOrImport::Import(import_global_index) => {
                let globals_base_addr = func.create_global_value(ir::GlobalValueData::Load {
                    base: vmctx,
                    offset: (vm::Ctx::offset_imported_globals() as i32).into(),
                    global_type: ptr_type,
                    readonly: true,
                });

                let offset = import_global_index.index() * mem::size_of::<*mut vm::LocalGlobal>();

                let local_global_ptr_ptr = func.create_global_value(ir::GlobalValueData::IAddImm {
                    base: globals_base_addr,
                    offset: (offset as i64).into(),
                    global_type: ptr_type,
                });

                let ty = self.module_info.read().unwrap().imported_globals[import_global_index]
                    .1
                    .ty;

                (
                    func.create_global_value(ir::GlobalValueData::Load {
                        base: local_global_ptr_ptr,
                        offset: 0.into(),
                        global_type: ptr_type,
                        readonly: true,
                    }),
                    ty,
                )
            }
        };

        Ok(cranelift_wasm::GlobalVariable::Memory {
            gv: local_global_addr,
            offset: (vm::LocalGlobal::offset_data() as i32).into(),
            ty: Converter(ty).into(),
        })
    }

    /// Sets up the necessary preamble definitions in `func` to access the linear memory identified
    /// by `index`.
    ///
    /// The index space covers both imported and locally declared memories.
    fn make_heap(
        &mut self,
        func: &mut ir::Function,
        clif_mem_index: cranelift_wasm::MemoryIndex,
    ) -> cranelift_wasm::WasmResult<ir::Heap> {
        let mem_index: MemoryIndex = Converter(clif_mem_index).into();
        // Create VMContext value.
        let vmctx = func.create_global_value(ir::GlobalValueData::VMContext);
        let ptr_type = self.pointer_type();

        let (local_memory_ptr_ptr, description) =
            match mem_index.local_or_import(&self.module_info.read().unwrap()) {
                LocalOrImport::Local(local_mem_index) => {
                    let memories_base_addr = func.create_global_value(ir::GlobalValueData::Load {
                        base: vmctx,
                        offset: (vm::Ctx::offset_memories() as i32).into(),
                        global_type: ptr_type,
                        readonly: true,
                    });

                    let local_memory_ptr_offset =
                        local_mem_index.index() * mem::size_of::<*mut vm::LocalMemory>();

                    (
                        func.create_global_value(ir::GlobalValueData::IAddImm {
                            base: memories_base_addr,
                            offset: (local_memory_ptr_offset as i64).into(),
                            global_type: ptr_type,
                        }),
                        self.module_info.read().unwrap().memories[local_mem_index],
                    )
                }
                LocalOrImport::Import(import_mem_index) => {
                    let memories_base_addr = func.create_global_value(ir::GlobalValueData::Load {
                        base: vmctx,
                        offset: (vm::Ctx::offset_imported_memories() as i32).into(),
                        global_type: ptr_type,
                        readonly: true,
                    });

                    let local_memory_ptr_offset =
                        import_mem_index.index() * mem::size_of::<*mut vm::LocalMemory>();

                    (
                        func.create_global_value(ir::GlobalValueData::IAddImm {
                            base: memories_base_addr,
                            offset: (local_memory_ptr_offset as i64).into(),
                            global_type: ptr_type,
                        }),
                        self.module_info.read().unwrap().imported_memories[import_mem_index].1,
                    )
                }
            };

        let (local_memory_ptr, local_memory_base) = {
            let local_memory_ptr = func.create_global_value(ir::GlobalValueData::Load {
                base: local_memory_ptr_ptr,
                offset: 0.into(),
                global_type: ptr_type,
                readonly: true,
            });

            (
                local_memory_ptr,
                func.create_global_value(ir::GlobalValueData::Load {
                    base: local_memory_ptr,
                    offset: (vm::LocalMemory::offset_base() as i32).into(),
                    global_type: ptr_type,
                    readonly: false,
                }),
            )
        };

        match description.memory_type() {
            mem_type @ BackingMemoryType::Dynamic => {
                let local_memory_bound = func.create_global_value(ir::GlobalValueData::Load {
                    base: local_memory_ptr,
                    offset: (vm::LocalMemory::offset_bound() as i32).into(),
                    global_type: ir::types::I32,
                    readonly: false,
                });

                Ok(func.create_heap(ir::HeapData {
                    base: local_memory_base,
                    min_size: (description.minimum.bytes().0 as u64).into(),
                    offset_guard_size: mem_type.guard_size().into(),
                    style: ir::HeapStyle::Dynamic {
                        bound_gv: local_memory_bound,
                    },
                    index_type: ir::types::I32,
                }))
            }
            mem_type @ BackingMemoryType::Static | mem_type @ BackingMemoryType::SharedStatic => {
                Ok(func.create_heap(ir::HeapData {
                    base: local_memory_base,
                    min_size: (description.minimum.bytes().0 as u64).into(),
                    offset_guard_size: mem_type.guard_size().into(),
                    style: ir::HeapStyle::Static {
                        bound: mem_type.bounds().unwrap().into(),
                    },
                    index_type: ir::types::I32,
                }))
            }
        }
    }

    /// Sets up the necessary preamble definitions in `func` to access the table identified
    /// by `index`.
    ///
    /// The index space covers both imported and locally declared tables.
    fn make_table(
        &mut self,
        func: &mut ir::Function,
        clif_table_index: cranelift_wasm::TableIndex,
    ) -> cranelift_wasm::WasmResult<ir::Table> {
        let table_index: TableIndex = Converter(clif_table_index).into();
        // Create VMContext value.
        let vmctx = func.create_global_value(ir::GlobalValueData::VMContext);
        let ptr_type = self.pointer_type();

        let (table_struct_ptr_ptr, description) = match table_index
            .local_or_import(&self.module_info.read().unwrap())
        {
            LocalOrImport::Local(local_table_index) => {
                let tables_base = func.create_global_value(ir::GlobalValueData::Load {
                    base: vmctx,
                    offset: (vm::Ctx::offset_tables() as i32).into(),
                    global_type: ptr_type,
                    readonly: true,
                });

                let table_struct_ptr_offset =
                    local_table_index.index() * vm::LocalTable::size() as usize;

                let table_struct_ptr_ptr = func.create_global_value(ir::GlobalValueData::IAddImm {
                    base: tables_base,
                    offset: (table_struct_ptr_offset as i64).into(),
                    global_type: ptr_type,
                });

                (
                    table_struct_ptr_ptr,
                    self.module_info.read().unwrap().tables[local_table_index],
                )
            }
            LocalOrImport::Import(import_table_index) => {
                let tables_base = func.create_global_value(ir::GlobalValueData::Load {
                    base: vmctx,
                    offset: (vm::Ctx::offset_imported_tables() as i32).into(),
                    global_type: ptr_type,
                    readonly: true,
                });

                let table_struct_ptr_offset =
                    import_table_index.index() * vm::LocalTable::size() as usize;

                let table_struct_ptr_ptr = func.create_global_value(ir::GlobalValueData::IAddImm {
                    base: tables_base,
                    offset: (table_struct_ptr_offset as i64).into(),
                    global_type: ptr_type,
                });

                (
                    table_struct_ptr_ptr,
                    self.module_info.read().unwrap().imported_tables[import_table_index].1,
                )
            }
        };

        let table_struct_ptr = func.create_global_value(ir::GlobalValueData::Load {
            base: table_struct_ptr_ptr,
            offset: 0.into(),
            global_type: ptr_type,
            readonly: true,
        });

        let table_base = func.create_global_value(ir::GlobalValueData::Load {
            base: table_struct_ptr,
            offset: (vm::LocalTable::offset_base() as i32).into(),
            global_type: ptr_type,
            // The table can reallocate, so the ptr can't be readonly.
            readonly: false,
        });

        let table_count = func.create_global_value(ir::GlobalValueData::Load {
            base: table_struct_ptr,
            offset: (vm::LocalTable::offset_count() as i32).into(),
            global_type: ir::types::I32,
            // The table length can change, so it can't be readonly.
            readonly: false,
        });

        Ok(func.create_table(ir::TableData {
            base_gv: table_base,
            min_size: (description.minimum as u64).into(),
            bound_gv: table_count,
            element_size: (vm::Anyfunc::size() as u64).into(),
            index_type: ir::types::I32,
        }))
    }

    /// Sets up a signature definition in `func`'s preamble.
    ///
    /// Signature may contain additional argument, but arguments marked as ArgumentPurpose::Normal`
    /// must correspond to the arguments in the wasm signature
    fn make_indirect_sig(
        &mut self,
        func: &mut ir::Function,
        clif_sig_index: cranelift_wasm::SignatureIndex,
    ) -> cranelift_wasm::WasmResult<ir::SigRef> {
        // Create a signature reference out of specified signature (with VMContext param added).
        Ok(func.import_signature(self.generate_signature(clif_sig_index)))
    }

    /// Sets up an external function definition in the preamble of `func` that can be used to
    /// directly call the function `index`.
    ///
    /// The index space covers both imported functions and functions defined in the current module.
    fn make_direct_func(
        &mut self,
        func: &mut ir::Function,
        func_index: cranelift_wasm::FuncIndex,
    ) -> cranelift_wasm::WasmResult<ir::FuncRef> {
        // Get signature of function.
        let signature_index = self.get_func_type(func_index);

        // Create a signature reference from specified signature (with VMContext param added).
        let signature = func.import_signature(self.generate_signature(signature_index));

        // Get name of function.
        let name = ir::ExternalName::user(0, func_index.as_u32());

        // Create function reference from fuction data.
        Ok(func.import_function(ir::ExtFuncData {
            name,
            signature,
            // Make this colocated so all calls between local functions are relative.
            colocated: true,
        }))
    }

    /// Generates an indirect call IR with `callee` and `call_args`.
    ///
    /// Inserts instructions at `pos` to the function `callee` in the table
    /// `table_index` with WebAssembly signature `sig_index`
    #[cfg_attr(feature = "cargo-clippy", allow(clippy::too_many_arguments))]
    fn translate_call_indirect(
        &mut self,
        mut pos: FuncCursor,
        _table_index: cranelift_wasm::TableIndex,
        table: ir::Table,
        clif_sig_index: cranelift_wasm::SignatureIndex,
        sig_ref: ir::SigRef,
        callee: ir::Value,
        call_args: &[ir::Value],
    ) -> cranelift_wasm::WasmResult<ir::Inst> {
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

        let vmctx_ptr = {
            let loaded_vmctx_ptr = pos.ins().load(
                ptr_type,
                mflags,
                entry_addr,
                vm::Anyfunc::offset_vmctx() as i32,
            );

            let argument_vmctx_ptr = pos
                .func
                .special_param(ir::ArgumentPurpose::VMContext)
                .expect("missing vmctx parameter");

            // If the loaded vmctx ptr is zero, use the caller vmctx, else use the callee (loaded) vmctx.
            pos.ins()
                .select(loaded_vmctx_ptr, loaded_vmctx_ptr, argument_vmctx_ptr)
        };

        let found_sig = pos.ins().load(
            ir::types::I32,
            mflags,
            entry_addr,
            vm::Anyfunc::offset_sig_id() as i32,
        );

        pos.ins().trapz(func_ptr, ir::TrapCode::IndirectCallToNull);

        let expected_sig = {
            let sig_index_global = pos.func.create_global_value(ir::GlobalValueData::Symbol {
                // The index of the `ExternalName` is the undeduplicated, signature index.
                name: ir::ExternalName::user(
                    call_names::SIG_NAMESPACE,
                    clif_sig_index.index() as u32,
                ),
                offset: 0.into(),
                colocated: false,
            });

            let val = pos.ins().symbol_value(ir::types::I64, sig_index_global);
            pos.ins().ireduce(ir::types::I32, val)

            // let dynamic_sigindices_array_ptr = pos.ins().load(
            //     ptr_type,
            //     mflags,

            // )

            // let expected_sig = pos.ins().iconst(ir::types::I32, sig_index.index() as i64);

            // self.env.deduplicated[clif_sig_index]
        };

        let not_equal_flags = pos.ins().ifcmp(found_sig, expected_sig);

        pos.ins().trapif(
            ir::condcodes::IntCC::NotEqual,
            not_equal_flags,
            ir::TrapCode::BadSignature,
        );

        // Build a value list for the indirect call instruction containing the call_args
        // and the vmctx parameter.
        let mut args = Vec::with_capacity(call_args.len() + 1);
        args.push(vmctx_ptr);
        args.extend(call_args.iter().cloned());

        Ok(pos.ins().call_indirect(sig_ref, func_ptr, &args))
    }

    /// Generates a call IR with `callee` and `call_args` and inserts it at `pos`
    ///
    /// It's about generating code that calls a local or imported function; in
    /// WebAssembly: `(call $foo)`.
    fn translate_call(
        &mut self,
        mut pos: FuncCursor,
        clif_callee_index: cranelift_wasm::FuncIndex,
        callee: ir::FuncRef,
        call_args: &[ir::Value],
    ) -> cranelift_wasm::WasmResult<ir::Inst> {
        let callee_index: FuncIndex = Converter(clif_callee_index).into();
        let ptr_type = self.pointer_type();

        match callee_index.local_or_import(&self.module_info.read().unwrap()) {
            LocalOrImport::Local(local_function_index) => {
                // this is an internal function
                let vmctx = pos
                    .func
                    .special_param(ir::ArgumentPurpose::VMContext)
                    .expect("missing vmctx parameter");

                let mut args = Vec::with_capacity(call_args.len() + 1);
                args.push(vmctx);
                args.extend(call_args.iter().cloned());

                let sig_ref = pos.func.dfg.ext_funcs[callee].signature;
                let function_ptr = {
                    let mflags = ir::MemFlags::trusted();

                    let function_array_ptr = pos.ins().load(
                        ptr_type,
                        mflags,
                        vmctx,
                        vm::Ctx::offset_local_functions() as i32,
                    );

                    pos.ins().load(
                        ptr_type,
                        mflags,
                        function_array_ptr,
                        (local_function_index.index() as i32) * 8,
                    )
                };

                Ok(pos.ins().call_indirect(sig_ref, function_ptr, &args))
            }
            LocalOrImport::Import(imported_func_index) => {
                // this is an imported function
                let vmctx = pos.func.create_global_value(ir::GlobalValueData::VMContext);

                let imported_funcs = pos.func.create_global_value(ir::GlobalValueData::Load {
                    base: vmctx,
                    offset: (vm::Ctx::offset_imported_funcs() as i32).into(),
                    global_type: ptr_type,
                    readonly: true,
                });

                let imported_func_offset =
                    imported_func_index.index() * vm::ImportedFunc::size() as usize;

                let imported_func_struct_addr =
                    pos.func.create_global_value(ir::GlobalValueData::IAddImm {
                        base: imported_funcs,
                        offset: (imported_func_offset as i64).into(),
                        global_type: ptr_type,
                    });

                let imported_func_addr = pos.func.create_global_value(ir::GlobalValueData::Load {
                    base: imported_func_struct_addr,
                    offset: (vm::ImportedFunc::offset_func() as i32).into(),
                    global_type: ptr_type,
                    readonly: true,
                });

                let imported_func_ctx_addr =
                    pos.func.create_global_value(ir::GlobalValueData::Load {
                        base: imported_func_struct_addr,
                        offset: (vm::ImportedFunc::offset_func_ctx() as i32).into(),
                        global_type: ptr_type,
                        readonly: true,
                    });

                let imported_func_ctx_vmctx_addr =
                    pos.func.create_global_value(ir::GlobalValueData::Load {
                        base: imported_func_ctx_addr,
                        offset: (vm::FuncCtx::offset_vmctx() as i32).into(),
                        global_type: ptr_type,
                        readonly: true,
                    });

                let imported_func_addr = pos.ins().global_value(ptr_type, imported_func_addr);
                let imported_func_ctx_vmctx_addr = pos
                    .ins()
                    .global_value(ptr_type, imported_func_ctx_vmctx_addr);

                let sig_ref = pos.func.dfg.ext_funcs[callee].signature;

                let mut args = Vec::with_capacity(call_args.len() + 1);
                args.push(imported_func_ctx_vmctx_addr);
                args.extend(call_args.iter().cloned());

                Ok(pos
                    .ins()
                    .call_indirect(sig_ref, imported_func_addr, &args[..]))
            }
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
        clif_mem_index: cranelift_wasm::MemoryIndex,
        _heap: ir::Heap,
        by_value: ir::Value,
    ) -> cranelift_wasm::WasmResult<ir::Value> {
        let signature = pos.func.import_signature(ir::Signature {
            call_conv: self.target_config().default_call_conv,
            params: vec![
                ir::AbiParam::special(self.pointer_type(), ir::ArgumentPurpose::VMContext),
                ir::AbiParam::new(ir::types::I32),
                ir::AbiParam::new(ir::types::I32),
            ],
            returns: vec![ir::AbiParam::new(ir::types::I32)],
        });

        let mem_index: MemoryIndex = Converter(clif_mem_index).into();

        let (namespace, mem_index, description) =
            match mem_index.local_or_import(&self.module_info.read().unwrap()) {
                LocalOrImport::Local(local_mem_index) => (
                    call_names::LOCAL_NAMESPACE,
                    local_mem_index.index(),
                    self.module_info.read().unwrap().memories[local_mem_index],
                ),
                LocalOrImport::Import(import_mem_index) => (
                    call_names::IMPORT_NAMESPACE,
                    import_mem_index.index(),
                    self.module_info.read().unwrap().imported_memories[import_mem_index].1,
                ),
            };

        let name_index = match description.memory_type() {
            BackingMemoryType::Dynamic => call_names::DYNAMIC_MEM_GROW,
            BackingMemoryType::Static => call_names::STATIC_MEM_GROW,
            BackingMemoryType::SharedStatic => call_names::SHARED_STATIC_MEM_GROW,
        };

        let name = ir::ExternalName::user(namespace, name_index);

        let mem_grow_func = pos.func.import_function(ir::ExtFuncData {
            name,
            signature,
            colocated: false,
        });

        let const_mem_index = pos.ins().iconst(ir::types::I32, mem_index as i64);

        let vmctx = pos
            .func
            .special_param(ir::ArgumentPurpose::VMContext)
            .expect("missing vmctx parameter");

        let call_inst = pos
            .ins()
            .call(mem_grow_func, &[vmctx, const_mem_index, by_value]);

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
        clif_mem_index: cranelift_wasm::MemoryIndex,
        _heap: ir::Heap,
    ) -> cranelift_wasm::WasmResult<ir::Value> {
        let signature = pos.func.import_signature(ir::Signature {
            call_conv: self.target_config().default_call_conv,
            params: vec![
                ir::AbiParam::special(self.pointer_type(), ir::ArgumentPurpose::VMContext),
                ir::AbiParam::new(ir::types::I32),
            ],
            returns: vec![ir::AbiParam::new(ir::types::I32)],
        });

        let mem_index: MemoryIndex = Converter(clif_mem_index).into();

        let (namespace, mem_index, description) =
            match mem_index.local_or_import(&self.module_info.read().unwrap()) {
                LocalOrImport::Local(local_mem_index) => (
                    call_names::LOCAL_NAMESPACE,
                    local_mem_index.index(),
                    self.module_info.read().unwrap().memories[local_mem_index],
                ),
                LocalOrImport::Import(import_mem_index) => (
                    call_names::IMPORT_NAMESPACE,
                    import_mem_index.index(),
                    self.module_info.read().unwrap().imported_memories[import_mem_index].1,
                ),
            };

        let name_index = match description.memory_type() {
            BackingMemoryType::Dynamic => call_names::DYNAMIC_MEM_SIZE,
            BackingMemoryType::Static => call_names::STATIC_MEM_SIZE,
            BackingMemoryType::SharedStatic => call_names::SHARED_STATIC_MEM_SIZE,
        };

        let name = ir::ExternalName::user(namespace, name_index);

        let mem_grow_func = pos.func.import_function(ir::ExtFuncData {
            name,
            signature,
            colocated: false,
        });

        let const_mem_index = pos.ins().iconst(ir::types::I32, mem_index as i64);
        let vmctx = pos
            .func
            .special_param(ir::ArgumentPurpose::VMContext)
            .expect("missing vmctx parameter");

        let call_inst = pos.ins().call(mem_grow_func, &[vmctx, const_mem_index]);

        Ok(*pos.func.dfg.inst_results(call_inst).first().unwrap())
    }
    fn translate_memory_copy(
        &mut self,
        _pos: FuncCursor,
        _clif_mem_index: cranelift_wasm::MemoryIndex,
        _heap: ir::Heap,
        _dst: ir::Value,
        _src: ir::Value,
        _len: ir::Value,
    ) -> cranelift_wasm::WasmResult<()> {
        unimplemented!("memory.copy not yet implemented");
    }

    fn translate_memory_fill(
        &mut self,
        _pos: FuncCursor,
        _clif_mem_index: cranelift_wasm::MemoryIndex,
        _heap: ir::Heap,
        _dst: ir::Value,
        _val: ir::Value,
        _len: ir::Value,
    ) -> cranelift_wasm::WasmResult<()> {
        unimplemented!("memory.fill not yet implemented");
    }

    fn translate_memory_init(
        &mut self,
        _pos: FuncCursor,
        _clif_mem_index: cranelift_wasm::MemoryIndex,
        _heap: ir::Heap,
        _seg_index: u32,
        _dst: ir::Value,
        _src: ir::Value,
        _len: ir::Value,
    ) -> cranelift_wasm::WasmResult<()> {
        unimplemented!("memory.init not yet implemented");
    }

    fn translate_data_drop(
        &mut self,
        _pos: FuncCursor,
        _seg_index: u32,
    ) -> cranelift_wasm::WasmResult<()> {
        unimplemented!("data.drop not yet implemented");
    }

    fn translate_table_size(
        &mut self,
        _pos: FuncCursor,
        _index: cranelift_wasm::TableIndex,
        _table: ir::Table,
    ) -> cranelift_wasm::WasmResult<ir::Value> {
        unimplemented!("table.size not yet implemented");
    }

    fn translate_table_copy(
        &mut self,
        _pos: FuncCursor,
        _dst_table_index: cranelift_wasm::TableIndex,
        _dst_table: ir::Table,
        _src_table_index: cranelift_wasm::TableIndex,
        _src_table: ir::Table,
        _dst: ir::Value,
        _src: ir::Value,
        _len: ir::Value,
    ) -> cranelift_wasm::WasmResult<()> {
        unimplemented!("table.copy not yet implemented");
    }

    fn translate_table_init(
        &mut self,
        _pos: FuncCursor,
        _seg_index: u32,
        _table_index: cranelift_wasm::TableIndex,
        _table: ir::Table,
        _dst: ir::Value,
        _src: ir::Value,
        _len: ir::Value,
    ) -> cranelift_wasm::WasmResult<()> {
        unimplemented!("table.init not yet implemented");
    }

    fn translate_elem_drop(
        &mut self,
        _pos: FuncCursor,
        _seg_index: u32,
    ) -> cranelift_wasm::WasmResult<()> {
        unimplemented!("elem.drop not yet implemented");
    }

    fn translate_table_grow(
        &mut self,
        _pos: FuncCursor,
        _table_index: u32,
        _delta: ir::Value,
        _init_value: ir::Value,
    ) -> cranelift_wasm::WasmResult<ir::Value> {
        unimplemented!("table.grow not yet implemented");
    }

    fn translate_table_get(
        &mut self,
        _pos: FuncCursor,
        _table_index: u32,
        _index: ir::Value,
    ) -> cranelift_wasm::WasmResult<ir::Value> {
        unimplemented!("table.get not yet implemented");
    }

    fn translate_table_set(
        &mut self,
        _pos: FuncCursor,
        _table_index: u32,
        _value: ir::Value,
        _index: ir::Value,
    ) -> cranelift_wasm::WasmResult<()> {
        unimplemented!("table.set not yet implemented");
    }

    fn translate_table_fill(
        &mut self,
        _pos: FuncCursor,
        _table_index: u32,
        _dst: ir::Value,
        _val: ir::Value,
        _len: ir::Value,
    ) -> cranelift_wasm::WasmResult<()> {
        unimplemented!("table.fill not yet implemented");
    }

    fn translate_ref_func(
        &mut self,
        _pos: FuncCursor,
        _func_index: u32,
    ) -> cranelift_wasm::WasmResult<ir::Value> {
        unimplemented!("ref.func not yet implemented");
    }

    fn translate_custom_global_get(
        &mut self,
        _pos: FuncCursor,
        _global_index: cranelift_wasm::GlobalIndex,
    ) -> cranelift_wasm::WasmResult<ir::Value> {
        unimplemented!("custom global.get not yet implemented");
    }

    fn translate_custom_global_set(
        &mut self,
        _pos: FuncCursor,
        _global_index: cranelift_wasm::GlobalIndex,
        _val: ir::Value,
    ) -> cranelift_wasm::WasmResult<()> {
        unimplemented!("custom global.set not yet implemented");
    }
}

impl FunctionEnvironment {
    pub fn get_func_type(
        &self,
        func_index: cranelift_wasm::FuncIndex,
    ) -> cranelift_wasm::SignatureIndex {
        let sig_index: SigIndex =
            self.module_info.read().unwrap().func_assoc[Converter(func_index).into()];
        Converter(sig_index).into()
    }

    /// Creates a signature with VMContext as the last param
    pub fn generate_signature(
        &self,
        clif_sig_index: cranelift_wasm::SignatureIndex,
    ) -> ir::Signature {
        // Get signature
        let mut signature = self.clif_signatures[Converter(clif_sig_index).into()].clone();

        // Add the vmctx parameter type to it
        signature.params.insert(
            0,
            ir::AbiParam::special(self.pointer_type(), ir::ArgumentPurpose::VMContext),
        );

        // Return signature
        signature
    }
}

impl FunctionCodeGenerator<CodegenError> for CraneliftFunctionCodeGenerator {
    fn feed_return(&mut self, _ty: WpType) -> Result<(), CodegenError> {
        Ok(())
    }

    fn feed_param(&mut self, _ty: WpType) -> Result<(), CodegenError> {
        self.next_local += 1;
        Ok(())
    }

    fn feed_local(&mut self, ty: WpType, n: usize, loc: u32) -> Result<(), CodegenError> {
        let mut next_local = self.next_local;
        let mut builder = FunctionBuilder::new(
            &mut self.func,
            &mut self.func_translator.func_ctx,
            &mut self.position,
        );
        builder.set_srcloc(ir::SourceLoc::new(loc));
        cranelift_wasm::declare_locals(
            &mut builder,
            n as u32,
            ty,
            &mut next_local,
            &mut self.func_env,
        )?;
        self.next_local = next_local;
        Ok(())
    }

    fn begin_body(&mut self, _module_info: &ModuleInfo) -> Result<(), CodegenError> {
        Ok(())
    }

    fn feed_event(
        &mut self,
        event: Event,
        _module_info: &ModuleInfo,
        source_loc: u32,
    ) -> Result<(), CodegenError> {
        let op = match event {
            Event::Wasm(x) => x,
            Event::WasmOwned(ref x) => x,
            Event::Internal(_x) => {
                return Ok(());
            }
        };

        //let builder = self.builder.as_mut().unwrap();
        //let func_environment = FuncEnv::new();
        //let state = TranslationState::new();

        if self.func_translator.state.control_stack.is_empty() {
            return Ok(());
        }

        let mut builder = FunctionBuilder::new(
            &mut self.func,
            &mut self.func_translator.func_ctx,
            &mut self.position,
        );
        builder.func.collect_debug_info();
        builder.set_srcloc(ir::SourceLoc::new(source_loc));
        let module_state = ModuleTranslationState::new();
        let func_state = &mut self.func_translator.state;
        translate_operator(
            &module_state,
            op,
            &mut builder,
            func_state,
            &mut self.func_env,
        )?;

        Ok(())
    }

    fn finalize(&mut self) -> Result<(), CodegenError> {
        let return_mode = self.return_mode();

        let mut builder = FunctionBuilder::new(
            &mut self.func,
            &mut self.func_translator.func_ctx,
            &mut self.position,
        );
        let state = &mut self.func_translator.state;

        // The final `End` operator left us in the exit block where we need to manually add a return
        // instruction.
        //
        // If the exit block is unreachable, it may not have the correct arguments, so we would
        // generate a return instruction that doesn't match the signature.
        if state.reachable() {
            debug_assert!(builder.is_pristine());
            if !builder.is_unreachable() {
                match return_mode {
                    ReturnMode::NormalReturns => builder.ins().return_(&state.stack),
                    ReturnMode::FallthroughReturn => builder.ins().fallthrough_return(&state.stack),
                };
            }
        }

        builder.finalize();

        // Discard any remaining values on the stack. Either we just returned them,
        // or the end of the function is unreachable.
        state.stack.clear();

        self.builder().finalize();
        Ok(())
    }
}

#[derive(Debug)]
pub struct CodegenError {
    pub message: String,
}

impl CraneliftModuleCodeGenerator {
    /// Return the signature index for the given function index.
    pub fn get_func_type(
        &self,
        module_info: &ModuleInfo,
        func_index: cranelift_wasm::FuncIndex,
    ) -> cranelift_wasm::SignatureIndex {
        let sig_index: SigIndex = module_info.func_assoc[Converter(func_index).into()];
        Converter(sig_index).into()
    }
}

impl CraneliftFunctionCodeGenerator {
    pub fn builder(&mut self) -> FunctionBuilder {
        FunctionBuilder::new(
            &mut self.func,
            &mut self.func_translator.func_ctx,
            &mut self.position,
        )
    }

    pub fn return_mode(&self) -> ReturnMode {
        ReturnMode::NormalReturns
    }
}

/// Creates a signature with VMContext as the last param
fn generate_signature(
    env: &CraneliftModuleCodeGenerator,
    clif_sig_index: cranelift_wasm::SignatureIndex,
) -> ir::Signature {
    // Get signature
    let mut signature = env.clif_signatures[Converter(clif_sig_index).into()].clone();

    // Add the vmctx parameter type to it
    signature.params.insert(
        0,
        ir::AbiParam::special(pointer_type(env), ir::ArgumentPurpose::VMContext),
    );

    // Return signature
    signature
}

fn pointer_type(mcg: &CraneliftModuleCodeGenerator) -> ir::Type {
    ir::Type::int(u16::from(
        mcg.isa.as_ref().unwrap().frontend_config().pointer_bits(),
    ))
    .unwrap()
}

/// Declare local variables for the signature parameters that correspond to WebAssembly locals.
///
/// Return the number of local variables declared.
fn declare_wasm_parameters(builder: &mut FunctionBuilder, entry_block: Block) -> usize {
    let sig_len = builder.func.signature.params.len();
    let mut next_local = 0;
    for i in 0..sig_len {
        let param_type = builder.func.signature.params[i];
        // There may be additional special-purpose parameters following the normal WebAssembly
        // signature parameters. For example, a `vmctx` pointer.
        if param_type.purpose == ir::ArgumentPurpose::Normal {
            // This is a normal WebAssembly signature parameter, so create a local for it.
            let local = Variable::new(next_local);
            builder.declare_var(local, param_type.value_type);
            next_local += 1;

            let param_value = builder.block_params(entry_block)[i];
            builder.def_var(local, param_value);
        }
        if param_type.purpose == ir::ArgumentPurpose::VMContext {
            let param_value = builder.block_params(entry_block)[i];
            builder.set_val_label(param_value, get_vmctx_value_label());
        }
    }

    next_local
}
