//! Parse the WIT binary representation into an [AST](crate::ast).

use crate::{ast::*, interpreter::Instruction};
use nom::{
    error::{make_error, ErrorKind, ParseError},
    Err, IResult,
};
use std::{convert::TryFrom, str};

/// Parse an `InterfaceType`.
impl TryFrom<u8> for InterfaceType {
    type Error = &'static str;

    fn try_from(code: u8) -> Result<Self, Self::Error> {
        Ok(match code {
            0 => Self::S8,
            1 => Self::S16,
            2 => Self::S32,
            3 => Self::S64,
            4 => Self::U8,
            5 => Self::U16,
            6 => Self::U32,
            7 => Self::U64,
            8 => Self::F32,
            9 => Self::F64,
            10 => Self::String,
            11 => Self::Anyref,
            12 => Self::I32,
            13 => Self::I64,
            _ => return Err("Unknown interface type code."),
        })
    }
}

/// Parse an interface kind.
impl TryFrom<u8> for InterfaceKind {
    type Error = &'static str;

    fn try_from(code: u8) -> Result<Self, Self::Error> {
        Ok(match code {
            0x00 => Self::Type,
            0x01 => Self::Import,
            0x02 => Self::Adapter,
            0x03 => Self::Export,
            0x04 => Self::Implementation,
            _ => return Err("Unknown interface kind code."),
        })
    }
}

