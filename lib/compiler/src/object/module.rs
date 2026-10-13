use super::error::ObjectError;
use object::{FileFlags, elf, write::Object};
use wasmer_types::target::{Architecture, BinaryFormat, Endianness, Triple};

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
            os_abi: elf::OsAbi(2),
            abi_version: 0,
        };
    }

    Ok(object)
}
