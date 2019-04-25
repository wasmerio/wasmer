use crate::codegen::{CodegenError, FunctionCodeGenerator, ModuleCodeGenerator};
use hashbrown::HashMap;
use wasmer_runtime_core::{
    backend::{Backend, CompilerConfig, RunnableModule},
    module::{
        DataInitializer, ExportIndex, ImportName, ModuleInfo, StringTable, StringTableBuilder,
        TableInitializer,
    },
    structures::{Map, TypedIndex},
    types::{
        ElementType, FuncIndex, FuncSig, GlobalDescriptor, GlobalIndex, GlobalInit,
        ImportedGlobalIndex, Initializer, MemoryDescriptor, MemoryIndex, SigIndex, TableDescriptor,
        TableIndex, Type, Value,
    },
    units::Pages,
};
use wasmparser::{
    BinaryReaderError, ExternalKind, FuncType, ImportSectionEntryType, Operator, Type as WpType,
    WasmDecoder,
};

#[derive(Debug)]
pub enum LoadError {
    Parse(BinaryReaderError),
    Codegen(CodegenError),
}

impl From<BinaryReaderError> for LoadError {
    fn from(other: BinaryReaderError) -> LoadError {
        LoadError::Parse(other)
    }
}

impl From<CodegenError> for LoadError {
    fn from(other: CodegenError) -> LoadError {
        LoadError::Codegen(other)
    }
}

pub fn read_module<
    MCG: ModuleCodeGenerator<FCG, RM>,
    FCG: FunctionCodeGenerator,
    RM: RunnableModule,
