//! The parse module contains common data structures and functions using to parse wasm files into
//! runtime data structures.

use crate::codegen::*;
use crate::{
    backend::{CompilerConfig, RunnableModule},
    error::CompileError,
    module::{
        DataInitializer, ExportIndex, ImportName, ModuleInfo, NameIndex, NamespaceIndex,
        StringTable, StringTableBuilder, TableInitializer,
    },
    structures::{Map, TypedIndex},
    types::{
        ElementType, FuncIndex, FuncSig, GlobalIndex, GlobalInit, GlobalType, ImportedGlobalIndex,
        Initializer, MemoryIndex, MemoryType, SigIndex, TableIndex, TableType, Type, Value,
    },
    units::Pages,
};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::{Arc, RwLock};
use wasmparser::{
    BinaryReaderError, ElemSectionEntryTable, ElementItem, ExternalKind, FuncType,
    ImportSectionEntryType, Operator, Type as WpType, WasmDecoder,
};

/// Kind of load error.
#[derive(Debug)]
pub enum LoadError {
    /// Parse error.
    Parse(String),
    /// Code generation error.
    Codegen(String),
}

impl From<LoadError> for CompileError {
    fn from(other: LoadError) -> CompileError {
        CompileError::InternalError {
            msg: format!("{:?}", other),
        }
    }
}

impl From<BinaryReaderError> for LoadError {
    fn from(other: BinaryReaderError) -> LoadError {
        LoadError::Parse(format!("{:?}", other))
    }
}

impl From<&BinaryReaderError> for LoadError {
    fn from(other: &BinaryReaderError) -> LoadError {
        LoadError::Parse(format!("{:?}", other))
    }
}

/// Read wasm binary into module data using the given backend, module code generator, middlewares,
/// and compiler configuration.
pub fn read_module<
    MCG: ModuleCodeGenerator<FCG, RM, E>,
    FCG: FunctionCodeGenerator<E>,
    RM: RunnableModule,
    E: Debug,
