//! Polyfill skeleton that traverses the whole WebAssembly module and
//! creates the corresponding import and export types.
//!
//! This shall not be needed once the JS type reflection API is available
//! for the Wasm imports and exports.//!
//! <https://github.com/WebAssembly/js-types/blob/master/proposals/js-types/Overview.md>
use core::convert::TryFrom;
use std::vec::Vec;
use wasmer_types::entity::EntityRef;
use wasmer_types::{
    ExportIndex, FunctionIndex, FunctionType, GlobalIndex, GlobalType, ImportIndex, MemoryIndex,
    MemoryType, ModuleInfo, Pages, SignatureIndex, TableIndex, TableType, TagIndex, TagType, Type,
};

use wasmparser::{
    self, BinaryReaderError, Export, ExportSectionReader, ExternalKind, FunctionSectionReader,
    GlobalSectionReader, GlobalType as WPGlobalType, ImportSectionReader, MemorySectionReader,
    MemoryType as WPMemoryType, NameSectionReader, Parser, Payload, TableSectionReader,
    TagType as WPTagType, TypeRef, TypeSectionReader,
};

pub type WasmResult<T> = Result<T, String>;

#[derive(Default)]
pub struct ModuleInfoPolyfill {
    pub(crate) info: ModuleInfo,
}

impl ModuleInfoPolyfill {
    pub(crate) fn declare_export(&mut self, export: ExportIndex, name: &str) -> WasmResult<()> {
        self.info.exports.insert(String::from(name), export);
        Ok(())
    }

    pub(crate) fn declare_import(
        &mut self,
        import: ImportIndex,
        module: &str,
        field: &str,
    ) -> WasmResult<()> {
        self.info.imports.insert(
            wasmer_types::ImportKey {
                module: String::from(module),
                field: String::from(field),
                import_idx: self.info.imports.len() as u32,
            },
            import,
        );
        Ok(())
    }

    pub(crate) fn reserve_signatures(&mut self, num: u32) -> WasmResult<()> {
        self.info
            .signatures
            .reserve_exact(usize::try_from(num).unwrap());
        Ok(())
    }

    pub(crate) fn declare_signature(&mut self, sig: FunctionType) -> WasmResult<()> {
        self.info.signatures.push(sig);
        Ok(())
    }

    pub(crate) fn declare_func_import(
        &mut self,
        sig_index: SignatureIndex,
        module: &str,
        field: &str,
    ) -> WasmResult<()> {
        debug_assert_eq!(
            self.info.functions.len(),
            self.info.num_imported_functions,
            "Imported functions must be declared first"
        );
        self.declare_import(
            ImportIndex::Function(FunctionIndex::from_u32(
                self.info.num_imported_functions as _,
            )),
            module,
            field,
        )?;
        self.info.functions.push(sig_index);
        self.info.num_imported_functions += 1;
        Ok(())
    }

    pub(crate) fn declare_tag_import(
        &mut self,
        t: wasmparser::TagType,
        module_name: &str,
        field_name: &str,
    ) -> WasmResult<()> {
        debug_assert_eq!(
            self.info.tags.len(),
            self.info.num_imported_tags,
            "Imported tags must be declared first"
        );

        let tag = SignatureIndex::from_u32(t.func_type_idx);
        debug_assert!(
            self.info
                .signatures
                .get(SignatureIndex::from_u32(t.func_type_idx))
                .is_some(),
            "Imported tags must mach a declared signature!"
        );

        self.declare_import(
            ImportIndex::Tag(TagIndex::from_u32(self.info.num_imported_tags as _)),
            module_name,
            field_name,
        )?;

        self.info.num_imported_tags += 1;
        self.info.tags.push(tag);

        Ok(())
    }

    pub(crate) fn declare_table_import(
        &mut self,
        table: TableType,
        module: &str,
        field: &str,
    ) -> WasmResult<()> {
        debug_assert_eq!(
            self.info.tables.len(),
            self.info.num_imported_tables,
            "Imported tables must be declared first"
        );
        self.declare_import(
            ImportIndex::Table(TableIndex::from_u32(self.info.num_imported_tables as _)),
            module,
            field,
        )?;
        self.info.tables.push(table);
        self.info.num_imported_tables += 1;
        Ok(())
    }