/// Parse a byte.
fn byte<'input, E: ParseError<&'input [u8]>>(input: &'input [u8]) -> IResult<&'input [u8], u8, E> {
    if input.is_empty() {
        return Err(Err::Error(make_error(input, ErrorKind::Eof)));
    }

    Ok((&input[1..], input[0]))
}

/// Parse an unsigned Little Endian Based (LEB) with value no larger
/// than a 64-bits number. Read
/// [LEB128](https://en.wikipedia.org/wiki/LEB128) to learn more, or
/// the Variable Length Data Section from the [DWARF 4
/// standard](http://dwarfstd.org/doc/DWARF4.pdf).
fn uleb<'input, E: ParseError<&'input [u8]>>(input: &'input [u8]) -> IResult<&'input [u8], u64, E> {
    if input.is_empty() {
        return Err(Err::Error(make_error(input, ErrorKind::Eof)));
    }

    let (output, bytes) = match input.iter().position(|&byte| byte & 0x80 == 0) {
        Some(length) if length <= 8 => (&input[length + 1..], &input[..=length]),
        Some(_) => return Err(Err::Error(make_error(input, ErrorKind::TooLarge))),
        None => return Err(Err::Error(make_error(input, ErrorKind::Eof))),
    };

    Ok((
        output,
        bytes
            .iter()
            .rev()
            .fold(0, |acc, byte| (acc << 7) | u64::from(byte & 0x7f)),
    ))
}

/// Parse a UTF-8 string.
fn string<'input, E: ParseError<&'input [u8]>>(
    input: &'input [u8],
) -> IResult<&'input [u8], &'input str, E> {
    if input.is_empty() {
        return Err(Err::Error(make_error(input, ErrorKind::Eof)));
    }

    let length = input[0] as usize;
    let input = &input[1..];

    if input.len() < length {
        return Err(Err::Error(make_error(input, ErrorKind::Eof)));
    }

    Ok((
        &input[length..],
        str::from_utf8(&input[..length])
            .map_err(|_| Err::Error(make_error(input, ErrorKind::ParseTo)))?,
    ))
}

/// Parse a list, with an item parser.
#[allow(clippy::type_complexity)]
fn list<'input, I, E: ParseError<&'input [u8]>>(
    input: &'input [u8],
    item_parser: fn(&'input [u8]) -> IResult<&'input [u8], I, E>,
) -> IResult<&'input [u8], Vec<I>, E> {
    if input.is_empty() {
        return Err(Err::Error(make_error(input, ErrorKind::Eof)));
    }

    let length = input[0] as usize;
    let mut input = &input[1..];

    if input.len() < length {
        return Err(Err::Error(make_error(input, ErrorKind::Eof)));
    }

    let mut items = Vec::with_capacity(length as usize);

    for _ in 0..length {
        consume!((input, item) = item_parser(input)?);
        items.push(item);
    }

    Ok((input, items))
}

/// Parse a type.
fn ty<'input, E: ParseError<&'input [u8]>>(
    input: &'input [u8],
) -> IResult<&'input [u8], InterfaceType, E> {
    if input.is_empty() {
        return Err(Err::Error(make_error(input, ErrorKind::Eof)));
    }

    let (output, ty) = byte(input)?;

    match InterfaceType::try_from(ty) {
        Ok(ty) => Ok((output, ty)),
        Err(_) => Err(Err::Error(make_error(input, ErrorKind::ParseTo))),
    }
}

/// Parse an instruction with its arguments.
fn instruction<'input, E: ParseError<&'input [u8]>>(
    input: &'input [u8],
) -> IResult<&'input [u8], Instruction, E> {
    let (mut input, opcode) = byte(input)?;

    Ok(match opcode {
        0x00 => {
            consume!((input, argument_0) = uleb(input)?);
            (
                input,
                Instruction::ArgumentGet {
                    index: argument_0 as u32,
                },
            )
        }

        0x01 => {
            consume!((input, argument_0) = uleb(input)?);
            (
                input,
                Instruction::Call {
                    function_index: argument_0 as usize,
                },
            )
        }

        0x02 => {
            consume!((input, argument_0) = string(input)?);
            (
                input,
                Instruction::CallExport {
                    export_name: argument_0,
                },
            )
        }

        0x03 => (input, Instruction::ReadUtf8),

        0x04 => {
            consume!((input, argument_0) = string(input)?);
            (
                input,
                Instruction::WriteUtf8 {
                    allocator_name: argument_0,
                },
            )
        }

        0x07 => (input, Instruction::I32ToS8),
        0x08 => (input, Instruction::I32ToS8X),
        0x09 => (input, Instruction::I32ToU8),
        0x0a => (input, Instruction::I32ToS16),
        0x0b => (input, Instruction::I32ToS16X),
        0x0c => (input, Instruction::I32ToU16),
        0x0d => (input, Instruction::I32ToS32),
        0x0e => (input, Instruction::I32ToU32),
        0x0f => (input, Instruction::I32ToS64),
        0x10 => (input, Instruction::I32ToU64),
        0x11 => (input, Instruction::I64ToS8),
        0x12 => (input, Instruction::I64ToS8X),
        0x13 => (input, Instruction::I64ToU8),
        0x14 => (input, Instruction::I64ToS16),
        0x15 => (input, Instruction::I64ToS16X),
        0x16 => (input, Instruction::I64ToU16),
        0x17 => (input, Instruction::I64ToS32),
        0x18 => (input, Instruction::I64ToS32X),
        0x19 => (input, Instruction::I64ToU32),
        0x1a => (input, Instruction::I64ToS64),
        0x1b => (input, Instruction::I64ToU64),
        0x1c => (input, Instruction::S8ToI32),
        0x1d => (input, Instruction::U8ToI32),
        0x1e => (input, Instruction::S16ToI32),
        0x1f => (input, Instruction::U16ToI32),
        0x20 => (input, Instruction::S32ToI32),
        0x21 => (input, Instruction::U32ToI32),
        0x22 => (input, Instruction::S64ToI32),
        0x23 => (input, Instruction::S64ToI32X),
        0x24 => (input, Instruction::U64ToI32),
        0x25 => (input, Instruction::U64ToI32X),
        0x26 => (input, Instruction::S8ToI64),
        0x27 => (input, Instruction::U8ToI64),
        0x28 => (input, Instruction::S16ToI64),
        0x29 => (input, Instruction::U16ToI64),
        0x2a => (input, Instruction::S32ToI64),
        0x2b => (input, Instruction::U32ToI64),
        0x2c => (input, Instruction::S64ToI64),
        0x2d => (input, Instruction::U64ToI64),

        _ => return Err(Err::Error(make_error(input, ErrorKind::ParseTo))),
    })
}

/// Parse a list of types.
fn types<'input, E: ParseError<&'input [u8]>>(
    mut input: &'input [u8],
) -> IResult<&'input [u8], Vec<Type>, E> {
    consume!((input, number_of_types) = uleb(input)?);

    let mut types = Vec::with_capacity(number_of_types as usize);

    for _ in 0..number_of_types {
        consume!((input, inputs) = list(input, ty)?);
        consume!((input, outputs) = list(input, ty)?);

        types.push(Type { inputs, outputs });
    }

    Ok((input, types))
}

