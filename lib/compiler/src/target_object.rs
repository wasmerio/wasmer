//! Target object formats shared by compilation and artifact loading.

use object::write::{Object, Symbol as ObjSymbol, SymbolSection};
use object::{FileFlags, SymbolFlags, SymbolKind, SymbolScope, elf};
use target_lexicon::{Architecture, BinaryFormat, Endianness, Triple};
use thiserror::Error;

/// The Object error can occur when creating an object file
/// from a `Compilation`.
#[derive(Error, Debug)]
pub enum ObjectError {
    /// The object was provided a not-supported binary format
    #[error("Binary format {0} not supported")]
    UnsupportedBinaryFormat(String),
    /// The object was provided a not-supported architecture
    #[error("Architecture {0} not supported")]
    UnsupportedArchitecture(String),
    /// The object was provided an unknown endianness
    #[error("Unknown Endianness")]
    UnknownEndianness,
    /// The module provided could not be serialized into bytes
    #[error("Error when serializing the given module: {0}")]
    Serialize(#[from] wasmer_types::SerializeError),
}

/// Create an object for a given target `Triple`.
///
/// # Usage
///
/// ```rust
/// # use wasmer_types::target::Triple;
/// # use wasmer_compiler::{ObjectError, get_object_for_target};
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
    // TODO: remove with a new libwild release
    if obj_binary_format == object::BinaryFormat::MachO {
        // libwild 0.11 skips symbol zero as if Mach-O had an ELF null symbol.
        // Keep an unreferenced local first: https://github.com/wild-linker/wild/pull/2845
        object.add_symbol(ObjSymbol {
            name: b"__wasmer_dummy".to_vec(),
            value: 0,
            size: 0,
            kind: SymbolKind::Data,
            scope: SymbolScope::Compilation,
            weak: false,
            section: SymbolSection::Absolute,
            flags: SymbolFlags::None,
        });
    }

    if let Architecture::Riscv64(_) = triple.architecture {
        object.flags = FileFlags::Elf {
            e_flags: elf::EF_RISCV_FLOAT_ABI_DOUBLE,
            os_abi: elf::OsAbi(2),
            abi_version: 0,
        };
    }

    Ok(object)
}