    pub(crate) fn declare_memory_import(
        &mut self,
        memory: MemoryType,
        module: &str,
        field: &str,
    ) -> WasmResult<()> {
        debug_assert_eq!(
            self.info.memories.len(),
            self.info.num_imported_memories,
            "Imported memories must be declared first"
        );
        self.declare_import(
            ImportIndex::Memory(MemoryIndex::from_u32(self.info.num_imported_memories as _)),
            module,
            field,
        )?;
        self.info.memories.push(memory);
        self.info.num_imported_memories += 1;
        Ok(())
    }

    pub(crate) fn declare_global_import(
        &mut self,
        global: GlobalType,
        module: &str,
        field: &str,
    ) -> WasmResult<()> {
        debug_assert_eq!(
            self.info.globals.len(),
            self.info.num_imported_globals,
            "Imported globals must be declared first"
        );
        self.declare_import(
            ImportIndex::Global(GlobalIndex::from_u32(self.info.num_imported_globals as _)),
            module,
            field,
        )?;
        self.info.globals.push(global);
        self.info.num_imported_globals += 1;
        Ok(())
    }

    pub(crate) fn reserve_func_types(&mut self, num: u32) -> WasmResult<()> {
        self.info
            .functions
            .reserve_exact(usize::try_from(num).unwrap());
        Ok(())
    }

    pub(crate) fn declare_func_type(&mut self, sig_index: SignatureIndex) -> WasmResult<()> {
        self.info.functions.push(sig_index);
        Ok(())
    }

    pub(crate) fn reserve_tables(&mut self, num: u32) -> WasmResult<()> {
        self.info
            .tables
            .reserve_exact(usize::try_from(num).unwrap());
        Ok(())
    }

    pub(crate) fn declare_table(&mut self, table: TableType) -> WasmResult<()> {
        self.info.tables.push(table);
        Ok(())
    }

    pub(crate) fn reserve_memories(&mut self, num: u32) -> WasmResult<()> {
        self.info
            .memories
            .reserve_exact(usize::try_from(num).unwrap());
        Ok(())
    }

    pub(crate) fn declare_memory(&mut self, memory: MemoryType) -> WasmResult<()> {
        self.info.memories.push(memory);
        Ok(())
    }

    pub(crate) fn reserve_globals(&mut self, num: u32) -> WasmResult<()> {
        self.info
            .globals
            .reserve_exact(usize::try_from(num).unwrap());
        Ok(())
    }

    pub(crate) fn declare_global(&mut self, global: GlobalType) -> WasmResult<()> {
        self.info.globals.push(global);
        Ok(())
    }

    pub(crate) fn reserve_tags(&mut self, num: u32) -> WasmResult<()> {
        self.info.tags.reserve_exact(usize::try_from(num).unwrap());
        Ok(())
    }

    pub(crate) fn declare_tag(&mut self, tag: SignatureIndex) -> WasmResult<()> {
        self.info.tags.push(tag);
        Ok(())
    }

    pub(crate) fn reserve_exports(&mut self, num: u32) -> WasmResult<()> {
        self.info.exports.reserve(usize::try_from(num).unwrap());
        Ok(())
    }

    pub(crate) fn reserve_imports(&mut self, num: u32) -> WasmResult<()> {
        self.info.imports.reserve(usize::try_from(num).unwrap());
        Ok(())
    }

    pub(crate) fn declare_func_export(
        &mut self,
        func_index: FunctionIndex,
        name: &str,
    ) -> WasmResult<()> {
        self.declare_export(ExportIndex::Function(func_index), name)
    }

    pub(crate) fn declare_table_export(
        &mut self,
        table_index: TableIndex,
        name: &str,
    ) -> WasmResult<()> {
        self.declare_export(ExportIndex::Table(table_index), name)
    }

    pub(crate) fn declare_memory_export(
        &mut self,
        memory_index: MemoryIndex,
        name: &str,
    ) -> WasmResult<()> {
        self.declare_export(ExportIndex::Memory(memory_index), name)
    }