/// Parse a list of imports.
fn imports<'input, E: ParseError<&'input [u8]>>(
    mut input: &'input [u8],
) -> IResult<&'input [u8], Vec<Import>, E> {
    consume!((input, number_of_imports) = uleb(input)?);

    let mut imports = Vec::with_capacity(number_of_imports as usize);

    for _ in 0..number_of_imports {
        consume!((input, namespace) = string(input)?);
        consume!((input, name) = string(input)?);
        consume!((input, signature_type) = uleb(input)?);

        imports.push(Import {
            namespace,
            name,
            signature_type: signature_type as u32,
        });
    }

    Ok((input, imports))
}

/// Parse a list of adapters.
fn adapters<'input, E: ParseError<&'input [u8]>>(
    mut input: &'input [u8],
) -> IResult<&'input [u8], Vec<Adapter>, E> {
    consume!((input, number_of_adapters) = uleb(input)?);

    let mut adapters = Vec::with_capacity(number_of_adapters as usize);

    for _ in 0..number_of_adapters {
        consume!((input, function_type) = uleb(input)?);
        consume!((input, instructions) = list(input, instruction)?);

        adapters.push(Adapter {
            function_type: function_type as u32,
            instructions,
        });
    }

    Ok((input, adapters))
}

/// Parse a list of exports.
fn exports<'input, E: ParseError<&'input [u8]>>(
    mut input: &'input [u8],
) -> IResult<&'input [u8], Vec<Export>, E> {
    consume!((input, number_of_exports) = uleb(input)?);

    let mut exports = Vec::with_capacity(number_of_exports as usize);

    for _ in 0..number_of_exports {
        consume!((input, name) = string(input)?);
        consume!((input, function_type) = uleb(input)?);

        exports.push(Export {
            name,
            function_type: function_type as u32,
        });
    }

    Ok((input, exports))
}

/// Parse a list of implementations.
fn implementations<'input, E: ParseError<&'input [u8]>>(
    mut input: &'input [u8],
) -> IResult<&'input [u8], Vec<Implementation>, E> {
    consume!((input, number_of_implementations) = uleb(input)?);

    let mut implementations = Vec::with_capacity(number_of_implementations as usize);

    for _ in 0..number_of_implementations {
        consume!((input, core_function_type) = uleb(input)?);
        consume!((input, adapter_function_type) = uleb(input)?);

        implementations.push(Implementation {
            core_function_type: core_function_type as u32,
            adapter_function_type: adapter_function_type as u32,
        });
    }

    Ok((input, implementations))
}

/// Parse complete interfaces.
fn interfaces<'input, E: ParseError<&'input [u8]>>(
    bytes: &'input [u8],
) -> IResult<&'input [u8], Interfaces, E> {
    let mut input = bytes;

    let mut all_types = vec![];
    let mut all_imports = vec![];
    let mut all_adapters = vec![];
    let mut all_exports = vec![];
    let mut all_implementations = vec![];

    while !input.is_empty() {
        consume!((input, interface_kind) = byte(input)?);

        let interface_kind = InterfaceKind::try_from(interface_kind)
            .map_err(|_| Err::Error(make_error(input, ErrorKind::ParseTo)))?;

        match interface_kind {
            InterfaceKind::Type => {
                consume!((input, mut new_types) = types(input)?);
                all_types.append(&mut new_types);
            }

            InterfaceKind::Import => {
                consume!((input, mut new_imports) = imports(input)?);
                all_imports.append(&mut new_imports);
            }

            InterfaceKind::Adapter => {
                consume!((input, mut new_adapters) = adapters(input)?);
                all_adapters.append(&mut new_adapters);
            }

            InterfaceKind::Export => {
                consume!((input, mut new_exports) = exports(input)?);
                all_exports.append(&mut new_exports);
            }

            InterfaceKind::Implementation => {
                consume!((input, mut new_implementations) = implementations(input)?);
                all_implementations.append(&mut new_implementations)
            }
        }
    }

    Ok((
        input,
        Interfaces {
            types: all_types,
            imports: all_imports,
            adapters: all_adapters,
            exports: all_exports,
            implementations: all_implementations,
        },
    ))
}

