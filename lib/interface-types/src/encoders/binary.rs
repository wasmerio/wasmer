//! Writes the AST into bytes representing WIT with its binary format.

use crate::{ast::*, interpreter::Instruction};
use std::io::{self, Write};

/// A trait for converting a value to bytes.
pub trait ToBytes<W>
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
            InterfaceType::S8 => 0x00_u8.to_bytes(writer),
            InterfaceType::S16 => 0x01_u8.to_bytes(writer),
            InterfaceType::S32 => 0x02_u8.to_bytes(writer),
            InterfaceType::S64 => 0x03_u8.to_bytes(writer),
            InterfaceType::U8 => 0x04_u8.to_bytes(writer),
            InterfaceType::U16 => 0x05_u8.to_bytes(writer),
            InterfaceType::U32 => 0x06_u8.to_bytes(writer),
            InterfaceType::U64 => 0x07_u8.to_bytes(writer),
            InterfaceType::F32 => 0x08_u8.to_bytes(writer),
            InterfaceType::F64 => 0x09_u8.to_bytes(writer),
            InterfaceType::String => 0x0a_u8.to_bytes(writer),
            InterfaceType::Anyref => 0x0b_u8.to_bytes(writer),
            InterfaceType::I32 => 0x0c_u8.to_bytes(writer),
            InterfaceType::I64 => 0x0d_u8.to_bytes(writer),
        }
    }
}

/// Encode an `InterfaceKind` into bytes.
impl<W> ToBytes<W> for InterfaceKind
where
    W: Write,
{
    fn to_bytes(&self, writer: &mut W) -> io::Result<()> {
        match self {
            Self::Type => 0x00_u8.to_bytes(writer),
            Self::Import => 0x01_u8.to_bytes(writer),
            Self::Adapter => 0x02_u8.to_bytes(writer),
            Self::Export => 0x03_u8.to_bytes(writer),
            Self::Implementation => 0x04_u8.to_bytes(writer),
        }
    }
}

/// Encode a `Type` into bytes.
///
/// Decoder is in `decoders::binary::types`.
impl<W> ToBytes<W> for Type
where
    W: Write,
{
    fn to_bytes(&self, writer: &mut W) -> io::Result<()> {
        self.inputs.to_bytes(writer)?;
        self.outputs.to_bytes(writer)?;

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
        (self.signature_type as u64).to_bytes(writer)?;

        Ok(())
    }
}

/// Encode an `Adapter` into bytes.
///
/// Decoder is in `decoders::binary::adapters`.
impl<W> ToBytes<W> for Adapter
where
    W: Write,
{
    fn to_bytes(&self, writer: &mut W) -> io::Result<()> {
        (self.function_type as u64).to_bytes(writer)?;
        self.instructions.to_bytes(writer)?;

        Ok(())
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
        (self.function_type as u64).to_bytes(writer)?;

        Ok(())
    }
}

/// Encode an `Implementation` into bytes.
///
/// Decoder is in `decoders::binary::implementations`.
impl<W> ToBytes<W> for Implementation
where
    W: Write,
{
    fn to_bytes(&self, writer: &mut W) -> io::Result<()> {
        (self.core_function_type as u64).to_bytes(writer)?;
        (self.adapter_function_type as u64).to_bytes(writer)?;

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
        if !self.types.is_empty() {
            InterfaceKind::Type.to_bytes(writer)?;
            self.types.to_bytes(writer)?;
        }

        if !self.imports.is_empty() {
            InterfaceKind::Import.to_bytes(writer)?;
            self.imports.to_bytes(writer)?;
        }

        if !self.adapters.is_empty() {
            InterfaceKind::Adapter.to_bytes(writer)?;
            self.adapters.to_bytes(writer)?;
        }

        if !self.exports.is_empty() {
            InterfaceKind::Export.to_bytes(writer)?;
            self.exports.to_bytes(writer)?;
        }

        if !self.implementations.is_empty() {
            InterfaceKind::Implementation.to_bytes(writer)?;
            self.implementations.to_bytes(writer)?;
        }

        Ok(())
    }
}

