use std::collections::{BTreeMap, BTreeSet};

use wasmparser::{
    BinaryReader, Dylink0SectionReader, Dylink0Subsection, ExternalKind, FuncType, GlobalType,
    MemoryType, Operator, Parser, Payload, SymbolFlags, TableType, TypeRef, ValType, Validator,
};

use crate::PlanError;

/// A fully typed core wasm import/export. Module-local type indices are not compared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternType {
    Function(FuncType),
    Global(GlobalType),
    Memory(MemoryType),
    Table(TableType),
    Tag(FuncType),
}

/// One import, in wasm import-section order (including host-supplied imports).
#[derive(Clone, Debug)]
pub struct Import {
    pub module: String,
    pub name: String,
    pub ty: ExternType,
    pub flags: SymbolFlags,
}

/// One export. `index` is in its wasm kind's index space, not a guest function pointer.
#[derive(Clone, Debug)]
pub struct Export {
    pub index: u32,
    pub ty: ExternType,
    pub flags: SymbolFlags,
    /// Literal i32 initializer if known; imported/global.get/extended initializers are deferred.
    /// Whether this is data-relative, TLS-relative or absolute is in `flags`.
    pub offset: Option<u32>,
}

/// Validated wasm32 module metadata. Unknown dylink subsections are retained, not interpreted.
#[derive(Clone, Debug)]
pub struct Module {
    pub(crate) memory: wasmparser::MemInfo,
    pub(crate) needed: Vec<String>,
    runtime_path: Vec<String>,
    pub(crate) imports: Vec<Import>,
    pub(crate) functions: Vec<FuncType>,
    pub(crate) exports: BTreeMap<String, Export>,
    unknown: Vec<(u8, Vec<u8>)>,
    start: Option<u32>,
}