    pub(crate) fn declare_global_export(
        &mut self,
        global_index: GlobalIndex,
        name: &str,
    ) -> WasmResult<()> {
        self.declare_export(ExportIndex::Global(global_index), name)
    }

    pub(crate) fn declare_tag_export(&mut self, tag_index: TagIndex, name: &str) -> WasmResult<()> {
        self.declare_export(ExportIndex::Tag(tag_index), name)
    }

    pub(crate) fn declare_module_name(&mut self, name: &str) -> WasmResult<()> {
        self.info.name = Some(name.to_string());
        Ok(())
    }
}

fn transform_err(err: BinaryReaderError) -> String {
    err.message().into()
}

/// Translate a sequence of bytes forming a valid Wasm binary into a
/// parsed ModuleInfo `ModuleInfoPolyfill`.
pub fn translate_module(data: &[u8]) -> WasmResult<ModuleInfoPolyfill> {
    let mut module_info: ModuleInfoPolyfill = Default::default();

    for payload in Parser::new(0).parse_all(data) {
        match payload.map_err(transform_err)? {
            Payload::TypeSection(types) => {
                parse_type_section(types, &mut module_info)?;
            }

            Payload::ImportSection(imports) => {
                parse_import_section(imports, &mut module_info)?;
            }

            Payload::FunctionSection(functions) => {
                parse_function_section(functions, &mut module_info)?;
            }

            Payload::TableSection(tables) => {
                parse_table_section(tables, &mut module_info)?;
            }

            Payload::MemorySection(memories) => {
                parse_memory_section(memories, &mut module_info)?;
            }

            Payload::GlobalSection(globals) => {
                parse_global_section(globals, &mut module_info)?;
            }

            Payload::ExportSection(exports) => {
                parse_export_section(exports, &mut module_info)?;
            }

            Payload::TagSection(tags) => {
                parse_tag_section(tags, &mut module_info)?;
            }

            Payload::CustomSection(sectionreader) => {
                // We still add the custom section data, but also read it as name section reader
                let name = sectionreader.name();
                if name == "name" {
                    parse_name_section(
                        NameSectionReader::new(wasmparser::BinaryReader::new(
                            sectionreader.data(),
                            sectionreader.data_offset(),
                        )),
                        &mut module_info,
                    )?;
                }
            }

            _ => {}
        }
    }

    Ok(module_info)
}

/// Helper function translating wasmparser types to Wasm Type.
pub fn wptype_to_type(ty: wasmparser::ValType) -> WasmResult<Type> {
    match ty {
        wasmparser::ValType::I32 => Ok(Type::I32),
        wasmparser::ValType::I64 => Ok(Type::I64),
        wasmparser::ValType::F32 => Ok(Type::F32),
        wasmparser::ValType::F64 => Ok(Type::F64),
        wasmparser::ValType::V128 => Ok(Type::V128),
        wasmparser::ValType::Ref(ty) => wpreftype_to_type(ty),
    }
}

/// Converts a wasmparser ref type to a [`Type`].
pub fn wpreftype_to_type(ty: wasmparser::RefType) -> WasmResult<Type> {
    if ty.is_extern_ref() {
        Ok(Type::ExternRef)
    } else if ty.is_func_ref() {
        Ok(Type::FuncRef)
    } else {
        Err(format!("Unsupported ref type: {ty:?}"))
    }
}

/// Parses the Type section of the wasm module.
pub fn parse_type_section(
    reader: TypeSectionReader,
    module_info: &mut ModuleInfoPolyfill,
) -> WasmResult<()> {
    module_info.reserve_signatures(reader.count())?;

    for res in reader {
        let group = res.map_err(transform_err)?;

        for ty in group.into_types() {
            match ty.composite_type.inner {
                wasmparser::CompositeInnerType::Func(functype) => {
                    let params = functype.params();
                    let returns = functype.results();
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
                    let sig = FunctionType::new(sig_params, sig_returns);
                    module_info.declare_signature(sig)?;
                }
                _ => {
                    unimplemented!("GC is  not implemented yet")
                }
            }
        }
    }

    Ok(())
}

