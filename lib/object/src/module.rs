use crate::error::ObjectError;
use object::write::{Object, Relocation, StandardSection, Symbol, SymbolSection};
use object::{RelocationEncoding, RelocationKind, SymbolFlags, SymbolKind, SymbolScope};
use wasm_common::{FunctionIndex, LocalFunctionIndex, SignatureIndex};
use wasmer_compiler::{
    Architecture, BinaryFormat, Compilation, CustomSectionProtection, Endianness, RelocationTarget,
    SectionIndex, Triple,
};

/// Returns names for the compilation types (functions, sections and trampolines)
pub trait CompilationNamer {
    /// Gets the function name given a local function index
    fn get_function_name(&self, index: &LocalFunctionIndex) -> String;

    /// Gets the section name given a section index
    fn get_section_name(&self, index: &SectionIndex) -> String;

    /// Gets the function call trampoline name given a signature index
    fn get_function_call_trampoline_name(&self, index: &SignatureIndex) -> String;

    /// Gets the dynamic function trampoline name given a function index
    fn get_dynamic_function_trampoline_name(&self, index: &FunctionIndex) -> String;
}

/// Create an object for a given target [`Triple`]
pub fn get_object_for_target(triple: &Triple) -> Result<Object, ObjectError> {
    let obj_binary_format = match triple.binary_format {
        BinaryFormat::Elf => object::BinaryFormat::Elf,
        BinaryFormat::Macho => object::BinaryFormat::MachO,
        BinaryFormat::Coff => object::BinaryFormat::Coff,
        binary_format => {
            return Err(ObjectError::UnsupportedBinaryFormat(format!(
                "{}",
                binary_format
            )));
        }
    };
    let obj_architecture = match triple.architecture {
        Architecture::X86_64 => object::Architecture::X86_64,
        Architecture::Aarch64(_) => object::Architecture::Aarch64,
        architecture => {
            return Err(ObjectError::UnsupportedArchitecture(format!(
                "{}",
                architecture
            )));
        }
    };
    let obj_endianness = match triple
        .endianness()
        .map_err(|_| ObjectError::UnknownEndianness)?
    {
        Endianness::Little => object::Endianness::Little,
        Endianness::Big => object::Endianness::Big,
    };

    Ok(Object::new(
        obj_binary_format,
        obj_architecture,
        obj_endianness,
    ))
}

/// Emit data in into an object
pub fn emit_data(obj: &mut Object, name: &[u8], data: &[u8]) -> Result<(), ObjectError> {
    let symbol_id = obj.add_symbol(Symbol {
        name: name.to_vec(),
        value: 0,
        size: 0,
        kind: SymbolKind::Data,
        scope: SymbolScope::Dynamic,
        weak: false,
        section: SymbolSection::Undefined,
        flags: SymbolFlags::None,
    });
    let section_id = obj.section_id(StandardSection::Data);
    obj.add_symbol_data(symbol_id, section_id, &data, 1);

    Ok(())
}

