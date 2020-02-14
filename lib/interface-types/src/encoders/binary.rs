//! Writes the AST into bytes representing WIT with its binary format.

use crate::{
    ast::{Adapter, AdapterKind, Export, Import, InterfaceType, Interfaces, Type},
    interpreter::Instruction,
};
use std::io::{self, Write};

/// A trait for converting a value to bytes.
trait ToBytes<W>
where
    W: Write,
{
    /// Converts the given value into `&[u8]` in the given `writer`.
    fn to_bytes(&self, writer: &mut W) -> io::Result<()>;
}

/// Encode a `u8` into a byte (well, it's already a byte!).
impl<W> ToBytes<W> for u8
where
    W: Write,
{
    fn to_bytes(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&[*self])
    }
}

/// Encode a `u64` into bytes with a LEB128 representation.
///
/// Decoder is `decoders::binary::uleb`.
impl<W> ToBytes<W> for u64
where
    W: Write,
{
    fn to_bytes(&self, writer: &mut W) -> io::Result<()> {
        let mut value = *self;

        // Code adapted from the Rust' `serialize` library.
        loop {
            if value < 0x80 {
                writer.write_all(&[value as u8])?;

                break;
            }

            writer.write_all(&[((value & 0x7f) | 0x80) as u8])?;
            value >>= 7;
        }

        Ok(())
    }
}

/// Encode a `str` into bytes.
///
/// Decoder is `decoders::binary::string`.
impl<W> ToBytes<W> for &str
where
    W: Write,
{
    fn to_bytes(&self, writer: &mut W) -> io::Result<()> {
        // Size first.
        writer.write_all(&[self.len() as u8])?;

        // Then the string.
        writer.write_all(self.as_bytes())?;

        Ok(())
    }
}

/// Encode a vector into bytes.
///
/// Decoder is `decoders::binary::list`.
impl<W, I> ToBytes<W> for Vec<I>
where
    W: Write,
    I: ToBytes<W>,
{
    fn to_bytes(&self, writer: &mut W) -> io::Result<()> {
        // Size first.
        (self.len() as u64).to_bytes(writer)?;

        // Then the items.
        for item in self {
            item.to_bytes(writer)?;
        }

        Ok(())
    }
}

/// Encode an `InterfaceType` into bytes.
impl<W> ToBytes<W> for InterfaceType
where
    W: Write,
{
    fn to_bytes(&self, writer: &mut W) -> io::Result<()> {
        match self {
            InterfaceType::Int => 0x7fff_u64.to_bytes(writer),
            InterfaceType::Float => 0x7ffe_u64.to_bytes(writer),
            InterfaceType::Any => 0x7ffd_u64.to_bytes(writer),
            InterfaceType::String => 0x7ffc_u64.to_bytes(writer),
            InterfaceType::Seq => 0x7ffb_u64.to_bytes(writer),
            InterfaceType::I32 => 0x7f_u64.to_bytes(writer),
            InterfaceType::I64 => 0x7e_u64.to_bytes(writer),
            InterfaceType::F32 => 0x7d_u64.to_bytes(writer),
            InterfaceType::F64 => 0x7d_u64.to_bytes(writer),
            InterfaceType::AnyRef => 0x6f_u64.to_bytes(writer),
        }
    }
}

/// Encode an `AdapterKind` into bytes.
impl<W> ToBytes<W> for AdapterKind
where
    W: Write,
{
    fn to_bytes(&self, writer: &mut W) -> io::Result<()> {
        match self {
            AdapterKind::Import => 0x00_u8.to_bytes(writer),
            AdapterKind::Export => 0x01_u8.to_bytes(writer),
            AdapterKind::HelperFunction => 0x02_u8.to_bytes(writer),
        }
    }
}

/// Encode an `Export` into bytes.
///
/// Decoder is in `decoders::binary::exports`.
impl<W> ToBytes<W> for Export<'_>
where
    W: Write,
{
    fn to_bytes(&self, writer: &mut W) -> io::Result<()> {
        self.name.to_bytes(writer)?;
        self.input_types.to_bytes(writer)?;
        self.output_types.to_bytes(writer)?;

        Ok(())
    }
}

/// Encode a `Type` into bytes.
///
/// Decoder is in `decoders::binary::types`.
impl<W> ToBytes<W> for Type<'_>
where
    W: Write,
{
    fn to_bytes(&self, writer: &mut W) -> io::Result<()> {
        self.name.to_bytes(writer)?;
        self.field_names().to_bytes(writer)?;
        self.field_types().to_bytes(writer)?;

        Ok(())
    }
}

/// Encode an `Import` into bytes.
///
/// Decoder is in `decoders::binary::imports`.
impl<W> ToBytes<W> for Import<'_>
where
    W: Write,
{
    fn to_bytes(&self, writer: &mut W) -> io::Result<()> {
        self.namespace.to_bytes(writer)?;
        self.name.to_bytes(writer)?;
        self.input_types.to_bytes(writer)?;
        self.output_types.to_bytes(writer)?;

        Ok(())
    }
}