/// Parses the Import section of the wasm module.
pub fn parse_import_section(
    imports: ImportSectionReader<'_>,
    module_info: &mut ModuleInfoPolyfill,
) -> WasmResult<()> {
    module_info.reserve_imports(imports.count())?;

    for entry in imports {
        let import = entry.map_err(transform_err)?;
        let module_name = import.module;
        let field_name = import.name;

        match import.ty {
            TypeRef::Func(sig) => {
                module_info.declare_func_import(
                    SignatureIndex::from_u32(sig),
                    module_name,
                    field_name,
                )?;
            }
            TypeRef::Tag(t) => {
                module_info.declare_tag_import(t, module_name, field_name)?;
            }
            TypeRef::Memory(WPMemoryType {
                shared,
                memory64,
                initial,
                maximum,
                ..
            }) => {
                if memory64 {
                    unimplemented!("64bit memory not implemented yet");
                }
                module_info.declare_memory_import(
                    MemoryType {
                        minimum: Pages(initial as u32),
                        maximum: maximum.map(|p| Pages(p as u32)),
                        shared,
                    },
                    module_name,
                    field_name,
                )?;
            }
            TypeRef::Global(ref ty) => {
                module_info.declare_global_import(
                    GlobalType {
                        ty: wptype_to_type(ty.content_type).unwrap(),
                        mutability: ty.mutable.into(),
                    },
                    module_name,
                    field_name,
                )?;
            }
            TypeRef::Table(ref tab) => {
                module_info.declare_table_import(
                    TableType {
                        ty: wpreftype_to_type(tab.element_type).unwrap(),
                        minimum: tab.initial as u32,
                        maximum: tab.maximum.map(|v| v as u32),
                    },
                    module_name,
                    field_name,
                )?;
            }
        }
    }
    Ok(())
}

/// Parses the Function section of the wasm module.
pub fn parse_function_section(
    functions: FunctionSectionReader,
    module_info: &mut ModuleInfoPolyfill,
) -> WasmResult<()> {
    let num_functions = functions.count();
    module_info.reserve_func_types(num_functions)?;

    for entry in functions {
        let sigindex = entry.map_err(transform_err)?;
        module_info.declare_func_type(SignatureIndex::from_u32(sigindex))?;
    }

    Ok(())
}

/// Parses the Table section of the wasm module.
pub fn parse_table_section(
    tables: TableSectionReader,
    module_info: &mut ModuleInfoPolyfill,
) -> WasmResult<()> {
    module_info.reserve_tables(tables.count())?;

    for entry in tables {
        let table = entry.map_err(transform_err)?;
        module_info.declare_table(TableType {
            ty: wpreftype_to_type(table.ty.element_type).unwrap(),
            minimum: table.ty.initial as u32,
            maximum: table.ty.maximum.map(|v| v as u32),
        })?;
    }

    Ok(())
}

/// Parses the Memory section of the wasm module.
pub fn parse_memory_section(
    memories: MemorySectionReader,
    module_info: &mut ModuleInfoPolyfill,
) -> WasmResult<()> {
    module_info.reserve_memories(memories.count())?;

    for entry in memories {
        let WPMemoryType {
            shared,
            memory64,
            initial,
            maximum,
            ..
        } = entry.map_err(transform_err)?;
        if memory64 {
            unimplemented!("64bit memory not implemented yet");
        }
        module_info.declare_memory(MemoryType {
            minimum: Pages(initial as u32),
            maximum: maximum.map(|p| Pages(p as u32)),
            shared,
        })?;
    }

    Ok(())
}

/// Parses the Global section of the wasm module.
pub fn parse_global_section(
    globals: GlobalSectionReader,
    module_info: &mut ModuleInfoPolyfill,
) -> WasmResult<()> {
    module_info.reserve_globals(globals.count())?;

    for entry in globals {
        let WPGlobalType {
            content_type,
            mutable,
            ..
        } = entry.map_err(transform_err)?.ty;
        let global = GlobalType {
            ty: wptype_to_type(content_type).unwrap(),
            mutability: mutable.into(),
        };
        module_info.declare_global(global)?;
    }

    Ok(())
}

