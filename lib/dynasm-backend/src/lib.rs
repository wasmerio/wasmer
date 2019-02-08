use std::ptr::NonNull;
use std::sync::Arc;
use wasmer_runtime_core::{
    backend::{Backend, Compiler, FuncResolver, ProtectedCaller, Token},
    error::{CompileError, CompileResult, RuntimeResult},
    module::{
        DataInitializer, ExportIndex, ImportName, ModuleInfo, ModuleInner, StringTable,
        TableInitializer,
    },
    structures::{Map, TypedIndex},
    types::{
        ElementType, FuncIndex, FuncSig, GlobalDescriptor, GlobalIndex, GlobalInit,
        ImportedGlobalIndex, Initializer, LocalFuncIndex, MemoryDescriptor, MemoryIndex,
        TableDescriptor, TableIndex, Type as CoreType, Value,
    },
    units::Pages,
    vm::{self, ImportBacking},
};
use wasmparser::{
    self, ExternalKind, FuncType, ImportSectionEntryType, InitExpr, MemoryType, ModuleReader,
    Operator, SectionCode, TableType, Type, WasmDecoder,
};

struct Placeholder;

impl FuncResolver for Placeholder {
    fn get(
        &self,
        _module: &ModuleInner,
        _local_func_index: LocalFuncIndex,
    ) -> Option<NonNull<vm::Func>> {
        None
    }
}

impl ProtectedCaller for Placeholder {
    fn call(
        &self,
        _module: &ModuleInner,
        _func_index: FuncIndex,
        _params: &[Value],
        _import_backing: &ImportBacking,
        _vmctx: *mut vm::Ctx,
        _: Token,
    ) -> RuntimeResult<Vec<Value>> {
        Ok(vec![])
    }
}

pub struct DynasmCompiler {}

impl DynasmCompiler {
    pub fn new() -> DynasmCompiler {
        DynasmCompiler {}
    }
}