/// Encode an `Instruction` into bytes.
///
/// Decoder is `decoders::binary::instruction`.
impl<W> ToBytes<W> for Instruction
where
    W: Write,
{
    fn to_bytes(&self, writer: &mut W) -> io::Result<()> {
        match self {
            Instruction::ArgumentGet { index } => {
                0x00_u8.to_bytes(writer)?;
                (*index as u64).to_bytes(writer)?;
            }

            Instruction::CallCore { function_index } => {
                0x01_u8.to_bytes(writer)?;
                (*function_index as u64).to_bytes(writer)?;
            }

            Instruction::S8FromI32 => 0x02_u8.to_bytes(writer)?,
            Instruction::S8FromI64 => 0x03_u8.to_bytes(writer)?,
            Instruction::S16FromI32 => 0x04_u8.to_bytes(writer)?,
            Instruction::S16FromI64 => 0x05_u8.to_bytes(writer)?,
            Instruction::S32FromI32 => 0x06_u8.to_bytes(writer)?,
            Instruction::S32FromI64 => 0x07_u8.to_bytes(writer)?,
            Instruction::S64FromI32 => 0x08_u8.to_bytes(writer)?,
            Instruction::S64FromI64 => 0x09_u8.to_bytes(writer)?,
            Instruction::I32FromS8 => 0x0a_u8.to_bytes(writer)?,
            Instruction::I32FromS16 => 0x0b_u8.to_bytes(writer)?,
            Instruction::I32FromS32 => 0x0c_u8.to_bytes(writer)?,
            Instruction::I32FromS64 => 0x0d_u8.to_bytes(writer)?,
            Instruction::I64FromS8 => 0x0e_u8.to_bytes(writer)?,
            Instruction::I64FromS16 => 0x0f_u8.to_bytes(writer)?,
            Instruction::I64FromS32 => 0x10_u8.to_bytes(writer)?,
            Instruction::I64FromS64 => 0x11_u8.to_bytes(writer)?,
            Instruction::U8FromI32 => 0x12_u8.to_bytes(writer)?,
            Instruction::U8FromI64 => 0x13_u8.to_bytes(writer)?,
            Instruction::U16FromI32 => 0x14_u8.to_bytes(writer)?,
            Instruction::U16FromI64 => 0x15_u8.to_bytes(writer)?,
            Instruction::U32FromI32 => 0x16_u8.to_bytes(writer)?,
            Instruction::U32FromI64 => 0x17_u8.to_bytes(writer)?,
            Instruction::U64FromI32 => 0x18_u8.to_bytes(writer)?,
            Instruction::U64FromI64 => 0x19_u8.to_bytes(writer)?,
            Instruction::I32FromU8 => 0x1a_u8.to_bytes(writer)?,
            Instruction::I32FromU16 => 0x1b_u8.to_bytes(writer)?,
            Instruction::I32FromU32 => 0x1c_u8.to_bytes(writer)?,
            Instruction::I32FromU64 => 0x1d_u8.to_bytes(writer)?,
            Instruction::I64FromU8 => 0x1e_u8.to_bytes(writer)?,
            Instruction::I64FromU16 => 0x1f_u8.to_bytes(writer)?,
            Instruction::I64FromU32 => 0x20_u8.to_bytes(writer)?,
            Instruction::I64FromU64 => 0x21_u8.to_bytes(writer)?,

            Instruction::StringLiftMemory => 0x22_u8.to_bytes(writer)?,

            Instruction::StringLowerMemory { allocator_index } => {
                0x23_u8.to_bytes(writer)?;
                (*allocator_index as u64).to_bytes(writer)?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! assert_to_bytes {
        ($expr:expr, $expected_output:expr) => {{
            let mut output = vec![];

            $expr.to_bytes(&mut output).expect(concat!(
                "Unable to encode the expression `",
                stringify!($expr),
                "` to bytes."
            ));

            assert_eq!(output.as_slice(), &$expected_output[..]);
        }};
    }

    #[test]
    fn test_u8() {
        assert_to_bytes!(0x01_u8, &[0x01]);
    }

    #[test]
    fn test_uleb_1_byte() {
        assert_to_bytes!(0x01_u64, &[0x01]);
    }

    #[test]
    fn test_uleb_3_bytes() {
        assert_to_bytes!(0x7ffc_u64, &[0xfc, 0xff, 0x01]);
    }

    // Examples from Figure 22 of [DWARF 4
    // standard](http://dwarfstd.org/doc/DWARF4.pdf).
    #[test]
    fn test_uleb_from_dward_standard() {
        assert_to_bytes!(2u64, &[2u8]);
        assert_to_bytes!(127u64, &[127u8]);
        assert_to_bytes!(128u64, &[0x80, 1u8]);
        assert_to_bytes!(129u64, &[1u8 | 0x80, 1]);
        assert_to_bytes!(130u64, &[2u8 | 0x80, 1]);
        assert_to_bytes!(12857u64, &[57u8 | 0x80, 100]);
    }

    #[test]
    fn test_empty_str() {
        assert_to_bytes!("", &[0x00]);
    }

    #[test]
    fn test_str() {
        assert_to_bytes!("abc", &[0x03, 0x61, 0x62, 0x63]);
    }

    #[test]
    fn test_empty_vec() {
        assert_to_bytes!(Vec::<u8>::new(), &[0x00]);
    }

    #[test]
    fn test_vec() {
        assert_to_bytes!(
            vec!["a", "b", "c"],
            &[
                0x03, // list of 3 items
                0x01, // string of 1 byte
                0x61, // "a"
                0x01, // string of 1 byte
                0x62, // "b"
                0x01, // string of 1 byte
                0x63, // "c"
            ]
        );
    }

    #[test]
    fn test_interface_type() {
        assert_to_bytes!(InterfaceType::S8, &[0x00]);
        assert_to_bytes!(InterfaceType::S16, &[0x01]);
        assert_to_bytes!(InterfaceType::S32, &[0x02]);
        assert_to_bytes!(InterfaceType::S64, &[0x03]);
        assert_to_bytes!(InterfaceType::U8, &[0x04]);
        assert_to_bytes!(InterfaceType::U16, &[0x05]);
        assert_to_bytes!(InterfaceType::U32, &[0x06]);
        assert_to_bytes!(InterfaceType::U64, &[0x07]);
        assert_to_bytes!(InterfaceType::F32, &[0x08]);
        assert_to_bytes!(InterfaceType::F64, &[0x09]);
        assert_to_bytes!(InterfaceType::String, &[0x0a]);
        assert_to_bytes!(InterfaceType::Anyref, &[0x0b]);
        assert_to_bytes!(InterfaceType::I32, &[0x0c]);
        assert_to_bytes!(InterfaceType::I64, &[0x0d]);
    }

    #[test]
    fn test_interface_kind() {
        assert_to_bytes!(InterfaceKind::Type, &[0x00]);
        assert_to_bytes!(InterfaceKind::Import, &[0x01]);
        assert_to_bytes!(InterfaceKind::Adapter, &[0x02]);
        assert_to_bytes!(InterfaceKind::Export, &[0x03]);
        assert_to_bytes!(InterfaceKind::Implementation, &[0x04]);
    }

    #[test]
    fn test_export() {
        assert_to_bytes!(
            Export {
                name: "abc",
                function_type: 0,
            },
            &[
                0x03, // string of length 3
                0x61, // "a"
                0x62, // "b"
                0x63, // "c"
                0x00, // function type
            ]
        );
    }

    #[test]
    fn test_type() {
        assert_to_bytes!(
            Type {
                inputs: vec![InterfaceType::I32, InterfaceType::I64],
                outputs: vec![InterfaceType::S32],
            },
            &[
                0x02, // list of 2 items
                0x0c, // I32
                0x0d, // I64
                0x01, // list of 1 items
                0x02, // I64
            ]
        );
    }

    #[test]
    fn test_import() {
        assert_to_bytes!(
            Import {
                namespace: "a",
                name: "b",
                signature_type: 0,
            },
            &[
                0x01, // string of length 1
                0x61, // "a"
                0x01, // string of length 1
                0x62, // "b"
                0x00, // signature typr
            ]
        );
    }

    #[test]
    fn test_adapter() {
        assert_to_bytes!(
            Adapter {
                function_type: 0,
                instructions: vec![Instruction::ArgumentGet { index: 1 }],
            },
            &[
                0x00, // function type
                0x01, // list of 1 item
                0x00, 0x01, // ArgumentGet { index: 1 }
            ]
        );
    }

    #[test]
    fn test_interfaces() {
        assert_to_bytes!(
            Interfaces {
                types: vec![Type {
                    inputs: vec![InterfaceType::S8],
                    outputs: vec![InterfaceType::S16],
                }],
                imports: vec![Import {
                    namespace: "ab",
                    name: "c",
                    signature_type: 0,
                }],
                adapters: vec![Adapter {
                    function_type: 0,
                    instructions: vec![Instruction::ArgumentGet { index: 1 }],
                }],
                exports: vec![Export {
                    name: "ab",
                    function_type: 1,
                }],
                implementations: vec![Implementation {
                    core_function_type: 2,
                    adapter_function_type: 3,
                }],
            },
            &[
                0x00, // type section
                0x01, // 1 type
                0x01, // list of 1 item
                0x00, // S8
                0x01, // list of 1 item
                0x01, // S16
                //
                0x01, // import section
                0x01, // 1 import
                0x02, // string of 2 bytes
                0x61, 0x62, // "a", "b"
                0x01, // string of 1 byte
                0x63, // "c"
                0x00, // signature type
                //
                0x02, // adapter section
                0x01, // 1 adapter
                0x00, // function type
                0x01, // list of 1 item
                0x00, 0x01, // ArgumentGet { index: 1 }
                //
                0x03, // export section
                0x01, // 1 export
                0x02, // string of 2 bytes
                0x61, 0x62, // "a", "b"
                0x01, // function type
                //
                0x04, // implementation section
                0x01, // 1 implementation
                0x02, // core function type
                0x03, // adapter function type
            ]
        );
    }

    #[test]
    fn test_instructions() {
        assert_to_bytes!(
            vec![
                Instruction::ArgumentGet { index: 1 },
                Instruction::CallCore { function_index: 1 },
                Instruction::S8FromI32,
                Instruction::S8FromI64,
                Instruction::S16FromI32,
                Instruction::S16FromI64,
                Instruction::S32FromI32,
                Instruction::S32FromI64,
                Instruction::S64FromI32,
                Instruction::S64FromI64,
                Instruction::I32FromS8,
                Instruction::I32FromS16,
                Instruction::I32FromS32,
                Instruction::I32FromS64,
                Instruction::I64FromS8,
                Instruction::I64FromS16,
                Instruction::I64FromS32,
                Instruction::I64FromS64,
                Instruction::U8FromI32,
                Instruction::U8FromI64,
                Instruction::U16FromI32,
                Instruction::U16FromI64,
                Instruction::U32FromI32,
                Instruction::U32FromI64,
                Instruction::U64FromI32,
                Instruction::U64FromI64,
                Instruction::I32FromU8,
                Instruction::I32FromU16,
                Instruction::I32FromU32,
                Instruction::I32FromU64,
                Instruction::I64FromU8,
                Instruction::I64FromU16,
                Instruction::I64FromU32,
                Instruction::I64FromU64,
                Instruction::StringLiftMemory,
                Instruction::StringLowerMemory { allocator_index: 1 },
            ],
            &[
                0x24, // list of 36 items
                0x00, 0x01, // ArgumentGet { index: 1 }
                0x01, 0x01, // CallCore { function_index: 1 }
                0x02, // S8FromI32
                0x03, // S8FromI64
                0x04, // S16FromI32
                0x05, // S16FromI64
                0x06, // S32FromI32
                0x07, // S32FromI64
                0x08, // S64FromI32
                0x09, // S64FromI64
                0x0a, // I32FromS8
                0x0b, // I32FromS16
                0x0c, // I32FromS32
                0x0d, // I32FromS64
                0x0e, // I64FromS8
                0x0f, // I64FromS16
                0x10, // I64FromS32
                0x11, // I64FromS64
                0x12, // U8FromI32
                0x13, // U8FromI64
                0x14, // U16FromI32
                0x15, // U16FromI64
                0x16, // U32FromI32
                0x17, // U32FromI64
                0x18, // U64FromI32
                0x19, // U64FromI64
                0x1a, // I32FromU8
                0x1b, // I32FromU16
                0x1c, // I32FromU32
                0x1d, // I32FromU64
                0x1e, // I64FromU8
                0x1f, // I64FromU16
                0x20, // I64FromU32
                0x21, // I64FromU64
                0x22, // StringLiftMemory
                0x23, 0x01, // StringLowerMemory { allocator_index: 1 }
            ]
        );
    }
}