/// Parse a sequence of bytes, expecting it to be a valid WIT binary
/// representation, into an [`Interfaces`](crate::ast::Interfaces)
/// structure.
///
/// # Example
///
/// ```rust
/// use wasmer_interface_types::{
///     ast::*,
///     decoders::binary::parse,
///     interpreter::Instruction,
/// };
///
/// let input = &[
///     0x00, // type section
///     0x01, // 1 type
///     0x01, // list of 1 item
///     0x00, // S8
///     0x01, // list of 1 item
///     0x01, // S16
///     //
///     0x01, // import section
///     0x01, // 1 import
///     0x02, // string of 2 bytes
///     0x61, 0x62, // "a", "b"
///     0x01, // string of 1 byte
///     0x63, // "c"
///     0x00, // signature type
///     //
///     0x02, // adapter section
///     0x01, // 1 adapter
///     0x00, // function type
///     0x01, // list of 1 item
///     0x00, 0x01, // ArgumentGet { index: 1 }
///     //
///     0x03, // export section
///     0x01, // 1 export
///     0x02, // string of 2 bytes
///     0x61, 0x62, // "a", "b"
///     0x01, // function type
///     //
///     0x04, // implementation section
///     0x01, // 1 implementation
///     0x02, // core function type
///     0x03, // adapter function type
/// ];
/// let output = Ok((
///     &[] as &[u8],
///     Interfaces {
///         types: vec![Type {
///             inputs: vec![InterfaceType::S8],
///             outputs: vec![InterfaceType::S16],
///         }],
///         imports: vec![Import {
///             namespace: "ab",
///             name: "c",
///             signature_type: 0,
///         }],
///         adapters: vec![Adapter {
///             function_type: 0,
///             instructions: vec![Instruction::ArgumentGet { index: 1 }],
///         }],
///         exports: vec![Export {
///             name: "ab",
///             function_type: 1,
///         }],
///         implementations: vec![Implementation {
///             core_function_type: 2,
///             adapter_function_type: 3,
///         }],
///     },
/// ));
///
/// assert_eq!(parse::<()>(input), output);
/// ```
pub fn parse<'input, E: ParseError<&'input [u8]>>(
    bytes: &'input [u8],
) -> IResult<&'input [u8], Interfaces, E> {
    interfaces(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nom::{error, Err};

    #[test]
    fn test_byte() {
        let input = &[0x01, 0x02, 0x03];
        let output = Ok((&[0x02, 0x03][..], 0x01u8));

        assert_eq!(byte::<()>(input), output);
    }

    #[test]
    fn test_uleb_1_byte() {
        let input = &[0x01, 0x02, 0x03];
        let output = Ok((&[0x02, 0x03][..], 0x01u64));

        assert_eq!(uleb::<()>(input), output);
    }

    #[test]
    fn test_uleb_3_bytes() {
        let input = &[0xfc, 0xff, 0x01, 0x02];
        let output = Ok((&[0x02][..], 0x7ffcu64));

        assert_eq!(uleb::<()>(input), output);
    }

    // Examples from Figure 22 of [DWARF 4
    // standard](http://dwarfstd.org/doc/DWARF4.pdf).
    #[test]
    fn test_uleb_from_dwarf_standard() {
        macro_rules! assert_uleb {
            ($to_parse:expr => $expected_result:expr) => {
                assert_eq!(uleb::<()>($to_parse), Ok((&[][..], $expected_result)));
            };
        }

        assert_uleb!(&[2u8] => 2u64);
        assert_uleb!(&[127u8] => 127u64);
        assert_uleb!(&[0x80, 1u8] => 128u64);
        assert_uleb!(&[1u8 | 0x80, 1] => 129u64);
        assert_uleb!(&[2u8 | 0x80, 1] => 130u64);
        assert_uleb!(&[57u8 | 0x80, 100] => 12857u64);
    }

    #[test]
    fn test_uleb_eof() {
        let input = &[0x80];

        assert_eq!(
            uleb::<(&[u8], error::ErrorKind)>(input),
            Err(Err::Error((&input[..], error::ErrorKind::Eof))),
        );
    }

    #[test]
    fn test_uleb_overflow() {
        let input = &[
            0x01 | 0x80,
            0x02 | 0x80,
            0x03 | 0x80,
            0x04 | 0x80,
            0x05 | 0x80,
            0x06 | 0x80,
            0x07 | 0x80,
            0x08 | 0x80,
            0x09 | 0x80,
            0x0a,
        ];

        assert_eq!(
            uleb::<(&[u8], error::ErrorKind)>(input),
            Err(Err::Error((&input[..], error::ErrorKind::TooLarge))),
        );
    }

    #[test]
    fn test_string() {
        let input = &[
            0x03, // string of 3 bytes
            0x61, // "a"
            0x62, // "b"
            0x63, // "c"
            0x64, 0x65,
        ];
        let output = Ok((&[0x64, 0x65][..], "abc"));

        assert_eq!(string::<()>(input), output);
    }

    #[test]
    fn test_list() {
        let input = &[
            0x02, // list of 2 items
            0x01, // string of 1 byte
            0x61, // "a"
            0x02, // string of 2 bytes
            0x62, // "b"
            0x63, // "c"
            0x07,
        ];
        let output = Ok((&[0x07][..], vec!["a", "bc"]));

        assert_eq!(list::<&str, ()>(input, string), output);
    }

    #[test]
    fn test_ty() {
        let input = &[
            0x0e, // list of 14 items
            0x00, // S8
            0x01, // S16
            0x02, // S32
            0x03, // S64
            0x04, // U8
            0x05, // U16
            0x06, // U32
            0x07, // U64
            0x08, // F32
            0x09, // F64
            0x0a, // String
            0x0b, // Anyref
            0x0c, // I32
            0x0d, // I64
            0x01,
        ];
        let output = Ok((
            &[0x01][..],
            vec![
                InterfaceType::S8,
                InterfaceType::S16,
                InterfaceType::S32,
                InterfaceType::S64,
                InterfaceType::U8,
                InterfaceType::U16,
                InterfaceType::U32,
                InterfaceType::U64,
                InterfaceType::F32,
                InterfaceType::F64,
                InterfaceType::String,
                InterfaceType::Anyref,
                InterfaceType::I32,
                InterfaceType::I64,
            ],
        ));

        assert_eq!(list::<InterfaceType, ()>(input, ty), output);
    }

    #[test]
    fn test_instructions() {
        let input = &[
            0x2c, // list of 44 items
            0x00, 0x01, // ArgumentGet { index: 1 }
            0x01, 0x01, // Call { function_index: 1 }
            0x02, 0x03, 0x61, 0x62, 0x63, // CallExport { export_name: "abc" }
            0x03, // ReadUtf8
            0x04, 0x03, 0x61, 0x62, 0x63, // WriteUtf8 { allocator_name: "abc" }
            0x07, // I32ToS8
            0x08, // I32ToS8X
            0x09, // I32ToU8
            0x0a, // I32ToS16
            0x0b, // I32ToS16X
            0x0c, // I32ToU16
            0x0d, // I32ToS32
            0x0e, // I32ToU32
            0x0f, // I32ToS64
            0x10, // I32ToU64
            0x11, // I64ToS8
            0x12, // I64ToS8X
            0x13, // I64ToU8
            0x14, // I64ToS16
            0x15, // I64ToS16X
            0x16, // I64ToU16
            0x17, // I64ToS32
            0x18, // I64ToS32X
            0x19, // I64ToU32
            0x1a, // I64ToS64
            0x1b, // I64ToU64
            0x1c, // S8ToI32
            0x1d, // U8ToI32
            0x1e, // S16ToI32
            0x1f, // U16ToI32
            0x20, // S32ToI32
            0x21, // U32ToI32
            0x22, // S64ToI32
            0x23, // S64ToI32X
            0x24, // U64ToI32
            0x25, // U64ToI32X
            0x26, // S8ToI64
            0x27, // U8ToI64
            0x28, // S16ToI64
            0x29, // U16ToI64
            0x2a, // S32ToI64
            0x2b, // U32ToI64
            0x2c, // S64ToI64
            0x2d, // U64ToI64
            0x0a,
        ];
        let output = Ok((
            &[0x0a][..],
            vec![
                Instruction::ArgumentGet { index: 1 },
                Instruction::Call { function_index: 1 },
                Instruction::CallExport { export_name: "abc" },
                Instruction::ReadUtf8,
                Instruction::WriteUtf8 {
                    allocator_name: "abc",
                },
                Instruction::I32ToS8,
                Instruction::I32ToS8X,
                Instruction::I32ToU8,
                Instruction::I32ToS16,
                Instruction::I32ToS16X,
                Instruction::I32ToU16,
                Instruction::I32ToS32,
                Instruction::I32ToU32,
                Instruction::I32ToS64,
                Instruction::I32ToU64,
                Instruction::I64ToS8,
                Instruction::I64ToS8X,
                Instruction::I64ToU8,
                Instruction::I64ToS16,
                Instruction::I64ToS16X,
                Instruction::I64ToU16,
                Instruction::I64ToS32,
                Instruction::I64ToS32X,
                Instruction::I64ToU32,
                Instruction::I64ToS64,
                Instruction::I64ToU64,
                Instruction::S8ToI32,
                Instruction::U8ToI32,
                Instruction::S16ToI32,
                Instruction::U16ToI32,
                Instruction::S32ToI32,
                Instruction::U32ToI32,
                Instruction::S64ToI32,
                Instruction::S64ToI32X,
                Instruction::U64ToI32,
                Instruction::U64ToI32X,
                Instruction::S8ToI64,
                Instruction::U8ToI64,
                Instruction::S16ToI64,
                Instruction::U16ToI64,
                Instruction::S32ToI64,
                Instruction::U32ToI64,
                Instruction::S64ToI64,
                Instruction::U64ToI64,
            ],
        ));

        assert_eq!(list::<Instruction, ()>(input, instruction), output);
    }

    #[test]
    fn test_exports() {
        let input = &[
            0x02, // 2 exports
            0x02, // string of 2 bytes
            0x61, 0x62, // "a", "b"
            0x01, // function type
            0x02, // string of 2 bytes
            0x63, 0x64, // "c", "d"
            0x02, // function type
        ];
        let output = Ok((
            &[] as &[u8],
            vec![
                Export {
                    name: "ab",
                    function_type: 1,
                },
                Export {
                    name: "cd",
                    function_type: 2,
                },
            ],
        ));

        assert_eq!(exports::<()>(input), output);
    }

    #[test]
    fn test_types() {
        let input = &[
            0x01, // 1 type
            0x02, // list of 2 items
            0x02, // S32
            0x02, // S32
            0x01, // list of 2 items
            0x02, // S32
        ];
        let output = Ok((
            &[] as &[u8],
            vec![Type {
                inputs: vec![InterfaceType::S32, InterfaceType::S32],
                outputs: vec![InterfaceType::S32],
            }],
        ));

        assert_eq!(types::<()>(input), output);
    }

    #[test]
    fn test_imports() {
        let input = &[
            0x02, // 2 imports
            0x01, // string of 1 byte
            0x61, // "a"
            0x01, // string of 1 byte
            0x62, // "b"
            0x01, // signature type
            0x01, // string of 1 byte
            0x63, // "c"
            0x01, // string of 1 byte
            0x64, // "d"
            0x02, // signature type
        ];
        let output = Ok((
            &[] as &[u8],
            vec![
                Import {
                    namespace: "a",
                    name: "b",
                    signature_type: 1,
                },
                Import {
                    namespace: "c",
                    name: "d",
                    signature_type: 2,
                },
            ],
        ));

        assert_eq!(imports::<()>(input), output);
    }

    #[test]
    fn test_adapters() {
        let input = &[
            0x01, // 1 adapters
            0x00, // function type
            0x01, // list of 1 item
            0x00, 0x01, // ArgumentGet { index: 1 }
        ];
        let output = Ok((
            &[] as &[u8],
            vec![Adapter {
                function_type: 0,
                instructions: vec![Instruction::ArgumentGet { index: 1 }],
            }],
        ));

        assert_eq!(adapters::<()>(input), output);
    }

    #[test]
    fn test_parse() {
        let input = &[
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
        ];
        let output = Ok((
            &[] as &[u8],
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
        ));

        assert_eq!(interfaces::<()>(input), output);
    }
}
