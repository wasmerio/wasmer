use std::collections::HashMap;

use tracing::warn;
use wasmer::Module;

use super::LinkError;

#[derive(Debug, Clone)]
pub struct DylinkInfo {
    pub mem_info: wasmparser::MemInfo,
    pub needed: Vec<String>,
    pub import_metadata: HashMap<(String, String), wasmparser::SymbolFlags>,
    pub export_metadata: HashMap<String, wasmparser::SymbolFlags>,
    pub runtime_path: Vec<String>,
}

pub fn is_dynamically_linked(module: &Module) -> bool {
    module.custom_sections("dylink.0").next().is_some()
}

pub fn parse_dylink0_section(module: &Module) -> Result<DylinkInfo, LinkError> {
    let mut sections = module.custom_sections("dylink.0");

    let Some(section) = sections.next() else {
        return Err(LinkError::NotDynamicLibrary);
    };

    // Verify the module contains exactly one dylink.0 section
    let None = sections.next() else {
        return Err(LinkError::NotDynamicLibrary);
    };

    let reader = wasmparser::Dylink0SectionReader::new(wasmparser::BinaryReader::new(&section, 0));

    let mut mem_info = None;
    let mut needed = None;
    let mut import_metadata = HashMap::new();
    let mut export_metadata = HashMap::new();
    let mut runtime_path = Vec::new();

    for subsection in reader {
        let subsection = subsection?;
        match subsection {
            wasmparser::Dylink0Subsection::MemInfo(m) => {
                mem_info = Some(m);
            }
            wasmparser::Dylink0Subsection::Needed(n) => {
                needed = Some(n.iter().map(|s| s.to_string()).collect::<Vec<_>>());
            }
            wasmparser::Dylink0Subsection::ImportInfo(i) => {
                for i in i {
                    import_metadata.insert((i.module.to_owned(), i.field.to_owned()), i.flags);
                }
            }
            wasmparser::Dylink0Subsection::ExportInfo(e) => {
                for e in e {
                    export_metadata.insert(e.name.to_owned(), e.flags);
                }
            }
            wasmparser::Dylink0Subsection::Unknown { ty, .. } => {
                warn!("Skipping unknown dylink.0 subsection {ty}");
            }
            wasmparser::Dylink0Subsection::RuntimePath(path) => {
                runtime_path.extend(path.into_iter().map(|path| path.to_string()));
            }
        }
    }

    Ok(DylinkInfo {
        mem_info: mem_info.unwrap_or(wasmparser::MemInfo {
            memory_size: 0,
            memory_alignment: 0,
            table_size: 0,
            table_alignment: 0,
        }),
        needed: needed.unwrap_or_default(),
        import_metadata,
        export_metadata,
        runtime_path,
    })
}
