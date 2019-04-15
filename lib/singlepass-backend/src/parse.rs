use crate::codegen::{CodegenError, FunctionCodeGenerator, ModuleCodeGenerator};
use hashbrown::HashMap;
use wasmer_runtime_core::{
    backend::{Backend, CompilerConfig, FuncResolver, ProtectedCaller},
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
    BinaryReaderError, Data, DataKind, Element, ElementKind, Export, ExternalKind, FuncType,
    Import, ImportSectionEntryType, InitExpr, ModuleReader, Operator, SectionCode, Type as WpType,
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

fn validate(bytes: &[u8]) -> Result<(), LoadError> {
    let mut parser = wasmparser::ValidatingParser::new(
        bytes,
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

    loop {
        let state = parser.read();
        match *state {
            wasmparser::ParserState::EndWasm => break Ok(()),
            wasmparser::ParserState::Error(err) => Err(LoadError::Parse(err))?,
            _ => {}
        }
    }
}

pub fn read_module<
    MCG: ModuleCodeGenerator<FCG, PC, FR>,
    FCG: FunctionCodeGenerator,
    PC: ProtectedCaller,
    FR: FuncResolver,
>(
    wasm: &[u8],
    backend: Backend,
    mcg: &mut MCG,
    compiler_config: &CompilerConfig,
) -> Result<ModuleInfo, LoadError> {
    validate(wasm)?;
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

    let mut reader = ModuleReader::new(wasm)?;

    loop {
        if reader.eof() {
            return Ok(info);
        }

        let section = reader.read()?;

        match section.code {
            SectionCode::Type => {
                let type_reader = section.get_type_section_reader()?;

                for ty in type_reader {
                    let ty = ty?;
                    info.signatures.push(func_type_to_func_sig(ty)?);
                }

                mcg.feed_signatures(info.signatures.clone())?;
            }
            SectionCode::Import => {
                let import_reader = section.get_import_section_reader()?;
                let mut namespace_builder = StringTableBuilder::new();
                let mut name_builder = StringTableBuilder::new();

                for import in import_reader {
                    let Import { module, field, ty } = import?;

                    let namespace_index = namespace_builder.register(module);
                    let name_index = name_builder.register(field);
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

                info.namespace_table = namespace_builder.finish();
                info.name_table = name_builder.finish();
            }
            SectionCode::Function => {
                let func_decl_reader = section.get_function_section_reader()?;

                for sigindex in func_decl_reader {
                    let sigindex = sigindex?;

                    let sigindex = SigIndex::new(sigindex as usize);
                    info.func_assoc.push(sigindex);
                }

                mcg.feed_function_signatures(info.func_assoc.clone())?;
            }
            SectionCode::Table => {
                let table_decl_reader = section.get_table_section_reader()?;

                for table_ty in table_decl_reader {
                    let table_ty = table_ty?;

                    let table_desc = TableDescriptor {
                        element: ElementType::Anyfunc,
                        minimum: table_ty.limits.initial,
                        maximum: table_ty.limits.maximum,
                    };

                    info.tables.push(table_desc);
                }
            }
            SectionCode::Memory => {
                let mem_decl_reader = section.get_memory_section_reader()?;

                for memory_ty in mem_decl_reader {
                    let memory_ty = memory_ty?;

                    let mem_desc = MemoryDescriptor {
                        minimum: Pages(memory_ty.limits.initial),
                        maximum: memory_ty.limits.maximum.map(|max| Pages(max)),
                        shared: memory_ty.shared,
                    };

                    info.memories.push(mem_desc);
                }
            }
            SectionCode::Global => {
                let global_decl_reader = section.get_global_section_reader()?;

                for global in global_decl_reader {
                    let global = global?;

                    let desc = GlobalDescriptor {
                        mutable: global.ty.mutable,
                        ty: wp_type_to_type(global.ty.content_type)?,
                    };

                    let global_init = GlobalInit {
                        desc,
                        init: eval_init_expr(&global.init_expr)?,
                    };

                    info.globals.push(global_init);
                }
            }
            SectionCode::Export => {
                let export_reader = section.get_export_section_reader()?;

                for export in export_reader {
                    let Export { field, kind, index } = export?;

                    let export_index = match kind {
                        ExternalKind::Function => ExportIndex::Func(FuncIndex::new(index as usize)),
                        ExternalKind::Table => ExportIndex::Table(TableIndex::new(index as usize)),
                        ExternalKind::Memory => {
                            ExportIndex::Memory(MemoryIndex::new(index as usize))
                        }
                        ExternalKind::Global => {
                            ExportIndex::Global(GlobalIndex::new(index as usize))
                        }
                    };

                    info.exports.insert(field.to_string(), export_index);
                }
            }
            SectionCode::Start => {
                let start_index = section.get_start_section_content()?;

                info.start_func = Some(FuncIndex::new(start_index as usize));
            }
            SectionCode::Element => {
                let element_reader = section.get_element_section_reader()?;

                for element in element_reader {
                    let Element { kind, items } = element?;

                    match kind {
                        ElementKind::Active {
                            table_index,
                            init_expr,
                        } => {
                            let table_index = TableIndex::new(table_index as usize);
                            let base = eval_init_expr(&init_expr)?;
                            let items_reader = items.get_items_reader()?;

                            let elements: Vec<_> = items_reader
                                .into_iter()
                                .map(|res| res.map(|index| FuncIndex::new(index as usize)))
                                .collect::<Result<_, _>>()?;

                            let table_init = TableInitializer {
                                table_index,
                                base,
                                elements,
                            };

                            info.elem_initializers.push(table_init);
                        }
                        ElementKind::Passive(_ty) => {
                            return Err(BinaryReaderError {
                                message: "passive tables are not yet supported",
                                offset: -1isize as usize,
                            }
                            .into());
                        }
                    }
                }
            }
            SectionCode::Code => {
                let mut code_reader = section.get_code_section_reader()?;
                if code_reader.get_count() as usize > info.func_assoc.len() {
                    return Err(BinaryReaderError {
                        message: "code_reader.get_count() > info.func_assoc.len()",
                        offset: ::std::usize::MAX,
                    }
                    .into());
                }
                mcg.check_precondition(&info)?;
                for i in 0..code_reader.get_count() {
                    let item = code_reader.read()?;
                    let fcg = mcg.next_function()?;
                    let sig = info
                        .signatures
                        .get(
                            *info
                                .func_assoc
                                .get(FuncIndex::new(i as usize + info.imported_functions.len()))
                                .unwrap(),
                        )
                        .unwrap();
                    for ret in sig.returns() {
                        fcg.feed_return(type_to_wp_type(*ret))?;
                    }
                    for param in sig.params() {
                        fcg.feed_param(type_to_wp_type(*param))?;
                    }
                    for local in item.get_locals_reader()? {
                        let (count, ty) = local?;
                        fcg.feed_local(ty, count as usize)?;
                    }
                    fcg.begin_body()?;
                    for op in item.get_operators_reader()? {
                        let op = op?;
                        fcg.feed_opcode(op, &info)?;
                    }
                    fcg.finalize()?;
                }
            }
            SectionCode::Data => {
                let data_reader = section.get_data_section_reader()?;

                for data in data_reader {
                    let Data { kind, data } = data?;

                    match kind {
                        DataKind::Active {
                            memory_index,
                            init_expr,
                        } => {
                            let memory_index = MemoryIndex::new(memory_index as usize);
                            let base = eval_init_expr(&init_expr)?;

                            let data_init = DataInitializer {
                                memory_index,
                                base,
                                data: data.to_vec(),
                            };

                            info.data_initializers.push(data_init);
                        }
                        DataKind::Passive => {
                            return Err(BinaryReaderError {
                                message: "passive memories are not yet supported",
                                offset: -1isize as usize,
                            }
                            .into());
                        }
                    }
                }
            }
            SectionCode::DataCount => {}
            SectionCode::Custom { .. } => {}
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

fn func_type_to_func_sig(func_ty: FuncType) -> Result<FuncSig, BinaryReaderError> {
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

fn eval_init_expr(expr: &InitExpr) -> Result<Initializer, BinaryReaderError> {
    let mut reader = expr.get_operators_reader();
    let (op, offset) = reader.read_with_offset()?;
    Ok(match op {
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
                offset,
            });
        }
    })
}
