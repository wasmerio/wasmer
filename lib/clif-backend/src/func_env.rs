use crate::{module::Converter, module_env::ModuleEnv, relocation::call_names};
use cranelift_codegen::{
    cursor::FuncCursor,
    ir::{self, InstBuilder},
    isa,
};
use cranelift_entity::EntityRef;
use cranelift_wasm::{self, FuncEnvironment, ModuleEnvironment};
use std::mem;
use wasmer_runtime_core::{
    memory::MemoryType,
    structures::TypedIndex,
    types::{FuncIndex, GlobalIndex, LocalOrImport, MemoryIndex, TableIndex},
    vm,
};

pub struct FuncEnv<'env, 'module, 'isa> {
    env: &'env ModuleEnv<'module, 'isa>,
}

impl<'env, 'module, 'isa> FuncEnv<'env, 'module, 'isa> {
    pub fn new(env: &'env ModuleEnv<'module, 'isa>) -> Self {
        Self { env }
    }

    /// Creates a signature with VMContext as the last param
    pub fn generate_signature(
        &self,
        clif_sig_index: cranelift_wasm::SignatureIndex,
    ) -> ir::Signature {
        // Get signature
        let mut signature = self.env.signatures[Converter(clif_sig_index).into()].clone();

        // Add the vmctx parameter type to it
        signature.params.insert(
            0,
            ir::AbiParam::special(self.pointer_type(), ir::ArgumentPurpose::VMContext),
        );

        // Return signature
        signature
    }
}

impl<'env, 'module, 'isa> FuncEnvironment for FuncEnv<'env, 'module, 'isa> {
    /// Gets configuration information needed for compiling functions
    fn target_config(&self) -> isa::TargetFrontendConfig {
        self.env.target_config()
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

    /// Sets up the necessary preamble definitions in `func` to access the global identified
    /// by `index`.
    ///
    /// The index space covers both imported and locally declared globals.
    fn make_global(
        &mut self,
        func: &mut ir::Function,
        clif_global_index: cranelift_wasm::GlobalIndex,
    ) -> cranelift_wasm::GlobalVariable {
        let global_index: GlobalIndex = Converter(clif_global_index).into();

        // Create VMContext value.
        let vmctx = func.create_global_value(ir::GlobalValueData::VMContext);
        let ptr_type = self.pointer_type();

        let local_global_addr = match global_index.local_or_import(&self.env.module.info) {
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

                func.create_global_value(ir::GlobalValueData::Load {
                    base: local_global_ptr_ptr,
                    offset: 0.into(),
                    global_type: ptr_type,
                    readonly: true,
                })
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

                func.create_global_value(ir::GlobalValueData::Load {
                    base: local_global_ptr_ptr,
                    offset: 0.into(),
                    global_type: ptr_type,
                    readonly: true,
                })
            }
        };

        cranelift_wasm::GlobalVariable::Memory {
            gv: local_global_addr,
            offset: (vm::LocalGlobal::offset_data() as i32).into(),
            ty: self.env.get_global(clif_global_index).ty,
        }
    }

    /// Sets up the necessary preamble definitions in `func` to access the linear memory identified
    /// by `index`.
    ///
    /// The index space covers both imported and locally declared memories.
    fn make_heap(
        &mut self,
        func: &mut ir::Function,
        clif_mem_index: cranelift_wasm::MemoryIndex,
    ) -> ir::Heap {
        let mem_index: MemoryIndex = Converter(clif_mem_index).into();
        // Create VMContext value.
        let vmctx = func.create_global_value(ir::GlobalValueData::VMContext);
        let ptr_type = self.pointer_type();

        let (local_memory_ptr_ptr, description) =
            match mem_index.local_or_import(&self.env.module.info) {
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
                        self.env.module.info.memories[local_mem_index],
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
                        self.env.module.info.imported_memories[import_mem_index].1,
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
            mem_type @ MemoryType::Dynamic => {
                let local_memory_bound = func.create_global_value(ir::GlobalValueData::Load {
                    base: local_memory_ptr,
                    offset: (vm::LocalMemory::offset_bound() as i32).into(),
                    global_type: ptr_type,
                    readonly: false,
                });

                func.create_heap(ir::HeapData {
                    base: local_memory_base,
                    min_size: (description.minimum.bytes().0 as u64).into(),
                    offset_guard_size: mem_type.guard_size().into(),
                    style: ir::HeapStyle::Dynamic {
                        bound_gv: local_memory_bound,
                    },
                    index_type: ir::types::I32,
                })
            }
            mem_type @ MemoryType::Static | mem_type @ MemoryType::SharedStatic => func
                .create_heap(ir::HeapData {
                    base: local_memory_base,
                    min_size: (description.minimum.bytes().0 as u64).into(),
                    offset_guard_size: mem_type.guard_size().into(),
                    style: ir::HeapStyle::Static {
                        bound: mem_type.bounds().unwrap().into(),
                    },
                    index_type: ir::types::I32,
                }),
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
    ) -> ir::Table {
        let table_index: TableIndex = Converter(clif_table_index).into();
        // Create VMContext value.
        let vmctx = func.create_global_value(ir::GlobalValueData::VMContext);
        let ptr_type = self.pointer_type();

        let (table_struct_ptr_ptr, description) = match table_index
            .local_or_import(&self.env.module.info)
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
                    self.env.module.info.tables[local_table_index],
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
                    self.env.module.info.imported_tables[import_table_index].1,
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
            global_type: ptr_type,
            // The table length can change, so it can't be readonly.
            readonly: false,
        });