/// Emit the compilation into an object
pub fn emit_compilation(
    obj: &mut Object,
    compilation: Compilation,
    namer: &impl CompilationNamer,
    triple: &Triple,
) -> Result<(), ObjectError> {
    let function_bodies = compilation.get_function_bodies();
    let function_relocations = compilation.get_relocations();
    let custom_sections = compilation.get_custom_sections();
    let custom_section_relocations = compilation.get_custom_section_relocations();
    let function_call_trampolines = compilation.get_function_call_trampolines();
    let dynamic_function_trampolines = compilation.get_dynamic_function_trampolines();

    // Add sections
    for (section_index, custom_section) in custom_sections.iter() {
        // TODO: We need to rename the sections corresponding to the DWARF information
        // to the proper names (like `.eh_frame`)
        let section_name = namer.get_section_name(&section_index);
        let (section_kind, standard_section) = match custom_section.protection {
            CustomSectionProtection::ReadExecute => (SymbolKind::Text, StandardSection::Text),
            // TODO: Fix this to be StandardSection::Data
            CustomSectionProtection::Read => (SymbolKind::Data, StandardSection::Text),
        };
        let symbol_id = obj.add_symbol(Symbol {
            name: section_name.into_bytes(),
            value: 0,
            size: 0,
            kind: section_kind,
            scope: SymbolScope::Dynamic,
            weak: false,
            section: SymbolSection::Undefined,
            flags: SymbolFlags::None,
        });
        let section_id = obj.section_id(standard_section);
        obj.add_symbol_data(symbol_id, section_id, custom_section.bytes.as_slice(), 1);
    }

    // Add functions
    for (function_local_index, function) in function_bodies.into_iter() {
        let function_name = namer.get_function_name(&function_local_index);
        let symbol_id = obj.add_symbol(Symbol {
            name: function_name.into_bytes(),
            value: 0,
            size: 0,
            kind: SymbolKind::Text,
            scope: SymbolScope::Dynamic,
            weak: false,
            section: SymbolSection::Undefined,
            flags: SymbolFlags::None,
        });

        let section_id = obj.section_id(StandardSection::Text);
        obj.add_symbol_data(symbol_id, section_id, &function.body, 1);
    }

    // Add function call trampolines
    for (signature_index, function) in function_call_trampolines.into_iter() {
        let function_name = namer.get_function_call_trampoline_name(&signature_index);
        let symbol_id = obj.add_symbol(Symbol {
            name: function_name.into_bytes(),
            value: 0,
            size: 0,
            kind: SymbolKind::Text,
            scope: SymbolScope::Dynamic,
            weak: false,
            section: SymbolSection::Undefined,
            flags: SymbolFlags::None,
        });
        let section_id = obj.section_id(StandardSection::Text);
        obj.add_symbol_data(symbol_id, section_id, &function.body, 1);
    }

    // Add dynamic function trampolines
    for (func_index, function) in dynamic_function_trampolines.into_iter() {
        let function_name = namer.get_dynamic_function_trampoline_name(&func_index);
        let symbol_id = obj.add_symbol(Symbol {
            name: function_name.into_bytes(),
            value: 0,
            size: 0,
            kind: SymbolKind::Text,
            scope: SymbolScope::Dynamic,
            weak: false,
            section: SymbolSection::Undefined,
            flags: SymbolFlags::None,
        });
        let section_id = obj.section_id(StandardSection::Text);
        obj.add_symbol_data(symbol_id, section_id, &function.body, 1);
    }

    // Add relocations (function and sections)
    let relocation_size = triple
        .pointer_width()
        .map(|pointer_width| pointer_width.bits())
        .map_err(|_| ObjectError::UnsupportedArchitecture(triple.architecture.to_string()))?;
    let (relocation_kind, relocation_encoding) = match triple.architecture {
        Architecture::X86_64 => (RelocationKind::PltRelative, RelocationEncoding::X86Branch),
        architecture => {
            return Err(ObjectError::UnsupportedArchitecture(
                architecture.to_string(),
            ))
        }
    };
    let mut all_relocations = Vec::new();

    for (function_local_index, relocations) in function_relocations.into_iter() {
        let function_name = namer.get_function_name(&function_local_index);
        let symbol_id = obj.symbol_id(function_name.as_bytes()).unwrap();
        all_relocations.push((symbol_id, relocations))
    }

    for (section_index, relocations) in custom_section_relocations.into_iter() {
        let section_name = namer.get_section_name(&section_index);
        let symbol_id = obj.symbol_id(section_name.as_bytes()).unwrap();
        all_relocations.push((symbol_id, relocations))
    }

    for (symbol_id, relocations) in all_relocations.into_iter() {
        let (_symbol_id, section_offset) = obj.symbol_section_and_offset(symbol_id).unwrap();
        let section_id = obj.section_id(StandardSection::Text);

        for r in relocations {
            let relocation_address = section_offset + r.offset as u64;

            match r.reloc_target {
                RelocationTarget::LocalFunc(index) => {
                    let target_name = namer.get_function_name(&index);
                    let target_symbol = obj.symbol_id(target_name.as_bytes()).unwrap();
                    obj.add_relocation(
                        section_id,
                        Relocation {
                            offset: relocation_address,
                            size: relocation_size,
                            kind: relocation_kind,
                            encoding: relocation_encoding,
                            symbol: target_symbol,
                            addend: r.addend,
                        },
                    )
                    .map_err(ObjectError::Write)?;
                }
                RelocationTarget::LibCall(libcall) => {
                    let libcall_fn_name = libcall.to_function_name().as_bytes();
                    // We add the symols lazily as we see them
                    let target_symbol = obj.symbol_id(libcall_fn_name).unwrap_or_else(|| {
                        obj.add_symbol(Symbol {
                            name: libcall_fn_name.to_vec(),
                            value: 0,
                            size: 0,
                            kind: SymbolKind::Unknown,
                            scope: SymbolScope::Unknown,
                            weak: false,
                            section: SymbolSection::Undefined,
                            flags: SymbolFlags::None,
                        })
                    });
                    obj.add_relocation(
                        section_id,
                        Relocation {
                            offset: relocation_address,
                            size: relocation_size,
                            kind: relocation_kind,
                            encoding: relocation_encoding,
                            symbol: target_symbol,
                            addend: r.addend,
                        },
                    )
                    .map_err(ObjectError::Write)?;
                }
                RelocationTarget::CustomSection(section_index) => {
                    let target_name = namer.get_section_name(&section_index);
                    let target_symbol = obj.symbol_id(target_name.as_bytes()).unwrap();
                    obj.add_relocation(
                        section_id,
                        Relocation {
                            offset: relocation_address,
                            size: relocation_size,
                            kind: relocation_kind,
                            encoding: relocation_encoding,
                            symbol: target_symbol,
                            addend: r.addend,
                        },
                    )
                    .map_err(ObjectError::Write)?;
                }
                RelocationTarget::JumpTable(_func_index, _jt) => {
                    // do nothing
                }
            };
        }
    }
    Ok(())
}