>(
    wasm: &[u8],
    backend: Backend,
    mcg: &mut MCG,
    compiler_config: &CompilerConfig,
) -> Result<ModuleInfo, LoadError> {
    let mut info = ModuleInfo {
        memories: Map::new(),
        globals: Map::new(),
        tables: Map::new(),

        imported_functions: Map::new(),
        imported_memories: Map::new(),
        imported_tables: Map::new(),
        imported_globals: Map::new(),

        exports: Default::default(),

        data_initializers: Vec::new(),
        elem_initializers: Vec::new(),

        start_func: None,

        func_assoc: Map::new(),
        signatures: Map::new(),
        backend: backend,

        namespace_table: StringTable::new(),
        name_table: StringTable::new(),

        em_symbol_map: compiler_config.symbol_map.clone(),

        custom_sections: HashMap::new(),
    };

    let mut parser = wasmparser::ValidatingParser::new(
        wasm,
        Some(wasmparser::ValidatingParserConfig {
            operator_config: wasmparser::OperatorValidatorConfig {
                enable_threads: false,
                enable_reference_types: false,
                enable_simd: false,
                enable_bulk_memory: false,
            },
            mutable_global_imports: false,
        }),
    );

    let mut namespace_builder = Some(StringTableBuilder::new());
    let mut name_builder = Some(StringTableBuilder::new());
    let mut func_count: usize = ::std::usize::MAX;

    loop {
        use wasmparser::ParserState;
        let state = parser.read();
        match *state {
            ParserState::EndWasm => break Ok(info),
            ParserState::Error(err) => Err(LoadError::Parse(err))?,
            ParserState::TypeSectionEntry(ref ty) => {
                info.signatures.push(func_type_to_func_sig(ty)?);
            }
            ParserState::ImportSectionEntry { module, field, ty } => {
                let namespace_index = namespace_builder.as_mut().unwrap().register(module);
                let name_index = name_builder.as_mut().unwrap().register(field);
                let import_name = ImportName {
                    namespace_index,
                    name_index,
                };

                match ty {
                    ImportSectionEntryType::Function(sigindex) => {
                        let sigindex = SigIndex::new(sigindex as usize);
                        info.imported_functions.push(import_name);
                        info.func_assoc.push(sigindex);
                        mcg.feed_import_function()?;
                    }
                    ImportSectionEntryType::Table(table_ty) => {
                        assert_eq!(table_ty.element_type, WpType::AnyFunc);
                        let table_desc = TableDescriptor {
                            element: ElementType::Anyfunc,
                            minimum: table_ty.limits.initial,
                            maximum: table_ty.limits.maximum,
                        };

                        info.imported_tables.push((import_name, table_desc));
                    }
                    ImportSectionEntryType::Memory(memory_ty) => {
                        let mem_desc = MemoryDescriptor {
                            minimum: Pages(memory_ty.limits.initial),
                            maximum: memory_ty.limits.maximum.map(|max| Pages(max)),
                            shared: memory_ty.shared,
                        };
                        info.imported_memories.push((import_name, mem_desc));
                    }
                    ImportSectionEntryType::Global(global_ty) => {
                        let global_desc = GlobalDescriptor {
                            mutable: global_ty.mutable,
                            ty: wp_type_to_type(global_ty.content_type)?,
                        };
                        info.imported_globals.push((import_name, global_desc));
                    }
                }
            }
            ParserState::FunctionSectionEntry(sigindex) => {
                let sigindex = SigIndex::new(sigindex as usize);
                info.func_assoc.push(sigindex);
            }
            ParserState::TableSectionEntry(table_ty) => {
                let table_desc = TableDescriptor {
                    element: ElementType::Anyfunc,
                    minimum: table_ty.limits.initial,
                    maximum: table_ty.limits.maximum,
                };

                info.tables.push(table_desc);
            }
            ParserState::MemorySectionEntry(memory_ty) => {
                let mem_desc = MemoryDescriptor {
                    minimum: Pages(memory_ty.limits.initial),
                    maximum: memory_ty.limits.maximum.map(|max| Pages(max)),
                    shared: memory_ty.shared,
                };

                info.memories.push(mem_desc);
            }
            ParserState::ExportSectionEntry { field, kind, index } => {
                let export_index = match kind {
                    ExternalKind::Function => ExportIndex::Func(FuncIndex::new(index as usize)),
                    ExternalKind::Table => ExportIndex::Table(TableIndex::new(index as usize)),
                    ExternalKind::Memory => ExportIndex::Memory(MemoryIndex::new(index as usize)),
                    ExternalKind::Global => ExportIndex::Global(GlobalIndex::new(index as usize)),
                };

                info.exports.insert(field.to_string(), export_index);
            }
            ParserState::StartSectionEntry(start_index) => {
                info.start_func = Some(FuncIndex::new(start_index as usize));
            }
            ParserState::BeginFunctionBody { .. } => {
                let id = func_count.wrapping_add(1);
                func_count = id;
                if func_count == 0 {
                    info.namespace_table = namespace_builder.take().unwrap().finish();
                    info.name_table = name_builder.take().unwrap().finish();
                    mcg.feed_signatures(info.signatures.clone())?;
                    mcg.feed_function_signatures(info.func_assoc.clone())?;
                    mcg.check_precondition(&info)?;
                }

                let fcg = mcg.next_function()?;
                let sig = info
                    .signatures
                    .get(
                        *info
                            .func_assoc
                            .get(FuncIndex::new(id as usize + info.imported_functions.len()))
                            .unwrap(),
                    )
                    .unwrap();
                for ret in sig.returns() {
                    fcg.feed_return(type_to_wp_type(*ret))?;
                }
                for param in sig.params() {
                    fcg.feed_param(type_to_wp_type(*param))?;
                }

                let mut body_begun = false;

                loop {
                    let state = parser.read();
                    match *state {
                        ParserState::Error(err) => return Err(LoadError::Parse(err)),
                        ParserState::FunctionBodyLocals { ref locals } => {
                            for &(count, ty) in locals.iter() {
                                fcg.feed_local(ty, count as usize)?;
                            }
                        }
                        ParserState::CodeOperator(ref op) => {
                            if !body_begun {
                                body_begun = true;
                                fcg.begin_body()?;
                            }
                            fcg.feed_opcode(op, &info)?;
                        }
                        ParserState::EndFunctionBody => break,
                        _ => unreachable!(),
                    }
                }
                fcg.finalize()?;
            }
            ParserState::BeginActiveElementSectionEntry(table_index) => {
                let table_index = TableIndex::new(table_index as usize);
                let mut elements: Option<Vec<FuncIndex>> = None;
                let mut base: Option<Initializer> = None;

                loop {
                    let state = parser.read();
                    match *state {
                        ParserState::Error(err) => return Err(LoadError::Parse(err)),
                        ParserState::InitExpressionOperator(ref op) => {
                            base = Some(eval_init_expr(op)?)
                        }
                        ParserState::ElementSectionEntryBody(ref _elements) => {
                            elements = Some(
                                _elements
                                    .iter()
                                    .cloned()
                                    .map(|index| FuncIndex::new(index as usize))
                                    .collect(),
                            );
                        }
                        ParserState::BeginInitExpressionBody
                        | ParserState::EndInitExpressionBody => {}
                        ParserState::EndElementSectionEntry => break,
                        _ => unreachable!(),
                    }
                }

                let table_init = TableInitializer {
                    table_index,
                    base: base.unwrap(),
                    elements: elements.unwrap(),
                };

                info.elem_initializers.push(table_init);
            }
            ParserState::BeginActiveDataSectionEntry(memory_index) => {
                let memory_index = MemoryIndex::new(memory_index as usize);
                let mut base: Option<Initializer> = None;
                let mut data: Vec<u8> = vec![];

                loop {
                    let state = parser.read();
                    match *state {
                        ParserState::Error(err) => return Err(LoadError::Parse(err)),
                        ParserState::InitExpressionOperator(ref op) => {
                            base = Some(eval_init_expr(op)?)
                        }
                        ParserState::DataSectionEntryBodyChunk(chunk) => {
                            data = chunk.to_vec();
                        }
                        ParserState::BeginInitExpressionBody
                        | ParserState::EndInitExpressionBody => {}
                        ParserState::BeginDataSectionEntryBody(_)
                        | ParserState::EndDataSectionEntryBody => {}
                        ParserState::EndDataSectionEntry => break,
                        _ => unreachable!(),
                    }
                }

                let data_init = DataInitializer {
                    memory_index,
                    base: base.unwrap(),
                    data,
                };
                info.data_initializers.push(data_init);
            }
            ParserState::BeginGlobalSectionEntry(ty) => {
                let init = loop {
                    let state = parser.read();
                    match *state {
                        ParserState::Error(err) => return Err(LoadError::Parse(err)),
                        ParserState::InitExpressionOperator(ref op) => {
                            break eval_init_expr(op)?;
                        }
                        ParserState::BeginInitExpressionBody => {}
                        _ => unreachable!(),
                    }
                };
                let desc = GlobalDescriptor {
                    mutable: ty.mutable,
                    ty: wp_type_to_type(ty.content_type)?,
                };

                let global_init = GlobalInit { desc, init };

                info.globals.push(global_init);
            }

            _ => {}
        }
    }
}