impl Module {
    /// Validates a core module, requires exactly one `dylink.0` section and its memory-info
    /// subsection, and retains type information without compiling or evaluating initializers.
    /// GC/typed-reference signatures and wasm64 are not supported by this planner.
    pub fn parse(bytes: &[u8]) -> Result<Self, PlanError> {
        Validator::new().validate_all(bytes)?;
        let mut types = Vec::new();
        let mut functions = Vec::new();
        let mut globals = Vec::new();
        let mut tables = Vec::new();
        let mut memories = Vec::new();
        let mut tags = Vec::new();
        let mut raw_imports = Vec::new();
        let mut raw_exports = Vec::new();
        let mut dylink = None;
        let mut start = None;
        for payload in Parser::new(0).parse_all(bytes) {
            match payload? {
                Payload::Version {
                    encoding: wasmparser::Encoding::Module,
                    ..
                } => {}
                Payload::Version { .. } => return Err(PlanError::Unsupported("component")),
                Payload::TypeSection(section) => {
                    for ty in section.into_iter_err_on_gc_types() {
                        let ty = ty?;
                        if ty.params().iter().chain(ty.results()).any(|t| matches!(t, ValType::Ref(r) if !matches!(r.heap_type(), wasmparser::HeapType::Abstract { .. }))) {
                            return Err(PlanError::Unsupported("typed-reference signature"));
                        }
                        types.push(ty);
                    }
                }
                Payload::ImportSection(section) => {
                    for import in section.into_imports() {
                        let import = import?;
                        let ty = match import.ty {
                            TypeRef::FuncExact(_) => {
                                return Err(PlanError::Unsupported("exact function import"));
                            }
                            TypeRef::Func(i) => {
                                let ty = types[i as usize].clone();
                                functions.push(ty.clone());
                                ExternType::Function(ty)
                            }
                            TypeRef::Global(ty) => {
                                globals.push((ty, None));
                                ExternType::Global(ty)
                            }
                            TypeRef::Table(ty) => {
                                tables.push(ty);
                                ExternType::Table(ty)
                            }
                            TypeRef::Memory(ty) => {
                                memories.push(ty);
                                ExternType::Memory(ty)
                            }
                            TypeRef::Tag(ty) => {
                                let ty = types[ty.func_type_idx as usize].clone();
                                tags.push(ty.clone());
                                ExternType::Tag(ty)
                            }
                        };
                        raw_imports.push((import.module.to_owned(), import.name.to_owned(), ty));
                    }
                }
                Payload::FunctionSection(section) => {
                    for i in section {
                        functions.push(types[i? as usize].clone());
                    }
                }
                Payload::GlobalSection(section) => {
                    for global in section {
                        let global = global?;
                        let mut ops = global.init_expr.get_operators_reader();
                        let offset = match (ops.read()?, ops.read()?) {
                            (Operator::I32Const { value }, Operator::End) => Some(value as u32),
                            _ => None,
                        };
                        globals.push((global.ty, offset));
                    }
                }
                Payload::MemorySection(section) => {
                    for ty in section {
                        memories.push(ty?);
                    }
                }
                Payload::TableSection(section) => {
                    for table in section {
                        tables.push(table?.ty);
                    }
                }
                Payload::TagSection(section) => {
                    for tag in section {
                        tags.push(types[tag?.func_type_idx as usize].clone());
                    }
                }
                Payload::ExportSection(section) => {
                    for export in section {
                        let export = export?;
                        raw_exports.push((export.name.to_owned(), export.kind, export.index));
                    }
                }
                Payload::StartSection { func, .. } => start = Some(func),
                Payload::CustomSection(section) if section.name() == "dylink.0" => {
                    if dylink.is_some() {
                        return Err(PlanError::Metadata("duplicate dylink.0 section"));
                    }
                    dylink = Some(parse_dylink(section.data(), section.data_offset())?);
                }
                _ => {}
            }
        }
        if memories.iter().any(|m| m.memory64) || tables.iter().any(|t| t.table64) {
            return Err(PlanError::Unsupported("wasm64"));
        }
        let Dylink {
            memory,
            needed,
            runtime_path,
            mut import_flags,
            mut export_flags,
            unknown,
        } = dylink.ok_or(PlanError::Metadata("missing dylink.0 section"))?;
        let imports = raw_imports
            .into_iter()
            .map(|(module, name, ty)| {
                let flags = import_flags
                    .get(&(module.clone(), name.clone()))
                    .copied()
                    .unwrap_or_default();
                Import {
                    module,
                    name,
                    ty,
                    flags,
                }
            })
            .collect::<Vec<_>>();
        for import in &imports {
            import_flags.remove(&(import.module.clone(), import.name.clone()));
        }
        if !import_flags.is_empty() {
            return Err(PlanError::Metadata(
                "dylink metadata names a missing import",
            ));
        }
        let mut exports = BTreeMap::new();
        for (name, kind, index) in raw_exports {
            let i = index as usize;
            let (ty, offset) = match kind {
                ExternalKind::FuncExact => {
                    return Err(PlanError::Unsupported("exact function export"));
                }
                ExternalKind::Func => (ExternType::Function(functions[i].clone()), None),
                ExternalKind::Global => (ExternType::Global(globals[i].0), globals[i].1),
                ExternalKind::Memory => (ExternType::Memory(memories[i]), None),
                ExternalKind::Table => (ExternType::Table(tables[i]), None),
                ExternalKind::Tag => (ExternType::Tag(tags[i].clone()), None),
            };
            let flags = export_flags.remove(&name).unwrap_or_default();
            if flags.contains(SymbolFlags::UNDEFINED) {
                return Err(PlanError::Metadata("export marked undefined"));
            }
            exports.insert(
                name,
                Export {
                    index,
                    ty,
                    flags,
                    offset,
                },
            );
        }
        if !export_flags.is_empty() {
            return Err(PlanError::Metadata(
                "dylink metadata names a missing export",
            ));
        }
        Ok(Self {
            memory,
            needed,
            runtime_path,
            imports,
            functions,
            exports,
            unknown,
            start,
        })
    }

    pub fn memory_info(&self) -> wasmparser::MemInfo {
        self.memory
    }
    pub fn needed(&self) -> &[String] {
        &self.needed
    }
    pub fn runtime_path(&self) -> &[String] {
        &self.runtime_path
    }
    pub fn imports(&self) -> &[Import] {
        &self.imports
    }
    pub fn exports(&self) -> &BTreeMap<String, Export> {
        &self.exports
    }
    pub fn unknown_subsections(&self) -> &[(u8, Vec<u8>)] {
        &self.unknown
    }
    /// The start function is untrusted; recording its index grants no permission to run it.
    pub fn start(&self) -> Option<u32> {
        self.start
    }
}

