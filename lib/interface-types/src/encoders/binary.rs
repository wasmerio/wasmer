//! Writes the AST into bytes representing WIT with its binary format.

use crate::{
    ast::{Adapter, AdapterKind, Export, Forward, Import, InterfaceType, Interfaces, Type},
    interpreter::Instruction,
};
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
            InterfaceType::Int => 0x7fff_u64.to_bytes(writer),
            InterfaceType::Float => 0x7ffe_u64.to_bytes(writer),
            InterfaceType::Any => 0x7ffd_u64.to_bytes(writer),
            InterfaceType::String => 0x7ffc_u64.to_bytes(writer),
            InterfaceType::Seq => 0x7ffb_u64.to_bytes(writer),
            InterfaceType::I32 => 0x7f_u64.to_bytes(writer),
            InterfaceType::I64 => 0x7e_u64.to_bytes(writer),
            InterfaceType::F32 => 0x7d_u64.to_bytes(writer),
            InterfaceType::F64 => 0x7c_u64.to_bytes(writer),
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

/// Encode an `Forward` into bytes.
///
/// Decoder is `decoders::binary::forwards`.
impl<W> ToBytes<W> for Forward<'_>
where
    W: Write,
{
    fn to_bytes(&self, writer: &mut W) -> io::Result<()> {
        self.name.to_bytes(writer)
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
        self.forwards.to_bytes(writer)?;

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
        assert_to_bytes!(InterfaceType::Int, &[0xff, 0xff, 0x01]);
        assert_to_bytes!(InterfaceType::Float, &[0xfe, 0xff, 0x01]);
        assert_to_bytes!(InterfaceType::Any, &[0xfd, 0xff, 0x01]);
        assert_to_bytes!(InterfaceType::String, &[0xfc, 0xff, 0x01]);
        assert_to_bytes!(InterfaceType::Seq, &[0xfb, 0xff, 0x01]);
        assert_to_bytes!(InterfaceType::I32, &[0x7f]);
        assert_to_bytes!(InterfaceType::I64, &[0x7e]);
        assert_to_bytes!(InterfaceType::F32, &[0x7d]);
        assert_to_bytes!(InterfaceType::F64, &[0x7c]);
        assert_to_bytes!(InterfaceType::AnyRef, &[0x6f]);
    }

    #[test]
    fn test_adapter_kind() {
        assert_to_bytes!(AdapterKind::Import, &[0x00]);
        assert_to_bytes!(AdapterKind::Export, &[0x01]);
        assert_to_bytes!(AdapterKind::HelperFunction, &[0x02]);
    }

    #[test]
    fn test_export() {
        assert_to_bytes!(
            Export {
                name: "abc",
                input_types: vec![InterfaceType::I32, InterfaceType::I64],
                output_types: vec![InterfaceType::I32]
            },
            &[
                0x03, // string of length 3
                0x61, // "a"
                0x62, // "b"
                0x63, // "c"
                0x02, // list of 2 items
                0x7f, // I32
                0x7e, // I64
                0x01, // list of 1 items
                0x7f, // I32
            ]
        );
    }

    #[test]
    fn test_type() {
        assert_to_bytes!(
            Type::new(
                "a",
                vec!["b", "c"],
                vec![InterfaceType::I32, InterfaceType::I64],
            ),
            &[
                0x01, // string of length 1
                0x61, // "a"
                0x02, // list of 2 items
                0x01, // string of length 1
                0x62, // "b"
                0x01, // string of length 1
                0x63, // "c"
                0x02, // list of 2 items
                0x7f, // I32
                0x7e, // I64
            ]
        );
    }

    #[test]
    fn test_import() {
        assert_to_bytes!(
            Import {
                namespace: "a",
                name: "b",
                input_types: vec![InterfaceType::I32, InterfaceType::I64],
                output_types: vec![InterfaceType::I32],
            },
            &[
                0x01, // string of length 1
                0x61, // "a"
                0x01, // string of length 1
                0x62, // "b"
                0x02, // list of 2 items
                0x7f, // I32
                0x7e, // I64
                0x01, // list of 1 items
                0x7f, // I32
            ]
        );
    }

    #[test]
    fn test_adapter_import() {
        assert_to_bytes!(
            Adapter::Import {
                namespace: "a",
                name: "b",
                input_types: vec![InterfaceType::I32, InterfaceType::I64],
                output_types: vec![InterfaceType::I32],
                instructions: vec![Instruction::ArgumentGet { index: 1 }],
            },
            &[
                0x00, // AdapterKind::Import
                0x01, // string of length 1
                0x61, // "a"
                0x01, // string of length 1
                0x62, // "b"
                0x02, // list of 2 items
                0x7f, // I32
                0x7e, // I64
                0x01, // list of 1 items
                0x7f, // I32
                0x01, // list of 1 item
                0x00, 0x01, // ArgumentGet { index: 1 }
            ]
        );
    }

    #[test]
    fn test_adapter_export() {
        assert_to_bytes!(
            Adapter::Export {
                name: "a",
                input_types: vec![InterfaceType::I32, InterfaceType::I64],
                output_types: vec![InterfaceType::I32],
                instructions: vec![Instruction::ArgumentGet { index: 1 }],
            },
            &[
                0x01, // AdapterKind::Export
                0x01, // string of length 1
                0x61, // "a"
                0x02, // list of 2 items
                0x7f, // I32
                0x7e, // I64
                0x01, // list of 1 items
                0x7f, // I32
                0x01, // list of 1 item
                0x00, 0x01, // ArgumentGet { index: 1 }
            ]
        );
    }

    #[test]
    fn test_adapter_helper_function() {
        assert_to_bytes!(
            Adapter::HelperFunction {
                name: "a",
                input_types: vec![InterfaceType::I32, InterfaceType::I64],
                output_types: vec![InterfaceType::I32],
                instructions: vec![Instruction::ArgumentGet { index: 1 }],
            },
            &[
                0x02, // AdapterKind::HelperFunction
                0x01, // string of length 1
                0x61, // "a"
                0x02, // list of 2 items
                0x7f, // I32
                0x7e, // I64
                0x01, // list of 1 items
                0x7f, // I32
                0x01, // list of 1 item
                0x00, 0x01, // ArgumentGet { index: 1 }
            ]
        );
    }

    #[test]
    fn test_forward() {
        assert_to_bytes!(
            Forward { name: "ab" },
            &[
                0x02, // string of length 2
                0x61, // "a"
                0x62, // "b"
            ]
        );
    }

    #[test]
    fn test_interfaces() {
        assert_to_bytes!(
            Interfaces {
                exports: vec![Export {
                    name: "ab",
                    input_types: vec![InterfaceType::I32],
                    output_types: vec![InterfaceType::I32],
                }],
                types: vec![Type::new(
                    "ab",
                    vec!["cd", "e"],
                    vec![InterfaceType::I32, InterfaceType::I32],
                )],
                imports: vec![Import {
                    namespace: "a",
                    name: "b",
                    input_types: vec![InterfaceType::I32],
                    output_types: vec![InterfaceType::I64],
                }],
                adapters: vec![Adapter::Import {
                    namespace: "a",
                    name: "b",
                    input_types: vec![InterfaceType::I32],
                    output_types: vec![InterfaceType::I32],
                    instructions: vec![Instruction::ArgumentGet { index: 1 }],
                }],
                forwards: vec![Forward { name: "a" }],
            },
            &[
                0x01, // 1 export
                0x02, // string of 2 bytes
                0x61, 0x62, // "a", "b"
                0x01, // list of 1 item
                0x7f, // I32
                0x01, // list of 1 item
                0x7f, // I32
                0x01, // 1 type
                0x02, // string of 2 bytes
                0x61, 0x62, // "a", "b"
                0x02, // list of 2 items
                0x02, // string of 2 bytes
                0x63, 0x64, // "c", "d"
                0x01, // string of 1 byte
                0x65, // "e"
                0x02, // list of 2 items
                0x7f, // I32
                0x7f, // I32
                0x01, // 1 import
                0x01, // string of 1 byte
                0x61, // "a"
                0x01, // string of 1 byte
                0x62, // "b"
                0x01, // list of 1 item
                0x7f, // I32
                0x01, // list of 1 item
                0x7e, // I64
                0x01, // 1 adapter
                0x00, // adapter kind: import
                0x01, // string of 1 byte
                0x61, // "a"
                0x01, // string of 1 byte
                0x62, // "b"
                0x01, // list of 1 item
                0x7f, // I32
                0x01, // list of 1 item
                0x7f, // I32
                0x01, // list of 1 item
                0x00, 0x01, // ArgumentGet { index: 1 }
                0x01, // 1 adapter
                0x01, // string of 1 byte
                0x61, // "a"
            ]
        );
    }

    #[test]
    fn test_instructions() {
        assert_to_bytes!(
            vec![
                Instruction::ArgumentGet { index: 1 },
                Instruction::Call { function_index: 1 },
                Instruction::CallExport { export_name: "abc" },
                Instruction::ReadUtf8,
                Instruction::WriteUtf8 {
                    allocator_name: "abc",
                },
                Instruction::AsWasm(InterfaceType::Int),
                Instruction::AsInterface(InterfaceType::I64),
                Instruction::TableRefAdd,
                Instruction::TableRefGet,
                Instruction::CallMethod(1),
                Instruction::MakeRecord(InterfaceType::I32),
                Instruction::GetField(InterfaceType::Int, 2),
                Instruction::Const(InterfaceType::I32, 1),
                Instruction::FoldSeq(1),
                Instruction::Add(InterfaceType::I32),
                Instruction::MemToSeq(InterfaceType::I32, "abc"),
                Instruction::Load(InterfaceType::I32, "abc"),
                Instruction::SeqNew(InterfaceType::I32),
                Instruction::ListPush,
                Instruction::RepeatUntil(1, 2),
            ],
            &[
                0x14, // list of 20 items
                0x00, 0x01, // ArgumentGet { index: 1 }
                0x01, 0x01, // Call { function_index: 1 }
                0x02, 0x03, 0x61, 0x62, 0x63, // CallExport { export_name: "abc" }
                0x03, // ReadUtf8
                0x04, 0x03, 0x61, 0x62, 0x63, // WriteUtf8 { allocator_name: "abc" }
                0x05, 0xff, 0xff, 0x01, // AsWasm(Int)
                0x06, 0x7e, // AsInterface(I64)
                0x07, // TableRefAdd
                0x08, // TableRefGet
                0x09, 0x01, // CallMethod(1)
                0x0a, 0x7f, // MakeRecord(I32)
                0x0c, 0xff, 0xff, 0x01, 0x02, // GetField(Int, 2)
                0x0d, 0x7f, 0x01, // Const(I32, 1)
                0x0e, 0x01, // FoldSeq(1)
                0x0f, 0x7f, // Add(I32)
                0x10, 0x7f, 0x03, 0x61, 0x62, 0x63, // MemToSeq(I32, "abc")
                0x11, 0x7f, 0x03, 0x61, 0x62, 0x63, // Load(I32, "abc")
                0x12, 0x7f, // SeqNew(I32)
                0x13, // ListPush
                0x14, 0x01, 0x02, // RepeatUntil(1, 2)
            ]
        );
    }
}
