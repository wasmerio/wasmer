use super::error::ObjectError;
use crate::{
    serialize::MetadataHeader,
    types::{
        function::Compilation,
        relocation::{RelocationKind as Reloc, RelocationTarget},
        section::{CustomSectionProtection, SectionIndex},
        symbols::{ModuleMetadata, Symbol, SymbolRegistry},
    },
};
use object::{
    FileFlags, RelocationEncoding, RelocationFlags, RelocationKind, SectionKind, SymbolFlags,
    SymbolKind, SymbolScope, elf, macho,
    write::{
        Object, Relocation, StandardSection, StandardSegment, Symbol as ObjSymbol, SymbolId,
        SymbolSection,
    },
};
use wasmer_types::LocalFunctionIndex;
use wasmer_types::entity::{EntityRef, PrimaryMap};
use wasmer_types::target::{Architecture, BinaryFormat, Endianness, PointerWidth, Triple};

const DWARF_SECTION_NAME: &[u8] = b".eh_frame";

/// Create an object for a given target `Triple`.
///
/// # Usage
///
/// ```rust
/// # use wasmer_types::target::Triple;
/// # use wasmer_compiler::object::{ObjectError, get_object_for_target};
///
/// # fn generate_object_for_target(triple: &Triple) -> Result<(), ObjectError> {
/// let mut object = get_object_for_target(&triple)?;
///
/// # Ok(())
/// # }
/// ```
pub fn get_object_for_target(triple: &Triple) -> Result<Object<'static>, ObjectError> {
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
        Architecture::Riscv32(_) => object::Architecture::Riscv32,
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
/// # use wasmer_types::target::Triple;
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
) -> Result<u64, ObjectError> {
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
    let offset = obj.add_symbol_data(symbol_id, section_id, data, align);

    Ok(offset)
}

/// Emit the compilation result into an existing object.
///
/// # Usage
///
/// ```rust
/// # use wasmer_compiler::types::{ symbols::SymbolRegistry, function::{Compilation} };
/// # use wasmer_types::target::Triple;
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

/// ObjectMetadataBuilder builds serialized module metadata include in
/// an object. In addition, it also relies on information from ModuleInfo
/// to build a table of function pointers, trmampolines and dynamic function
/// trampoline pointers. ObjectMetadataBuilder takes care of setting up
/// relocations, so a linker can automatically fill in actual addresses of
/// all relevant functions. There is no need to piece the information together
/// in the glue C file.
pub struct ObjectMetadataBuilder {
    placeholder_data: Vec<u8>,
    metadata_length: u64,
    section_offset: u64,
    num_function_pointers: u64,
    num_trampolines: u64,
    num_dynamic_function_trampoline_pointers: u64,
    endianness: Endianness,
    pointer_width: PointerWidth,
}

impl ObjectMetadataBuilder {
    /// Creates a new FunctionRelocsBuilder
    pub fn new(metadata: &ModuleMetadata, triple: &Triple) -> Result<Self, ObjectError> {
        let serialized_data = metadata.serialize()?;
        let mut metadata_binary = vec![];
        metadata_binary.extend(MetadataHeader::new(serialized_data.len()).into_bytes());
        metadata_binary.extend(serialized_data);
        let metadata_length = metadata_binary.len() as u64;

        let pointer_width = triple.pointer_width().unwrap();
        let endianness = triple
            .endianness()
            .map_err(|_| ObjectError::UnknownEndianness)?;

        let module = &metadata.compile_info.module;
        let num_function_pointers = module
            .functions
            .iter()
            .filter(|(f_index, _)| module.local_func_index(*f_index).is_some())
            .count() as u64;
        let num_trampolines = module.signatures.len() as u64;
        let num_dynamic_function_trampoline_pointers = module.num_imported_functions as u64;

        let mut aself = Self {
            placeholder_data: metadata_binary,
            metadata_length,
            section_offset: 0,
            num_function_pointers,
            num_trampolines,
            num_dynamic_function_trampoline_pointers,
            endianness,
            pointer_width,
        };

        aself
            .placeholder_data
            .extend_from_slice(&aself.serialize_value(aself.num_function_pointers));
        aself.placeholder_data.extend_from_slice(&vec![
            0u8;
            (aself.pointer_bytes() * aself.num_function_pointers)
                as usize
        ]);
        aself
            .placeholder_data
            .extend_from_slice(&aself.serialize_value(aself.num_trampolines));
        aself.placeholder_data.extend_from_slice(&vec![
            0u8;
            (aself.pointer_bytes() * aself.num_trampolines)
                as usize
        ]);
        aself.placeholder_data.extend_from_slice(
            &aself.serialize_value(aself.num_dynamic_function_trampoline_pointers),
        );
        aself.placeholder_data.extend_from_slice(&vec![
            0u8;
            (aself.pointer_bytes() * aself.num_dynamic_function_trampoline_pointers)
                as usize
        ]);

        Ok(aself)
    }

