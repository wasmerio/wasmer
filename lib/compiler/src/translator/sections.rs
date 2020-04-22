//! Helper functions to gather information for each of the non-function sections of a
//! WebAssembly module.
//!
//! The code of these helper functions is straightforward since they only read metadata
//! about linear memories, tables, globals, etc. and store them for later use.
//!
//! The special case of the initialize expressions for table elements offsets or global variables
//! is handled, according to the semantics of WebAssembly, to only specific expressions that are
//! interpreted on the fly.
use super::environ::ModuleEnvironment;
use super::errors::{to_wasm_error, WasmError, WasmResult};
use super::state::ModuleTranslationState;
use crate::{wasm_unsupported, HashMap};
use core::convert::TryFrom;
use std::boxed::Box;
use std::vec::Vec;
use wasm_common::entity::packed_option::ReservedValue;
use wasm_common::entity::EntityRef;
use wasm_common::{
    DataIndex, ElemIndex, FuncIndex, FuncType, GlobalIndex, GlobalInit, GlobalType, MemoryIndex,
    MemoryType, SignatureIndex, TableIndex, TableType, Type, V128,
};
use wasmparser::{
    self, CodeSectionReader, Data, DataKind, DataSectionReader, Element, ElementItem, ElementItems,
    ElementKind, ElementSectionReader, Export, ExportSectionReader, ExternalKind,
    FuncType as WPFuncType, FunctionSectionReader, GlobalSectionReader, GlobalType as WPGlobalType,
    ImportSectionEntryType, ImportSectionReader, MemorySectionReader, MemoryType as WPMemoryType,
    NameSectionReader, Naming, NamingReader, Operator, TableSectionReader, TypeSectionReader,
};

/// Helper function translating wasmparser types to Wasm Type.
pub fn wptype_to_type(ty: wasmparser::Type) -> WasmResult<Type> {
    match ty {
        wasmparser::Type::I32 => Ok(Type::I32),
        wasmparser::Type::I64 => Ok(Type::I64),
        wasmparser::Type::F32 => Ok(Type::F32),
        wasmparser::Type::F64 => Ok(Type::F64),
        wasmparser::Type::V128 => Ok(Type::V128),
        wasmparser::Type::AnyRef => Ok(Type::AnyRef),
        wasmparser::Type::AnyFunc => Ok(Type::FuncRef),
        ty => Err(wasm_unsupported!(
            "wptype_to_irtype: parser wasm type {:?}",
            ty
        )),
    }
}

/// Parses the Type section of the wasm module.
pub fn parse_type_section(
    types: TypeSectionReader,
    module_translation_state: &mut ModuleTranslationState,
    environ: &mut ModuleEnvironment,
) -> WasmResult<()> {
    let count = types.get_count();
    environ.reserve_signatures(count)?;

    for entry in types {
        match entry.map_err(to_wasm_error)? {
            WPFuncType {
                form: wasmparser::Type::Func,
                params,
                returns,
            } => {
                let sig_params: Vec<Type> = params
                    .iter()
                    .map(|ty| {
                        wptype_to_type(*ty)
                            .expect("only numeric types are supported in function signatures")
                    })
                    .collect();
                let sig_returns: Vec<Type> = returns
                    .iter()
                    .map(|ty| {
                        wptype_to_type(*ty)
                            .expect("only numeric types are supported in function signatures")
                    })
                    .collect();
                let sig = FuncType::new(sig_params, sig_returns);
                environ.declare_signature(sig)?;
                module_translation_state.wasm_types.push((params, returns));
            }
            ty => {
                return Err(wasm_unsupported!(
                    "unsupported type in type section: {:?}",
                    ty
                ))
            }
        }
    }
    Ok(())
}