struct Dylink {
    memory: wasmparser::MemInfo,
    needed: Vec<String>,
    runtime_path: Vec<String>,
    import_flags: BTreeMap<(String, String), SymbolFlags>,
    export_flags: BTreeMap<String, SymbolFlags>,
    unknown: Vec<(u8, Vec<u8>)>,
}

fn flags(flags: SymbolFlags) -> Result<SymbolFlags, PlanError> {
    if flags.bits() & !SymbolFlags::all().bits() != 0
        || flags.contains(SymbolFlags::BINDING_LOCAL | SymbolFlags::BINDING_WEAK)
    {
        return Err(PlanError::Metadata(
            "unsupported or conflicting symbol flags",
        ));
    }
    Ok(flags)
}

fn parse_dylink(bytes: &[u8], offset: u64) -> Result<Dylink, PlanError> {
    check_subsection_lengths(bytes, offset)?;
    let mut memory = None;
    let mut needed = Vec::new();
    let mut runtime_path = Vec::new();
    let mut import_flags = BTreeMap::new();
    let mut export_flags = BTreeMap::new();
    let mut unknown = Vec::new();
    let mut seen = BTreeSet::new();
    for subsection in Dylink0SectionReader::new(BinaryReader::new(bytes, offset)) {
        let subsection = subsection?;
        let id = match &subsection {
            Dylink0Subsection::MemInfo(_) => 1,
            Dylink0Subsection::Needed(_) => 2,
            Dylink0Subsection::ExportInfo(_) => 3,
            Dylink0Subsection::ImportInfo(_) => 4,
            Dylink0Subsection::RuntimePath(_) => 5,
            Dylink0Subsection::Unknown { ty, .. } => *ty,
        };
        if id <= 5 && !seen.insert(id) {
            return Err(PlanError::Metadata("duplicate dylink subsection"));
        }
        match subsection {
            Dylink0Subsection::MemInfo(info) => {
                if info.memory_alignment >= 32 || info.table_alignment >= 32 {
                    return Err(PlanError::Metadata(
                        "wasm32 alignment exponent out of range",
                    ));
                }
                memory = Some(info);
            }
            Dylink0Subsection::Needed(names) => {
                needed = names.into_iter().map(str::to_owned).collect()
            }
            Dylink0Subsection::RuntimePath(paths) => {
                runtime_path = paths.into_iter().map(str::to_owned).collect()
            }
            Dylink0Subsection::ImportInfo(entries) => {
                for entry in entries {
                    if import_flags
                        .insert(
                            (entry.module.to_owned(), entry.field.to_owned()),
                            flags(entry.flags)?,
                        )
                        .is_some()
                    {
                        return Err(PlanError::Metadata("duplicate import flags"));
                    }
                }
            }
            Dylink0Subsection::ExportInfo(entries) => {
                for entry in entries {
                    if export_flags
                        .insert(entry.name.to_owned(), flags(entry.flags)?)
                        .is_some()
                    {
                        return Err(PlanError::Metadata("duplicate export flags"));
                    }
                }
            }
            Dylink0Subsection::Unknown { ty, data, .. } => unknown.push((ty, data.to_vec())),
        }
    }
    Ok(Dylink {
        memory: memory.ok_or(PlanError::Metadata("missing memory-info subsection"))?,
        needed,
        runtime_path,
        import_flags,
        export_flags,
        unknown,
    })
}

// wasmparser's dylink reader intentionally ignores trailing bytes in known subsections.
// A link plan must not silently accept two different interpretations of the same metadata.
fn check_subsection_lengths(bytes: &[u8], offset: u64) -> Result<(), PlanError> {
    let mut reader = BinaryReader::new(bytes, offset);
    while !reader.eof() {
        let id = reader.read_u8()?;
        let mut body = reader.read_reader()?;
        match id {
            1 => {
                for _ in 0..4 {
                    body.read_var_u32()?;
                }
            }
            2..=5 => {
                let count = body.read_var_u32()?;
                for _ in 0..count {
                    body.read_string()?;
                    if id == 4 {
                        body.read_string()?;
                    }
                    if id == 3 || id == 4 {
                        body.read_var_u32()?;
                    }
                }
            }
            _ => continue,
        }
        if !body.eof() {
            return Err(PlanError::Metadata("trailing bytes in dylink subsection"));
        }
    }
    Ok(())
}