pub fn wp_type_to_type(ty: WpType) -> Result<Type, BinaryReaderError> {
    Ok(match ty {
        WpType::I32 => Type::I32,
        WpType::I64 => Type::I64,
        WpType::F32 => Type::F32,
        WpType::F64 => Type::F64,
        WpType::V128 => {
            return Err(BinaryReaderError {
                message: "the wasmer llvm backend does not yet support the simd extension",
                offset: -1isize as usize,
            });
        }
        _ => panic!("broken invariant, invalid type"),
    })
}

pub fn type_to_wp_type(ty: Type) -> WpType {
    match ty {
        Type::I32 => WpType::I32,
        Type::I64 => WpType::I64,
        Type::F32 => WpType::F32,
        Type::F64 => WpType::F64,
    }
}

fn func_type_to_func_sig(func_ty: &FuncType) -> Result<FuncSig, BinaryReaderError> {
    assert_eq!(func_ty.form, WpType::Func);

    Ok(FuncSig::new(
        func_ty
            .params
            .iter()
            .cloned()
            .map(wp_type_to_type)
            .collect::<Result<Vec<_>, _>>()?,
        func_ty
            .returns
            .iter()
            .cloned()
            .map(wp_type_to_type)
            .collect::<Result<Vec<_>, _>>()?,
    ))
}

fn eval_init_expr(op: &Operator) -> Result<Initializer, BinaryReaderError> {
    Ok(match *op {
        Operator::GetGlobal { global_index } => {
            Initializer::GetGlobal(ImportedGlobalIndex::new(global_index as usize))
        }
        Operator::I32Const { value } => Initializer::Const(Value::I32(value)),
        Operator::I64Const { value } => Initializer::Const(Value::I64(value)),
        Operator::F32Const { value } => {
            Initializer::Const(Value::F32(f32::from_bits(value.bits())))
        }
        Operator::F64Const { value } => {
            Initializer::Const(Value::F64(f64::from_bits(value.bits())))
        }
        _ => {
            return Err(BinaryReaderError {
                message: "init expr evaluation failed: unsupported opcode",
                offset: -1isize as usize,
            });
        }
    })
}