        func.create_table(ir::TableData {
            base_gv: table_base,
            min_size: (description.minimum as u64).into(),
            bound_gv: table_count,
            element_size: (vm::Anyfunc::size() as u64).into(),
            index_type: ir::types::I32,
        })
    }

    /// Sets up a signature definition in `func`'s preamble.
    ///
    /// Signature may contain additional argument, but arguments marked as ArgumentPurpose::Normal`
    /// must correspond to the arguments in the wasm signature
    fn make_indirect_sig(
        &mut self,
        func: &mut ir::Function,
        clif_sig_index: cranelift_wasm::SignatureIndex,
    ) -> ir::SigRef {
        // Create a signature reference out of specified signature (with VMContext param added).
        func.import_signature(self.generate_signature(clif_sig_index))
    }

    /// Sets up an external function definition in the preamble of `func` that can be used to
    /// directly call the function `index`.
    ///
    /// The index space covers both imported functions and functions defined in the current module.
    fn make_direct_func(
        &mut self,
        func: &mut ir::Function,
        func_index: cranelift_wasm::FuncIndex,
    ) -> ir::FuncRef {
        // Get signature of function.
        let signature_index = self.env.get_func_type(func_index);

        // Create a signature reference from specified signature (with VMContext param added).
        let signature = func.import_signature(self.generate_signature(signature_index));

        // Get name of function.
        let name = ir::ExternalName::user(0, func_index.as_u32());

        // Create function reference from fuction data.
        func.import_function(ir::ExtFuncData {
            name,
            signature,
            // Make this colocated so all calls between local functions are relative.
            colocated: true,
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

            pos.ins().symbol_value(ir::types::I64, sig_index_global)

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
    /// TODO: add support for imported functions
    fn translate_call(
        &mut self,
        mut pos: FuncCursor,
        clif_callee_index: cranelift_wasm::FuncIndex,
        callee: ir::FuncRef,
        call_args: &[ir::Value],
    ) -> cranelift_wasm::WasmResult<ir::Inst> {
        let callee_index: FuncIndex = Converter(clif_callee_index).into();

        match callee_index.local_or_import(&self.env.module.info) {
            LocalOrImport::Local(_) => {
                // this is an internal function
                let vmctx = pos
                    .func
                    .special_param(ir::ArgumentPurpose::VMContext)
                    .expect("missing vmctx parameter");

                let mut args = Vec::with_capacity(call_args.len() + 1);
                args.push(vmctx);
                args.extend(call_args.iter().cloned());

                Ok(pos.ins().call(callee, &args))
            }
            LocalOrImport::Import(imported_func_index) => {
                let ptr_type = self.pointer_type();
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

                let imported_vmctx_addr = pos.func.create_global_value(ir::GlobalValueData::Load {
                    base: imported_func_struct_addr,
                    offset: (vm::ImportedFunc::offset_vmctx() as i32).into(),
                    global_type: ptr_type,
                    readonly: true,
                });

                let imported_func_addr = pos.ins().global_value(ptr_type, imported_func_addr);
                let imported_vmctx_addr = pos.ins().global_value(ptr_type, imported_vmctx_addr);

                let sig_ref = pos.func.dfg.ext_funcs[callee].signature;

                let mut args = Vec::with_capacity(call_args.len() + 1);
                args.push(imported_vmctx_addr);
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
            match mem_index.local_or_import(&self.env.module.info) {
                LocalOrImport::Local(local_mem_index) => (
                    call_names::LOCAL_NAMESPACE,
                    local_mem_index.index(),
                    self.env.module.info.memories[local_mem_index],
                ),
                LocalOrImport::Import(import_mem_index) => (
                    call_names::IMPORT_NAMESPACE,
                    import_mem_index.index(),
                    self.env.module.info.imported_memories[import_mem_index].1,
                ),
            };

        let name_index = match description.memory_type() {
            MemoryType::Dynamic => call_names::DYNAMIC_MEM_GROW,
            MemoryType::Static => call_names::STATIC_MEM_GROW,
            MemoryType::SharedStatic => call_names::SHARED_STATIC_MEM_GROW,
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
            match mem_index.local_or_import(&self.env.module.info) {
                LocalOrImport::Local(local_mem_index) => (
                    call_names::LOCAL_NAMESPACE,
                    local_mem_index.index(),
                    self.env.module.info.memories[local_mem_index],
                ),
                LocalOrImport::Import(import_mem_index) => (
                    call_names::IMPORT_NAMESPACE,
                    import_mem_index.index(),
                    self.env.module.info.imported_memories[import_mem_index].1,
                ),
            };

        let name_index = match description.memory_type() {
            MemoryType::Dynamic => call_names::DYNAMIC_MEM_SIZE,
            MemoryType::Static => call_names::STATIC_MEM_SIZE,
            MemoryType::SharedStatic => call_names::SHARED_STATIC_MEM_SIZE,
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
}