/// Parses the Import section of the wasm module.
pub fn parse_import_section<'data>(
    imports: ImportSectionReader<'data>,
    environ: &mut ModuleEnvironment<'data>,
) -> WasmResult<()> {
    environ.reserve_imports(imports.get_count())?;

    for entry in imports {
        let import = entry.map_err(to_wasm_error)?;
        let module_name = import.module;
        let field_name = import.field;

        match import.ty {
            ImportSectionEntryType::Function(sig) => {
                environ.declare_func_import(
                    SignatureIndex::from_u32(sig),
                    module_name,
                    field_name,
                )?;
            }
            ImportSectionEntryType::Memory(WPMemoryType {
                limits: ref memlimits,
                shared,
            }) => {
                environ.declare_memory_import(
                    MemoryType {
                        minimum: memlimits.initial,
                        maximum: memlimits.maximum,
                        shared,
                    },
                    module_name,
                    field_name,
                )?;
            }
            ImportSectionEntryType::Global(ref ty) => {
                environ.declare_global_import(
                    GlobalType {
                        ty: wptype_to_type(ty.content_type).unwrap(),
                        mutability: ty.mutable.into(),
                        initializer: GlobalInit::Import,
                    },
                    module_name,
                    field_name,
                )?;
            }
            ImportSectionEntryType::Table(ref tab) => {
                environ.declare_table_import(
                    TableType {
                        ty: wptype_to_type(tab.element_type).unwrap(),
                        minimum: tab.limits.initial,
                        maximum: tab.limits.maximum,
                    },
                    module_name,
                    field_name,
                )?;
            }
        }
    }

    environ.finish_imports()?;
    Ok(())
}

/// Parses the Function section of the wasm module.
pub fn parse_function_section(
    functions: FunctionSectionReader,
    environ: &mut ModuleEnvironment,
) -> WasmResult<()> {
    let num_functions = functions.get_count();
    if num_functions == std::u32::MAX {
        // We reserve `u32::MAX` for our own use.
        return Err(WasmError::ImplLimitExceeded);
    }

    environ.reserve_func_types(num_functions)?;

    for entry in functions {
        let sigindex = entry.map_err(to_wasm_error)?;
        environ.declare_func_type(SignatureIndex::from_u32(sigindex))?;
    }

    Ok(())
}

/// Parses the Table section of the wasm module.
pub fn parse_table_section(
    tables: TableSectionReader,
    environ: &mut ModuleEnvironment,
) -> WasmResult<()> {
    environ.reserve_tables(tables.get_count())?;

    for entry in tables {
        let table = entry.map_err(to_wasm_error)?;
        environ.declare_table(TableType {
            ty: wptype_to_type(table.element_type).unwrap(),
            minimum: table.limits.initial,
            maximum: table.limits.maximum,
        })?;
    }

    Ok(())
}

/// Parses the Memory section of the wasm module.
pub fn parse_memory_section(
    memories: MemorySectionReader,
    environ: &mut ModuleEnvironment,
) -> WasmResult<()> {
    environ.reserve_memories(memories.get_count())?;

    for entry in memories {
        let memory = entry.map_err(to_wasm_error)?;
        environ.declare_memory(MemoryType {
            minimum: memory.limits.initial,
            maximum: memory.limits.maximum,
            shared: memory.shared,
        })?;
    }

    Ok(())
}