/// Encode an `Adapter` into bytes.
///
/// Decoder is in `decoders::binary::imports`.
impl<W> ToBytes<W> for Adapter<'_>
where
    W: Write,
{
    fn to_bytes(&self, writer: &mut W) -> io::Result<()> {
        match self {
            Adapter::Import {
                namespace,
                name,
                input_types,
                output_types,
                instructions,
            } => {
                AdapterKind::Import.to_bytes(writer)?;
                namespace.to_bytes(writer)?;
                name.to_bytes(writer)?;
                input_types.to_bytes(writer)?;
                output_types.to_bytes(writer)?;
                instructions.to_bytes(writer)?;
            }

            Adapter::Export {
                name,
                input_types,
                output_types,
                instructions,
            } => {
                AdapterKind::Export.to_bytes(writer)?;
                name.to_bytes(writer)?;
                input_types.to_bytes(writer)?;
                output_types.to_bytes(writer)?;
                instructions.to_bytes(writer)?;
            }

            Adapter::HelperFunction {
                name,
                input_types,
                output_types,
                instructions,
            } => {
                AdapterKind::HelperFunction.to_bytes(writer)?;
                name.to_bytes(writer)?;
                input_types.to_bytes(writer)?;
                output_types.to_bytes(writer)?;
                instructions.to_bytes(writer)?;
            }
        }

        Ok(())
    }
}

/// Encode an `Interfaces` into bytes.
///
/// Decoder is `decoders::binary::parse`.
impl<W> ToBytes<W> for Interfaces<'_>
where
    W: Write,
{
    fn to_bytes(&self, writer: &mut W) -> io::Result<()> {
        self.exports.to_bytes(writer)?;
        self.types.to_bytes(writer)?;
        self.imports.to_bytes(writer)?;
        self.adapters.to_bytes(writer)?;

        Ok(())
    }
}

/// Encode an `Instruction` into bytes.
///
/// Decoder is `decoders::binary::instruction`.
impl<W> ToBytes<W> for Instruction<'_>
where
    W: Write,
{
    fn to_bytes(&self, writer: &mut W) -> io::Result<()> {
        match self {
            Instruction::ArgumentGet { index } => {
                0x00_u8.to_bytes(writer)?;
                index.to_bytes(writer)?;
            }

            Instruction::Call { function_index } => {
                0x01_u8.to_bytes(writer)?;
                (*function_index as u64).to_bytes(writer)?;
            }

            Instruction::CallExport { export_name } => {
                0x02_u8.to_bytes(writer)?;
                export_name.to_bytes(writer)?;
            }

            Instruction::ReadUtf8 => 0x03_u8.to_bytes(writer)?,

            Instruction::WriteUtf8 { allocator_name } => {
                0x04_u8.to_bytes(writer)?;
                allocator_name.to_bytes(writer)?;
            }

            Instruction::AsWasm(interface_type) => {
                0x05_u8.to_bytes(writer)?;
                interface_type.to_bytes(writer)?;
            }

            Instruction::AsInterface(interface_type) => {
                0x06_u8.to_bytes(writer)?;
                interface_type.to_bytes(writer)?;
            }

            Instruction::TableRefAdd => 0x07_u8.to_bytes(writer)?,

            Instruction::TableRefGet => 0x08_u8.to_bytes(writer)?,

            Instruction::CallMethod(function_index) => {
                0x09_u8.to_bytes(writer)?;
                function_index.to_bytes(writer)?;
            }

            Instruction::MakeRecord(interface_type) => {
                0x0a_u8.to_bytes(writer)?;
                interface_type.to_bytes(writer)?;
            }

            Instruction::GetField(interface_type, field_index) => {
                0x0c_u8.to_bytes(writer)?;
                interface_type.to_bytes(writer)?;
                field_index.to_bytes(writer)?;
            }

            Instruction::Const(interface_type, index) => {
                0x0d_u8.to_bytes(writer)?;
                interface_type.to_bytes(writer)?;
                index.to_bytes(writer)?;
            }

            Instruction::FoldSeq(index) => {
                0x0e_u8.to_bytes(writer)?;
                index.to_bytes(writer)?;
            }

            Instruction::Add(interface_type) => {
                0x0f_u8.to_bytes(writer)?;
                interface_type.to_bytes(writer)?;
            }

            Instruction::MemToSeq(interface_type, string) => {
                0x10_u8.to_bytes(writer)?;
                interface_type.to_bytes(writer)?;
                string.to_bytes(writer)?;
            }

            Instruction::Load(interface_type, string) => {
                0x11_u8.to_bytes(writer)?;
                interface_type.to_bytes(writer)?;
                string.to_bytes(writer)?;
            }

            Instruction::SeqNew(interface_type) => {
                0x12_u8.to_bytes(writer)?;
                interface_type.to_bytes(writer)?;
            }

            Instruction::ListPush => 0x13_u8.to_bytes(writer)?,

            Instruction::RepeatUntil(index1, index2) => {
                0x14_u8.to_bytes(writer)?;
                index1.to_bytes(writer)?;
                index2.to_bytes(writer)?;
            }
        }

        Ok(())
    }
}