>(
    wasm: &[u8],
    mcg: &mut MCG,
    middlewares: &mut MiddlewareChain,
    compiler_config: &CompilerConfig,
) -> Result<Arc<RwLock<ModuleInfo>>, LoadError> {
    mcg.feed_compiler_config(compiler_config)
        .map_err(|x| LoadError::Codegen(format!("{:?}", x)))?;
    let info = Arc::new(RwLock::new(ModuleInfo {
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
        backend: MCG::backend_id().to_string(),

        namespace_table: StringTable::new(),
        name_table: StringTable::new(),

        em_symbol_map: compiler_config.symbol_map.clone(),

        custom_sections: HashMap::new(),

        generate_debug_info: compiler_config.should_generate_debug_info(),
        #[cfg(feature = "generate-debug-information")]
        debug_info_manager: crate::jit_debug::JitCodeDebugInfoManager::new(),
    }));

    let mut parser = wasmparser::ValidatingParser::new(
        wasm,
        Some(validating_parser_config(&compiler_config.features)),
    );

    let mut namespace_builder = Some(StringTableBuilder::new());
    let mut name_builder = Some(StringTableBuilder::new());
    let mut func_count: usize = 0;

    let mut feed_mcg_signatures: Option<_> = Some(|mcg: &mut MCG| -> Result<(), LoadError> {
        let info_read = info.read().unwrap();
        mcg.feed_signatures(info_read.signatures.clone())
            .map_err(|x| LoadError::Codegen(format!("{:?}", x)))?;
        Ok(())
    });
    let mut feed_mcg_info: Option<_> = Some(
        |mcg: &mut MCG,
         ns_builder: StringTableBuilder<NamespaceIndex>,
         name_builder: StringTableBuilder<NameIndex>|
         -> Result<(), LoadError> {
            {
                let mut info_write = info.write().unwrap();
                info_write.namespace_table = ns_builder.finish();
                info_write.name_table = name_builder.finish();
            }
            let info_read = info.read().unwrap();
            mcg.feed_function_signatures(info_read.func_assoc.clone())
                .map_err(|x| LoadError::Codegen(format!("{:?}", x)))?;
            mcg.check_precondition(&info_read)
                .map_err(|x| LoadError::Codegen(format!("{:?}", x)))?;
            Ok(())
        },
    );

    loop {
        use wasmparser::ParserState;
        let state = parser.read();

        match *state {
            ParserState::Error(ref err) => return Err(err.clone().into()),
            ParserState::TypeSectionEntry(ref ty) => {
                info.write()
                    .unwrap()
                    .signatures
                    .push(func_type_to_func_sig(ty)?);
            }
            ParserState::ImportSectionEntry { module, field, ty } => {
                if let Some(f) = feed_mcg_signatures.take() {
                    f(mcg)?;
                }

                let namespace_index = namespace_builder.as_mut().unwrap().register(module);
                let name_index = name_builder.as_mut().unwrap().register(field);
                let import_name = ImportName {
                    namespace_index,
                    name_index,
                };

                match ty {
                    ImportSectionEntryType::Function(sigindex) => {
                        let sigindex = SigIndex::new(sigindex as usize);
                        info.write().unwrap().imported_functions.push(import_name);
                        info.write().unwrap().func_assoc.push(sigindex);
                        mcg.feed_import_function(sigindex)
                            .map_err(|x| LoadError::Codegen(format!("{:?}", x)))?;
                    }
                    ImportSectionEntryType::Table(table_ty) => {
                        assert_eq!(table_ty.element_type, WpType::AnyFunc);
                        let table_desc = TableType {
                            element: ElementType::Anyfunc,
                            minimum: table_ty.limits.initial,
                            maximum: table_ty.limits.maximum,
                        };

                        info.write()
                            .unwrap()
                            .imported_tables
                            .push((import_name, table_desc));
                    }
                    ImportSectionEntryType::Memory(memory_ty) => {
                        let mem_desc = MemoryType::new(
                            Pages(memory_ty.limits.initial),
                            memory_ty.limits.maximum.map(Pages),
                            memory_ty.shared,
                        )
                        .map_err(|x| LoadError::Codegen(format!("{:?}", x)))?;

                        info.write()
                            .unwrap()
                            .imported_memories
                            .push((import_name, mem_desc));
                    }
                    ImportSectionEntryType::Global(global_ty) => {
                        let global_desc = GlobalType {
                            mutable: global_ty.mutable,
                            ty: wp_type_to_type(global_ty.content_type)?,
                        };
                        info.write()
                            .unwrap()
                            .imported_globals
                            .push((import_name, global_desc));
                    }
                }
            }
            ParserState::FunctionSectionEntry(sigindex) => {
                let sigindex = SigIndex::new(sigindex as usize);
                info.write().unwrap().func_assoc.push(sigindex);
            }
            ParserState::TableSectionEntry(table_ty) => {
                let table_desc = TableType {
                    element: ElementType::Anyfunc,
                    minimum: table_ty.limits.initial,
                    maximum: table_ty.limits.maximum,
                };

                info.write().unwrap().tables.push(table_desc);
            }
            ParserState::MemorySectionEntry(memory_ty) => {
                let mem_desc = MemoryType::new(
                    Pages(memory_ty.limits.initial),
                    memory_ty.limits.maximum.map(Pages),
                    memory_ty.shared,
                )
                .map_err(|x| LoadError::Codegen(format!("{:?}", x)))?;

                info.write().unwrap().memories.push(mem_desc);
            }
            ParserState::ExportSectionEntry { field, kind, index } => {
                let export_index = match kind {
                    ExternalKind::Function => ExportIndex::Func(FuncIndex::new(index as usize)),
                    ExternalKind::Table => ExportIndex::Table(TableIndex::new(index as usize)),
                    ExternalKind::Memory => ExportIndex::Memory(MemoryIndex::new(index as usize)),
                    ExternalKind::Global => ExportIndex::Global(GlobalIndex::new(index as usize)),
                };

                info.write()
                    .unwrap()
                    .exports
                    .insert(field.to_string(), export_index);
            }
            ParserState::StartSectionEntry(start_index) => {
                info.write().unwrap().start_func = Some(FuncIndex::new(start_index as usize));
            }
            ParserState::BeginFunctionBody { range } => {
                if let Some(f) = feed_mcg_signatures.take() {
                    f(mcg)?;
                }
                if let Some(f) = feed_mcg_info.take() {
                    f(
                        mcg,
                        namespace_builder.take().unwrap(),
                        name_builder.take().unwrap(),
                    )?;
                }
                let id = func_count;
                let fcg = mcg
                    .next_function(
                        Arc::clone(&info),
                        WasmSpan::new(range.start as u32, range.end as u32),
                    )
                    .map_err(|x| LoadError::Codegen(format!("{:?}", x)))?;

                {
                    let info_read = info.read().unwrap();
                    let sig = info_read
                        .signatures
                        .get(
                            *info
                                .read()
                                .unwrap()
                                .func_assoc
                                .get(FuncIndex::new(
                                    id as usize + info_read.imported_functions.len(),
                                ))
                                .unwrap(),
                        )
                        .unwrap();
                    for ret in sig.returns() {
                        fcg.feed_return(type_to_wp_type(*ret))
                            .map_err(|x| LoadError::Codegen(format!("{:?}", x)))?;
                    }
                    for param in sig.params() {
                        fcg.feed_param(type_to_wp_type(*param))
                            .map_err(|x| LoadError::Codegen(format!("{:?}", x)))?;
                    }
                }

                let info_read = info.read().unwrap();
                let mut cur_pos = parser.current_position() as u32;
                let mut state = parser.read();
                // loop until the function body starts
                loop {
                    match state {
                        ParserState::Error(err) => return Err(err.into()),
                        ParserState::FunctionBodyLocals { ref locals } => {
                            for &(count, ty) in locals.iter() {
                                fcg.feed_local(ty, count as usize, cur_pos)
                                    .map_err(|x| LoadError::Codegen(format!("{:?}", x)))?;
                            }
                        }
                        ParserState::CodeOperator(_) => {
                            // the body of the function has started
                            fcg.begin_body(&info_read)
                                .map_err(|x| LoadError::Codegen(format!("{:?}", x)))?;
                            middlewares
                                .run(
                                    Some(fcg),
                                    Event::Internal(InternalEvent::FunctionBegin(id as u32)),
                                    &info_read,
                                    cur_pos,
                                )
                                .map_err(LoadError::Codegen)?;
                            // go to other loop
                            break;
                        }
                        ParserState::EndFunctionBody => break,
                        _ => unreachable!(),
                    }
                    cur_pos = parser.current_position() as u32;
                    state = parser.read();
                }

                // loop until the function body ends
                loop {
                    match state {
                        ParserState::Error(err) => return Err(err.into()),
                        ParserState::CodeOperator(op) => {
                            middlewares
                                .run(Some(fcg), Event::Wasm(op), &info_read, cur_pos)
                                .map_err(LoadError::Codegen)?;
                        }
                        ParserState::EndFunctionBody => break,
                        _ => unreachable!(),
                    }
                    cur_pos = parser.current_position() as u32;
                    state = parser.read();
                }
                middlewares
                    .run(
                        Some(fcg),
                        Event::Internal(InternalEvent::FunctionEnd),
                        &info_read,
                        cur_pos,
                    )
                    .map_err(LoadError::Codegen)?;

                fcg.finalize()
                    .map_err(|x| LoadError::Codegen(format!("{:?}", x)))?;
                func_count = func_count.wrapping_add(1);
            }
            ParserState::BeginElementSectionEntry {
                table: ElemSectionEntryTable::Active(table_index_raw),
                ty: WpType::AnyFunc,
            } => {
                let table_index = TableIndex::new(table_index_raw as usize);
                let mut elements: Option<Vec<FuncIndex>> = None;
                let mut base: Option<Initializer> = None;

                loop {
                    let state = parser.read();
                    match *state {
                        ParserState::Error(ref err) => return Err(err.into()),
                        ParserState::InitExpressionOperator(ref op) => {
                            base = Some(eval_init_expr(op)?)
                        }
                        ParserState::ElementSectionEntryBody(ref _elements) => {
                            elements = Some(
                                _elements
                                    .iter()
                                    .map(|elem_idx| match elem_idx {
                                        ElementItem::Null => Err(LoadError::Parse(format!("Error at table {}: null entries in tables are not yet supported", table_index_raw))),
                                        ElementItem::Func(idx) => Ok(FuncIndex::new(*idx as usize)),
                                    })
                                    .collect::<Result<Vec<FuncIndex>, LoadError>>()?,
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

                info.write().unwrap().elem_initializers.push(table_init);
            }
            ParserState::BeginElementSectionEntry {
                table: ElemSectionEntryTable::Active(table_index),
                ty,
            } => {
                return Err(LoadError::Parse(format!(
                    "Error at table {}: type \"{:?}\" is not supported in tables yet",
                    table_index, ty
                )));
            }
            ParserState::BeginActiveDataSectionEntry(memory_index) => {
                let memory_index = MemoryIndex::new(memory_index as usize);
                let mut base: Option<Initializer> = None;
                let mut data: Vec<u8> = vec![];

                loop {
                    let state = parser.read();
                    match *state {
                        ParserState::Error(ref err) => return Err(err.into()),
                        ParserState::InitExpressionOperator(ref op) => {
                            base = Some(eval_init_expr(op)?)
                        }
                        ParserState::DataSectionEntryBodyChunk(chunk) => {
                            data.extend_from_slice(chunk);
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
                info.write().unwrap().data_initializers.push(data_init);
            }
            ParserState::BeginGlobalSectionEntry(ty) => {
                let init = loop {
                    let state = parser.read();
                    match *state {
                        ParserState::Error(ref err) => return Err(err.into()),
                        ParserState::InitExpressionOperator(ref op) => {
                            break eval_init_expr(op)?;
                        }
                        ParserState::BeginInitExpressionBody => {}
                        _ => unreachable!(),
                    }
                };
                let desc = GlobalType {
                    mutable: ty.mutable,
                    ty: wp_type_to_type(ty.content_type)?,
                };

                let global_init = GlobalInit { desc, init };

                info.write().unwrap().globals.push(global_init);
            }
            ParserState::EndWasm => {
                if let Some(f) = feed_mcg_signatures.take() {
                    f(mcg)?;
                }
                if let Some(f) = feed_mcg_info.take() {
                    f(
                        mcg,
                        namespace_builder.take().unwrap(),
                        name_builder.take().unwrap(),
                    )?;
                }
                break;
            }
            _ => {}
        }
    }
    Ok(info)
}

/// Convert given `WpType` to `Type`.
pub fn wp_type_to_type(ty: WpType) -> Result<Type, LoadError> {
    match ty {
        WpType::I32 => Ok(Type::I32),
        WpType::I64 => Ok(Type::I64),
        WpType::F32 => Ok(Type::F32),
        WpType::F64 => Ok(Type::F64),
        WpType::V128 => Ok(Type::V128),
        _ => {
            return Err(LoadError::Parse(
                "broken invariant, invalid type".to_string(),
            ));
        }
    }
}

/// Convert given `Type` to `WpType`.
pub fn type_to_wp_type(ty: Type) -> WpType {
    match ty {
        Type::I32 => WpType::I32,
        Type::I64 => WpType::I64,
        Type::F32 => WpType::F32,
        Type::F64 => WpType::F64,
        Type::V128 => WpType::V128,
    }
}

fn func_type_to_func_sig(func_ty: &FuncType) -> Result<FuncSig, LoadError> {
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

fn eval_init_expr(op: &Operator) -> Result<Initializer, LoadError> {
    Ok(match *op {
        Operator::GlobalGet { global_index } => {
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
        Operator::V128Const { value } => {
            Initializer::Const(Value::V128(u128::from_le_bytes(*value.bytes())))
        }
        _ => {
            return Err(LoadError::Parse(
                "init expr evaluation failed: unsupported opcode".to_string(),
            ));
        }
    })
}