/// Parses the Global section of the wasm module.
pub fn parse_global_section(
    globals: GlobalSectionReader,
    environ: &mut ModuleEnvironment,
) -> WasmResult<()> {
    environ.reserve_globals(globals.get_count())?;

    for entry in globals {
        let wasmparser::Global {
            ty: WPGlobalType {
                content_type,
                mutable,
            },
            init_expr,
        } = entry.map_err(to_wasm_error)?;
        let mut init_expr_reader = init_expr.get_binary_reader();
        let initializer = match init_expr_reader.read_operator().map_err(to_wasm_error)? {
            Operator::I32Const { value } => GlobalInit::I32Const(value),
            Operator::I64Const { value } => GlobalInit::I64Const(value),
            Operator::F32Const { value } => GlobalInit::F32Const(value.bits()),
            Operator::F64Const { value } => GlobalInit::F64Const(value.bits()),
            Operator::V128Const { value } => {
                GlobalInit::V128Const(V128::from(value.bytes().to_vec().as_slice()))
            }
            Operator::RefNull => GlobalInit::RefNullConst,
            Operator::RefFunc { function_index } => {
                GlobalInit::RefFunc(FuncIndex::from_u32(function_index))
            }
            Operator::GlobalGet { global_index } => {
                GlobalInit::GetGlobal(GlobalIndex::from_u32(global_index))
            }
            ref s => {
                return Err(wasm_unsupported!(
                    "unsupported init expr in global section: {:?}",
                    s
                ));
            }
        };
        let global = GlobalType {
            ty: wptype_to_type(content_type).unwrap(),
            mutability: mutable.into(),
            initializer,
        };
        environ.declare_global(global)?;
    }

    Ok(())
}

/// Parses the Export section of the wasm module.
pub fn parse_export_section<'data>(
    exports: ExportSectionReader<'data>,
    environ: &mut ModuleEnvironment<'data>,
) -> WasmResult<()> {
    environ.reserve_exports(exports.get_count())?;

    for entry in exports {
        let Export {
            field,
            ref kind,
            index,
        } = entry.map_err(to_wasm_error)?;

        // The input has already been validated, so we should be able to
        // assume valid UTF-8 and use `from_utf8_unchecked` if performance
        // becomes a concern here.
        let index = index as usize;
        match *kind {
            ExternalKind::Function => environ.declare_func_export(FuncIndex::new(index), field)?,
            ExternalKind::Table => environ.declare_table_export(TableIndex::new(index), field)?,
            ExternalKind::Memory => {
                environ.declare_memory_export(MemoryIndex::new(index), field)?
            }
            ExternalKind::Global => {
                environ.declare_global_export(GlobalIndex::new(index), field)?
            }
        }
    }

    environ.finish_exports()?;
    Ok(())
}

/// Parses the Start section of the wasm module.
pub fn parse_start_section(index: u32, environ: &mut ModuleEnvironment) -> WasmResult<()> {
    environ.declare_start_func(FuncIndex::from_u32(index))?;
    Ok(())
}

fn read_elems(items: &ElementItems) -> WasmResult<Box<[FuncIndex]>> {
    let items_reader = items.get_items_reader().map_err(to_wasm_error)?;
    let mut elems = Vec::with_capacity(usize::try_from(items_reader.get_count()).unwrap());
    for item in items_reader {
        let elem = match item.map_err(to_wasm_error)? {
            ElementItem::Null => FuncIndex::reserved_value(),
            ElementItem::Func(index) => FuncIndex::from_u32(index),
        };
        elems.push(elem);
    }
    Ok(elems.into_boxed_slice())
}

/// Parses the Element section of the wasm module.
pub fn parse_element_section<'data>(
    elements: ElementSectionReader<'data>,
    environ: &mut ModuleEnvironment,
) -> WasmResult<()> {
    environ.reserve_table_elements(elements.get_count())?;

    for (index, entry) in elements.into_iter().enumerate() {
        let Element { kind, items, ty } = entry.map_err(to_wasm_error)?;
        if ty != wasmparser::Type::AnyFunc {
            return Err(wasm_unsupported!(
                "unsupported table element type: {:?}",
                ty
            ));
        }
        let segments = read_elems(&items)?;
        match kind {
            ElementKind::Active {
                table_index,
                init_expr,
            } => {
                let mut init_expr_reader = init_expr.get_binary_reader();
                let (base, offset) =
                    match init_expr_reader.read_operator().map_err(to_wasm_error)? {
                        Operator::I32Const { value } => (None, value as u32 as usize),
                        Operator::GlobalGet { global_index } => {
                            (Some(GlobalIndex::from_u32(global_index)), 0)
                        }
                        ref s => {
                            return Err(wasm_unsupported!(
                                "unsupported init expr in element section: {:?}",
                                s
                            ));
                        }
                    };
                environ.declare_table_elements(
                    TableIndex::from_u32(table_index),
                    base,
                    offset,
                    segments,
                )?
            }
            ElementKind::Passive => {
                let index = ElemIndex::from_u32(index as u32);
                environ.declare_passive_element(index, segments)?;
            }
            ElementKind::Declared => return Err(wasm_unsupported!("element kind declared")),
        }
    }
    Ok(())
}