/// Parses the Tag section of the wasm module.
fn parse_tag_section(
    tags: wasmparser::SectionLimited<wasmparser::TagType>,
    module_info: &mut ModuleInfoPolyfill,
) -> WasmResult<()> {
    module_info.reserve_tags(tags.count())?;

    for entry in tags {
        let WPTagType { func_type_idx, .. } = entry.map_err(transform_err)?;
        module_info.declare_tag(SignatureIndex::from_u32(func_type_idx))?;
    }

    Ok(())
}

/// Parses the Export section of the wasm module.
pub fn parse_export_section(
    exports: ExportSectionReader<'_>,
    module_info: &mut ModuleInfoPolyfill,
) -> WasmResult<()> {
    module_info.reserve_exports(exports.count())?;

    for entry in exports {
        let Export {
            name,
            ref kind,
            index,
        } = entry.map_err(transform_err)?;

        // The input has already been validated, so we should be able to
        // assume valid UTF-8 and use `from_utf8_unchecked` if performance
        // becomes a concern here.
        let index = index as usize;
        match *kind {
            ExternalKind::Func => {
                module_info.declare_func_export(FunctionIndex::new(index), name)?
            }
            ExternalKind::Table => {
                module_info.declare_table_export(TableIndex::new(index), name)?
            }
            ExternalKind::Memory => {
                module_info.declare_memory_export(MemoryIndex::new(index), name)?
            }
            ExternalKind::Global => {
                module_info.declare_global_export(GlobalIndex::new(index), name)?
            }
            ExternalKind::Tag => module_info.declare_tag_export(TagIndex::new(index), name)?,
        }
    }
    Ok(())
}

// /// Parses the Start section of the wasm module.
// pub fn parse_start_section(index: u32, module_info: &mut ModuleInfoPolyfill) -> WasmResult<()> {
//     module_info.declare_start_function(FunctionIndex::from_u32(index))?;
//     Ok(())
// }

/// Parses the Name section of the wasm module.
pub fn parse_name_section(
    mut names: NameSectionReader<'_>,
    module_info: &mut ModuleInfoPolyfill,
) -> WasmResult<()> {
    while let Some(Ok(subsection)) = names.next() {
        match subsection {
            wasmparser::Name::Function(_function_subsection) => {
                //for naming in function_subsection.into_iter().flatten() {
                //    if naming.index != std::u32::MAX {
                //        environ.declare_function_name(
                //            FunctionIndex::from_u32(naming.index),
                //            naming.name,
                //        )?;
                //    }
                //}
            }
            wasmparser::Name::Module {
                name,
                name_range: _,
            } => {
                module_info.declare_module_name(name)?;
            }
            wasmparser::Name::Local(_) => {}
            wasmparser::Name::Label(_)
            | wasmparser::Name::Type(_)
            | wasmparser::Name::Table(_)
            | wasmparser::Name::Memory(_)
            | wasmparser::Name::Global(_)
            | wasmparser::Name::Element(_)
            | wasmparser::Name::Data(_)
            | wasmparser::Name::Tag(_)
            | wasmparser::Name::Field(_)
            | wasmparser::Name::Unknown { .. } => {}
        };
    }
    Ok(())
}

// fn parse_function_name_subsection(
//     mut naming_reader: NamingReader<'_>,
// ) -> Option<HashMap<FunctionIndex, &str>> {
//     let mut function_names = HashMap::new();
//     for _ in 0..naming_reader.count() {
//         let Naming { index, name } = naming_reader.read().ok()?;
//         if index == std::u32::MAX {
//             // We reserve `u32::MAX` for our own use.
//             return None;
//         }

//         if function_names
//             .insert(FunctionIndex::from_u32(index), name)
//             .is_some()
//         {
//             // If the function index has been previously seen, then we
//             // break out of the loop and early return `None`, because these
//             // should be unique.
//             return None;
//         }
//     }
//     Some(function_names)
// }
