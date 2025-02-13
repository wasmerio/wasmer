use super::error::ObjectError;
use crate::types::{
    function::Compilation,
    relocation::{RelocationKind as Reloc, RelocationTarget},
    section::{CustomSectionProtection, SectionIndex},
    symbols::{Symbol, SymbolRegistry},
    target::{Architecture, BinaryFormat, Endianness, Triple},
};
use object::{
    elf, macho,
    write::{
        Object, Relocation, StandardSection, StandardSegment, Symbol as ObjSymbol, SymbolSection,
    },
    FileFlags, RelocationEncoding, RelocationKind, SectionKind, SymbolFlags, SymbolKind,
    SymbolScope,
};
use wasmer_types::entity::PrimaryMap;
use wasmer_types::LocalFunctionIndex;

const DWARF_SECTION_NAME: &[u8] = b".eh_frame";

/// Create an object for a given target `Triple`.
///
/// # Usage
///
/// ```rust
/// # use wasmer_compiler::types::target::Triple;
/// # use wasmer_compiler::object::{ObjectError, get_object_for_target};
///
/// # fn generate_object_for_target(triple: &Triple) -> Result<(), ObjectError> {
/// let mut object = get_object_for_target(&triple)?;
///
/// # Ok(())
/// # }
/// ```
pub fn get_object_for_target(triple: &Triple) -> Result<Object, ObjectError> {
    let obj_binary_format = match triple.binary_format {
        BinaryFormat::Elf => object::BinaryFormat::Elf,
        BinaryFormat::Macho => object::BinaryFormat::MachO,
        BinaryFormat::Coff => object::BinaryFormat::Coff,
        binary_format => {
            return Err(ObjectError::UnsupportedBinaryFormat(format!(
                "{binary_format}"
            )));
        }
    };
    let obj_architecture = match triple.architecture {
        Architecture::X86_64 => object::Architecture::X86_64,
        Architecture::Aarch64(_) => object::Architecture::Aarch64,
        Architecture::Riscv64(_) => object::Architecture::Riscv64,
        Architecture::LoongArch64 => object::Architecture::LoongArch64,
        architecture => {
            return Err(ObjectError::UnsupportedArchitecture(format!(
                "{architecture}"
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

    let mut object = Object::new(obj_binary_format, obj_architecture, obj_endianness);

    if let Architecture::Riscv64(_) = triple.architecture {
        object.flags = FileFlags::Elf {
            e_flags: elf::EF_RISCV_FLOAT_ABI_DOUBLE,
            os_abi: 2,
            abi_version: 0,
        };
    }

    Ok(object)
}

/// Write data into an existing object.
///
/// # Usage
///
/// ```rust
/// # use wasmer_compiler::types::target::Triple;
/// # use wasmer_compiler::object::{ObjectError, get_object_for_target, emit_data};
///
/// # fn emit_data_into_object(triple: &Triple) -> Result<(), ObjectError> {
/// let mut object = get_object_for_target(&triple)?;
/// emit_data(&mut object, b"WASMER_METADATA", &b"Hello, World!"[..], 1)?;
///
/// # Ok(())
/// # }
/// ```
pub fn emit_data(
    obj: &mut Object,
    name: &[u8],
    data: &[u8],
    align: u64,
) -> Result<(), ObjectError> {
    let symbol_id = obj.add_symbol(ObjSymbol {
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
    obj.add_symbol_data(symbol_id, section_id, data, align);

    Ok(())
}

/// Emit the compilation result into an existing object.
///
/// # Usage
///
/// ```rust
/// # use wasmer_compiler::types::{ symbols::SymbolRegistry, target::{Triple}, function::{Compilation} };
/// # use wasmer_compiler::object::{ObjectError, get_object_for_target, emit_compilation};
///
/// # fn emit_compilation_into_object(
/// #     triple: &Triple,
/// #     compilation: Compilation,
/// #     symbol_registry: impl SymbolRegistry,
/// # ) -> Result<(), ObjectError> {
/// let mut object = get_object_for_target(&triple)?;
/// emit_compilation(&mut object, compilation, &symbol_registry, &triple)?;
/// # Ok(())
/// # }
/// ```
pub fn emit_compilation(
    obj: &mut Object,
    compilation: Compilation,
    symbol_registry: &impl SymbolRegistry,
    triple: &Triple,
) -> Result<(), ObjectError> {
    let mut function_bodies = PrimaryMap::with_capacity(compilation.functions.len());
    let mut function_relocations = PrimaryMap::with_capacity(compilation.functions.len());
    for (_, func) in compilation.functions.into_iter() {
        function_bodies.push(func.body);
        function_relocations.push(func.relocations);
    }
    let custom_section_relocations = compilation
        .custom_sections
        .iter()
        .map(|(_, section)| section.relocations.clone())
        .collect::<PrimaryMap<SectionIndex, _>>();

    let debug_index = compilation.unwind_info.eh_frame;

    let align = match triple.architecture {
        Architecture::X86_64 => 1,
        // In Arm64 is recommended a 4-byte alignment
        Architecture::Aarch64(_) => 4,
        _ => 1,
    };

    // Add sections
    let custom_section_ids = compilation
        .custom_sections
        .into_iter()
        .map(|(section_index, custom_section)| {
            if debug_index.map_or(false, |d| d == section_index) {
                // If this is the debug section
                let segment = obj.segment_name(StandardSegment::Debug).to_vec();
                let section_id =
                    obj.add_section(segment, DWARF_SECTION_NAME.to_vec(), SectionKind::Debug);
                obj.append_section_data(section_id, custom_section.bytes.as_slice(), align);
                let section_name = symbol_registry.symbol_to_name(Symbol::Section(section_index));
                let symbol_id = obj.add_symbol(ObjSymbol {
                    name: section_name.into_bytes(),
                    value: 0,
                    size: custom_section.bytes.len() as _,
                    kind: SymbolKind::Data,
                    scope: SymbolScope::Compilation,
                    weak: false,
                    section: SymbolSection::Section(section_id),
                    flags: SymbolFlags::None,
                });
                (section_id, symbol_id)
            } else {
                let section_name = symbol_registry.symbol_to_name(Symbol::Section(section_index));
                let (section_kind, standard_section) = match custom_section.protection {
                    CustomSectionProtection::ReadExecute => {
                        (SymbolKind::Text, StandardSection::Text)
                    }
                    CustomSectionProtection::Read => (SymbolKind::Data, StandardSection::Data),
                };
                let section_id = obj.section_id(standard_section);
                let symbol_id = obj.add_symbol(ObjSymbol {
                    name: section_name.into_bytes(),
                    value: 0,
                    size: custom_section.bytes.len() as _,
                    kind: section_kind,
                    scope: SymbolScope::Dynamic,
                    weak: false,
                    section: SymbolSection::Section(section_id),
                    flags: SymbolFlags::None,
                });
                obj.add_symbol_data(
                    symbol_id,
                    section_id,
                    custom_section.bytes.as_slice(),
                    align,
                );
                (section_id, symbol_id)
            }
        })
        .collect::<PrimaryMap<SectionIndex, _>>();

    // Add functions
    let function_symbol_ids = function_bodies
        .into_iter()
        .map(|(function_local_index, function)| {
            let function_name =
                symbol_registry.symbol_to_name(Symbol::LocalFunction(function_local_index));
            let section_id = obj.section_id(StandardSection::Text);
            let symbol_id = obj.add_symbol(ObjSymbol {
                name: function_name.into_bytes(),
                value: 0,
                size: function.body.len() as _,
                kind: SymbolKind::Text,
                scope: SymbolScope::Dynamic,
                weak: false,
                section: SymbolSection::Section(section_id),
                flags: SymbolFlags::None,
            });
            obj.add_symbol_data(symbol_id, section_id, &function.body, align);
            (section_id, symbol_id)
        })
        .collect::<PrimaryMap<LocalFunctionIndex, _>>();

    // Add function call trampolines
    for (signature_index, function) in compilation.function_call_trampolines.into_iter() {
        let function_name =
            symbol_registry.symbol_to_name(Symbol::FunctionCallTrampoline(signature_index));
        let section_id = obj.section_id(StandardSection::Text);
        let symbol_id = obj.add_symbol(ObjSymbol {
            name: function_name.into_bytes(),
            value: 0,
            size: function.body.len() as _,
            kind: SymbolKind::Text,
            scope: SymbolScope::Dynamic,
            weak: false,
            section: SymbolSection::Section(section_id),
            flags: SymbolFlags::None,
        });
        obj.add_symbol_data(symbol_id, section_id, &function.body, align);
    }

    // Add dynamic function trampolines
    for (func_index, function) in compilation.dynamic_function_trampolines.into_iter() {
        let function_name =
            symbol_registry.symbol_to_name(Symbol::DynamicFunctionTrampoline(func_index));
        let section_id = obj.section_id(StandardSection::Text);
        let symbol_id = obj.add_symbol(ObjSymbol {
            name: function_name.into_bytes(),
            value: 0,
            size: function.body.len() as _,
            kind: SymbolKind::Text,
            scope: SymbolScope::Dynamic,
            weak: false,
            section: SymbolSection::Section(section_id),
            flags: SymbolFlags::None,
        });
        obj.add_symbol_data(symbol_id, section_id, &function.body, align);
    }

    let mut all_relocations = Vec::new();

    for (function_local_index, relocations) in function_relocations.into_iter() {
        let (section_id, symbol_id) = function_symbol_ids.get(function_local_index).unwrap();
        all_relocations.push((*section_id, *symbol_id, relocations))
    }

    for (section_index, relocations) in custom_section_relocations.into_iter() {
        if !debug_index.map_or(false, |d| d == section_index) {
            // Skip DWARF relocations just yet
            let (section_id, symbol_id) = custom_section_ids.get(section_index).unwrap();
            all_relocations.push((*section_id, *symbol_id, relocations));
        }
    }

    for (section_id, symbol_id, relocations) in all_relocations.into_iter() {
        let (_symbol_id, section_offset) = obj.symbol_section_and_offset(symbol_id).unwrap();

        for r in relocations {
            let relocation_address = section_offset + r.offset as u64;

            let (relocation_kind, relocation_encoding, relocation_size) = match r.kind {
                Reloc::Abs4 => (RelocationKind::Absolute, RelocationEncoding::Generic, 32),
                Reloc::Abs8 => (RelocationKind::Absolute, RelocationEncoding::Generic, 64),
                Reloc::X86PCRel4 => (RelocationKind::Relative, RelocationEncoding::Generic, 32),
                Reloc::X86CallPCRel4 => {
                    (RelocationKind::Relative, RelocationEncoding::X86Branch, 32)
                }
                Reloc::X86CallPLTRel4 => (
                    RelocationKind::PltRelative,
                    RelocationEncoding::X86Branch,
                    32,
                ),
                Reloc::X86GOTPCRel4 => {
                    (RelocationKind::GotRelative, RelocationEncoding::Generic, 32)
                }
                Reloc::Arm64Call => (
                    match obj.format() {
                        object::BinaryFormat::Elf => RelocationKind::Elf(elf::R_AARCH64_CALL26),
                        object::BinaryFormat::MachO => RelocationKind::MachO {
                            value: macho::ARM64_RELOC_BRANCH26,
                            relative: true,
                        },
                        fmt => panic!("unsupported binary format {fmt:?}"),
                    },
                    RelocationEncoding::Generic,
                    32,
                ),
                Reloc::ElfX86_64TlsGd => (
                    RelocationKind::Elf(elf::R_X86_64_TLSGD),
                    RelocationEncoding::Generic,
                    32,
                ),
                other => {
                    return Err(ObjectError::UnsupportedArchitecture(format!(
                        "{} (relocation: {}",
                        triple.architecture, other
                    )))
                }
            };

            match r.reloc_target {
                RelocationTarget::LocalFunc(index) => {
                    let (_, target_symbol) = function_symbol_ids.get(index).unwrap();
                    obj.add_relocation(
                        section_id,
                        Relocation {
                            offset: relocation_address,
                            size: relocation_size,
                            kind: relocation_kind,
                            encoding: relocation_encoding,
                            symbol: *target_symbol,
                            addend: r.addend,
                        },
                    )
                    .map_err(ObjectError::Write)?;
                }
                RelocationTarget::LibCall(libcall) => {
                    let libcall_fn_name = libcall.to_function_name().as_bytes();
                    // We add the symols lazily as we see them
                    let target_symbol = obj.symbol_id(libcall_fn_name).unwrap_or_else(|| {
                        obj.add_symbol(ObjSymbol {
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
                    let (_, target_symbol) = custom_section_ids.get(section_index).unwrap();
                    obj.add_relocation(
                        section_id,
                        Relocation {
                            offset: relocation_address,
                            size: relocation_size,
                            kind: relocation_kind,
                            encoding: relocation_encoding,
                            symbol: *target_symbol,
                            addend: r.addend,
                        },
                    )
                    .map_err(ObjectError::Write)?;
                }
            };
        }
    }

    Ok(())
}

/// Emit the compilation result into an existing object.
///
/// # Usage
///
/// ```rust
/// # use wasmer_compiler::types::{ symbols::SymbolRegistry, target::{Triple}, function::{Compilation} };
/// # use wasmer_compiler::object::{ObjectError, get_object_for_target, emit_serialized};
///
/// # fn emit_compilation_into_object(
/// #     triple: &Triple,
/// #     compilation: Compilation,
/// #     symbol_registry: impl SymbolRegistry,
/// # ) -> Result<(), ObjectError> {
/// let bytes = &[ /* compilation bytes */];
/// let mut object = get_object_for_target(&triple)?;
/// emit_serialized(&mut object, bytes, &triple, "WASMER_MODULE")?;
/// # Ok(())
/// # }
/// ```
pub fn emit_serialized(
    obj: &mut Object,
    sercomp: &[u8],
    triple: &Triple,
    object_name: &str,
) -> Result<(), ObjectError> {
    obj.set_mangling(object::write::Mangling::None);
    //let module_name = module.compile_info.module.name.clone();
    let len_name = format!("{object_name}_LENGTH");
    let data_name = format!("{object_name}_DATA");
    //let metadata_name = "WASMER_MODULE_METADATA";

    let align = match triple.architecture {
        Architecture::X86_64 => 1,
        // In Arm64 is recommended a 4-byte alignment
        Architecture::Aarch64(_) => 4,
        _ => 1,
    };

    let len = sercomp.len();
    let section_id = obj.section_id(StandardSection::Data);
    let symbol_id = obj.add_symbol(ObjSymbol {
        name: len_name.as_bytes().to_vec(),
        value: 0,
        size: len.to_le_bytes().len() as _,
        kind: SymbolKind::Data,
        scope: SymbolScope::Dynamic,
        weak: false,
        section: SymbolSection::Section(section_id),
        flags: SymbolFlags::None,
    });
    obj.add_symbol_data(symbol_id, section_id, &len.to_le_bytes(), align);

    let section_id = obj.section_id(StandardSection::Data);
    let symbol_id = obj.add_symbol(ObjSymbol {
        name: data_name.as_bytes().to_vec(),
        value: 0,
        size: sercomp.len() as _,
        kind: SymbolKind::Data,
        scope: SymbolScope::Dynamic,
        weak: false,
        section: SymbolSection::Section(section_id),
        flags: SymbolFlags::None,
    });
    obj.add_symbol_data(symbol_id, section_id, sercomp, align);

    Ok(())
}