/// Parses the Code section of the wasm module.
pub fn parse_code_section<'data>(
    code: CodeSectionReader<'data>,
    module_translation_state: &ModuleTranslationState,
    environ: &mut ModuleEnvironment<'data>,
) -> WasmResult<()> {
    for body in code {
        let mut reader = body.map_err(to_wasm_error)?.get_binary_reader();
        let size = reader.bytes_remaining();
        let offset = reader.original_position();
        environ.define_function_body(
            module_translation_state,
            reader.read_bytes(size).map_err(to_wasm_error)?,
            offset,
        )?;
    }
    Ok(())
}

/// Parses the Data section of the wasm module.
pub fn parse_data_section<'data>(
    data: DataSectionReader<'data>,
    environ: &mut ModuleEnvironment<'data>,
) -> WasmResult<()> {
    environ.reserve_data_initializers(data.get_count())?;

    for (index, entry) in data.into_iter().enumerate() {
        let Data { kind, data } = entry.map_err(to_wasm_error)?;
        match kind {
            DataKind::Active {
                memory_index,
                init_expr,
            } => {
                let mut init_expr_reader = init_expr.get_binary_reader();
                let (base, offset) =
                    match init_expr_reader.read_operator().map_err(to_wasm_error)? {
                        Operator::I32Const { value } => (None, value as u32 as usize),
                        Operator::GlobalGet { global_index } => {
                            (Some(GlobalIndex::from_u32(global_index)), 0)
                        }
                        ref s => {
                            return Err(wasm_unsupported!(
                                "unsupported init expr in data section: {:?}",
                                s
                            ))
                        }
                    };
                environ.declare_data_initialization(
                    MemoryIndex::from_u32(memory_index),
                    base,
                    offset,
                    data,
                )?;
            }
            DataKind::Passive => {
                let index = DataIndex::from_u32(index as u32);
                environ.declare_passive_data(index, data)?;
            }
        }
    }

    Ok(())
}

/// Parses the Name section of the wasm module.
pub fn parse_name_section<'data>(
    mut names: NameSectionReader<'data>,
    environ: &mut ModuleEnvironment<'data>,
) -> WasmResult<()> {
    while let Ok(subsection) = names.read() {
        match subsection {
            wasmparser::Name::Function(function_subsection) => {
                if let Some(function_names) = function_subsection
                    .get_map()
                    .ok()
                    .and_then(parse_function_name_subsection)
                {
                    for (index, name) in function_names {
                        environ.declare_func_name(index, name)?;
                    }
                }
            }
            wasmparser::Name::Module(module) => {
                if let Ok(name) = module.get_name() {
                    environ.declare_module_name(name)?;
                }
            }
            wasmparser::Name::Local(_) => {}
        };
    }
    Ok(())
}

fn parse_function_name_subsection(
    mut naming_reader: NamingReader<'_>,
) -> Option<HashMap<FuncIndex, &str>> {
    let mut function_names = HashMap::new();
    for _ in 0..naming_reader.get_count() {
        let Naming { index, name } = naming_reader.read().ok()?;
        if index == std::u32::MAX {
            // We reserve `u32::MAX` for our own use.
            return None;
        }

        if function_names
            .insert(FuncIndex::from_u32(index), name)
            .is_some()
        {
            // If the function index has been previously seen, then we
            // break out of the loop and early return `None`, because these
            // should be unique.
            return None;
        }
    }
    Some(function_names)
}