    /// Sets section offset used in relocations
    pub fn set_section_offset(&mut self, offset: u64) {
        self.section_offset = offset;
    }

    /// Placeholder data for emit_data call
    pub fn placeholder_data(&self) -> &[u8] {
        &self.placeholder_data
    }

    /// Bytes of a pointer for target architecture
    pub fn pointer_bytes(&self) -> u64 {
        self.pointer_width.bytes() as u64
    }

    /// Sets up relocation for a function pointer
    pub fn setup_function_pointer(
        &self,
        obj: &mut Object,
        index: usize,
        symbol_id: SymbolId,
    ) -> Result<(), ObjectError> {
        let section_id = obj.section_id(StandardSection::Data);
        obj.add_relocation(
            section_id,
            Relocation {
                offset: self.function_pointers_start_offset()
                    + self.pointer_bytes() * (index as u64),
                flags: RelocationFlags::Generic {
                    kind: RelocationKind::Absolute,
                    encoding: RelocationEncoding::Generic,
                    size: self.pointer_width.bits(),
                },
                symbol: symbol_id,
                addend: 0,
            },
        )
        .map_err(ObjectError::Write)
    }

    /// Sets up relocation for a trampoline
    pub fn setup_trampoline(
        &self,
        obj: &mut Object,
        index: usize,
        symbol_id: SymbolId,
    ) -> Result<(), ObjectError> {
        let section_id = obj.section_id(StandardSection::Data);
        obj.add_relocation(
            section_id,
            Relocation {
                offset: self.trampolines_start_offset() + self.pointer_bytes() * (index as u64),
                flags: RelocationFlags::Generic {
                    kind: RelocationKind::Absolute,
                    encoding: RelocationEncoding::Generic,
                    size: self.pointer_width.bits(),
                },
                symbol: symbol_id,
                addend: 0,
            },
        )
        .map_err(ObjectError::Write)
    }

    /// Sets up relocation for a dynamic function trampoline pointer
    pub fn setup_dynamic_function_trampoline_pointer(
        &self,
        obj: &mut Object,
        index: usize,
        symbol_id: SymbolId,
    ) -> Result<(), ObjectError> {
        let section_id = obj.section_id(StandardSection::Data);
        obj.add_relocation(
            section_id,
            Relocation {
                offset: self.dynamic_function_trampoline_pointers_start_offset()
                    + self.pointer_bytes() * (index as u64),
                flags: RelocationFlags::Generic {
                    kind: RelocationKind::Absolute,
                    encoding: RelocationEncoding::Generic,
                    size: self.pointer_width.bits(),
                },
                symbol: symbol_id,
                addend: 0,
            },
        )
        .map_err(ObjectError::Write)
    }

    fn function_pointers_start_offset(&self) -> u64 {
        self.section_offset + self.metadata_length + self.pointer_bytes()
    }

    fn trampolines_start_offset(&self) -> u64 {
        self.function_pointers_start_offset()
            + self.pointer_bytes() * self.num_function_pointers
            + self.pointer_bytes()
    }

    fn dynamic_function_trampoline_pointers_start_offset(&self) -> u64 {
        self.trampolines_start_offset()
            + self.pointer_bytes() * self.num_trampolines
            + self.pointer_bytes()
    }

    fn serialize_value(&self, value: u64) -> Vec<u8> {
        match (self.endianness, self.pointer_width) {
            (Endianness::Little, PointerWidth::U16) => (value as u16).to_le_bytes().to_vec(),
            (Endianness::Big, PointerWidth::U16) => (value as u16).to_be_bytes().to_vec(),
            (Endianness::Little, PointerWidth::U32) => (value as u32).to_le_bytes().to_vec(),
            (Endianness::Big, PointerWidth::U32) => (value as u32).to_be_bytes().to_vec(),
            (Endianness::Little, PointerWidth::U64) => value.to_le_bytes().to_vec(),
            (Endianness::Big, PointerWidth::U64) => value.to_be_bytes().to_vec(),
        }
    }
}