impl Compiler for DynasmCompiler {
    fn compile(&self, wasm: &[u8], _: Token) -> CompileResult<ModuleInner> {
        validate(wasm)?;

        let mut reader = ModuleReader::new(wasm)?;
        let mut m = ModuleInner {
            // this is a placeholder
            func_resolver: Box::new(Placeholder),
            protected_caller: Box::new(Placeholder),

            info: ModuleInfo {
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
                backend: Backend::Cranelift,

                namespace_table: StringTable::new(),
                name_table: StringTable::new(),
            },
        };
        let mut types: Vec<FuncType> = Vec::new();

        loop {
            if reader.eof() {
                return Ok(m);
            }
            let section = reader.read()?;
            match section.code {
                SectionCode::Custom { .. } => {}
                SectionCode::Type => {
                    let mut ty_reader = section.get_type_section_reader()?;
                    let count = ty_reader.get_count();
                    for _ in 0..count {
                        types.push(ty_reader.read()?);
                    }
                }
                SectionCode::Import => {
                    let mut imp_reader = section.get_import_section_reader()?;
                    let count = imp_reader.get_count();
                    for _ in 0..count {
                        let imp = imp_reader.read()?;
                        // FIXME: not implemented
                    }
                }
                SectionCode::Function => {
                    let mut func_reader = section.get_function_section_reader()?;
                    let count = func_reader.get_count();
                    for _ in 0..count {
                        let ty_id = func_reader.read()? as usize;
                        m.info.signatures.push(Arc::new(FuncSig::new(
                            types[ty_id]
                                .params
                                .iter()
                                .cloned()
                                .map(CoreType::from_wasmparser_type)
                                .collect::<CompileResult<Vec<CoreType>>>()?,
                            types[ty_id]
                                .returns
                                .iter()
                                .cloned()
                                .map(CoreType::from_wasmparser_type)
                                .collect::<CompileResult<Vec<CoreType>>>()?,
                        )));
                    }
                }
                SectionCode::Table => {
                    let mut table_reader = section.get_table_section_reader()?;
                    let count = table_reader.get_count();
                    for _ in 0..count {
                        let tt = table_reader.read()?;
                        if tt.element_type != Type::AnyFunc {
                            return Err(CompileError::InternalError {
                                msg: "unsupported table element type".into(),
                            });
                        }
                        m.info.tables.push(TableDescriptor {
                            element: ElementType::Anyfunc,
                            minimum: tt.limits.initial,
                            maximum: tt.limits.maximum,
                        });
                    }
                }
                SectionCode::Memory => {
                    let mut mem_reader = section.get_memory_section_reader()?;
                    let count = mem_reader.get_count();
                    for _ in 0..count {
                        let mem_info = mem_reader.read()?;
                        m.info.memories.push(MemoryDescriptor {
                            minimum: Pages(mem_info.limits.initial),
                            maximum: mem_info.limits.maximum.map(Pages),
                            shared: mem_info.shared,
                        });
                    }
                }
                SectionCode::Global => {
                    let mut global_reader = section.get_global_section_reader()?;
                    let count = global_reader.get_count();
                    for _ in 0..count {
                        let info = global_reader.read()?;
                        m.info.globals.push(GlobalInit {
                            desc: GlobalDescriptor {
                                mutable: info.ty.mutable,
                                ty: CoreType::from_wasmparser_type(info.ty.content_type)?,
                            },
                            init: eval_init_expr(&info.init_expr)?,
                        });
                    }
                }
                SectionCode::Export => {
                    let mut export_reader = section.get_export_section_reader()?;
                    let count = export_reader.get_count();
                    for _ in 0..count {
                        let v = export_reader.read()?;
                        m.info.exports.insert(
                            match ::std::str::from_utf8(v.field) {
                                Ok(x) => x.to_string(),
                                Err(_) => {
                                    return Err(CompileError::InternalError {
                                        msg: "field name not in utf-8".into(),
                                    })
                                }
                            },
                            match v.kind {
                                ExternalKind::Function => {
                                    ExportIndex::Func(FuncIndex::new(v.index as usize))
                                }
                                ExternalKind::Global => {
                                    ExportIndex::Global(GlobalIndex::new(v.index as usize))
                                }
                                ExternalKind::Memory => {
                                    ExportIndex::Memory(MemoryIndex::new(v.index as usize))
                                }
                                ExternalKind::Table => {
                                    ExportIndex::Table(TableIndex::new(v.index as usize))
                                }
                            },
                        );
                    }
                }
                SectionCode::Start => {
                    m.info.start_func =
                        Some(FuncIndex::new(section.get_start_section_content()? as usize));
                }
                SectionCode::Element => {
                    let mut element_reader = section.get_element_section_reader()?;
                    let count = element_reader.get_count();
                    for _ in 0..count {
                        let elem = element_reader.read()?;
                        let table_index = elem.table_index as usize;

                        let mut item_reader = elem.items.get_items_reader()?;
                        let item_count = item_reader.get_count() as usize;

                        m.info.elem_initializers.push(TableInitializer {
                            table_index: TableIndex::new(table_index),
                            base: eval_init_expr(&elem.init_expr)?,
                            elements: (0..item_count)
                                .map(|_| Ok(FuncIndex::new(item_reader.read()? as usize)))
                                .collect::<CompileResult<_>>()?,
                        });
                    }
                }
                SectionCode::Code => {
                    let mut code_reader = section.get_code_section_reader()?;
                    let count = code_reader.get_count() as usize;

                    if count != m.info.signatures.len() {
                        return Err(CompileError::InternalError {
                            msg: "len(function_bodies) != len(functions)".into(),
                        });
                    }

                    for i in 0..count {
                        let body = code_reader.read()?;
                        // FIXME: not implemented
                    }
                }
                SectionCode::Data => {
                    let mut data_reader = section.get_data_section_reader()?;
                    let count = data_reader.get_count();
                    for _ in 0..count {
                        let initializer = data_reader.read()?;
                        m.info.data_initializers.push(DataInitializer {
                            memory_index: MemoryIndex::new(initializer.memory_index as usize),
                            base: eval_init_expr(&initializer.init_expr)?,
                            data: initializer.data.to_vec(),
                        });
                    }
                }
            }
        }
    }
}

fn validate(bytes: &[u8]) -> CompileResult<()> {
    let mut parser = wasmparser::ValidatingParser::new(bytes, None);
    loop {
        let state = parser.read();
        match *state {
            wasmparser::ParserState::EndWasm => break Ok(()),
            wasmparser::ParserState::Error(err) => Err(CompileError::ValidationError {
                msg: err.message.to_string(),
            })?,
            _ => {}
        }
    }
}

fn eval_init_expr(expr: &InitExpr) -> CompileResult<Initializer> {
    let mut reader = expr.get_operators_reader();
    let op = reader.read()?;
    Ok(match op {
        Operator::GetGlobal { global_index } => {
            Initializer::GetGlobal(ImportedGlobalIndex::new(global_index as usize))
        }
        Operator::I32Const { value } => Initializer::Const(Value::I32(value)),
        Operator::I64Const { value } => Initializer::Const(Value::I64(value)),
        Operator::F32Const { value } => {
            Initializer::Const(Value::F32(unsafe { ::std::mem::transmute(value.bits()) }))
        }
        Operator::F64Const { value } => {
            Initializer::Const(Value::F64(unsafe { ::std::mem::transmute(value.bits()) }))
        }
        _ => {
            return Err(CompileError::InternalError {
                msg: "init expr evaluation failed: unsupported opcode".into(),
            })
        }
    })
}
