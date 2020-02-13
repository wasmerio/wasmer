//! Parse the WIT binary representation into an AST.

use crate::{ast::*, interpreter::Instruction};
use nom::{
    error::{make_error, ErrorKind, ParseError},
    Err, IResult,
};
use std::{convert::TryFrom, str};

/// Parse an `InterfaceType`.
impl TryFrom<u64> for InterfaceType {
    type Error = &'static str;

    fn try_from(code: u64) -> Result<Self, Self::Error> {
        Ok(match code {
            0x7fff => Self::Int,
            0x7ffe => Self::Float,
            0x7ffd => Self::Any,
            0x7ffc => Self::String,
            0x7ffb => Self::Seq,
            0x7f => Self::I32,
            0x7e => Self::I64,
            0x7d => Self::F32,
            0x7c => Self::F64,
            0x6f => Self::AnyRef,
            _ => return Err("Unknown interface type code."),
        })
    }
}

/// Parse an adapter kind.
impl TryFrom<u8> for AdapterKind {
    type Error = &'static str;

    fn try_from(code: u8) -> Result<Self, Self::Error> {
        Ok(match code {
            0x0 => Self::Import,
            0x1 => Self::Export,
            0x2 => Self::HelperFunction,
            _ => return Err("Unknown adapter kind code."),
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

/// Parse an unsigned LEB with value no larger than a 64-bits number.
fn leb<'input, E: ParseError<&'input [u8]>>(input: &'input [u8]) -> IResult<&'input [u8], u64, E> {
    if input.is_empty() {
        return Err(Err::Error(make_error(input, ErrorKind::Eof)));
    }

    let (output, bytes) = match input.iter().position(|&byte| byte & 0x80 == 0) {
        Some(position) => (&input[position + 1..], &input[..=position]),
        None => (&[] as &[u8], input),
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

    Ok((&input[length..], unsafe {
        str::from_utf8_unchecked(&input[..length])
    }))
}

/// Parse a list, with a item parser.
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

    let (output, ty) = leb(input)?;

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
            consume!((input, argument_0) = leb(input)?);
            (input, Instruction::ArgumentGet { index: argument_0 })
        }

        0x01 => {
            consume!((input, argument_0) = leb(input)?);
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

        0x05 => {
            consume!((input, argument_0) = ty(input)?);
            (input, Instruction::AsWasm(argument_0))
        }

        0x06 => {
            consume!((input, argument_0) = ty(input)?);
            (input, Instruction::AsInterface(argument_0))
        }

        0x07 => (input, Instruction::TableRefAdd),

        0x08 => (input, Instruction::TableRefGet),

        0x09 => {
            consume!((input, argument_0) = leb(input)?);
            (input, Instruction::CallMethod(argument_0))
        }

        0x0a => {
            consume!((input, argument_0) = ty(input)?);
            (input, Instruction::MakeRecord(argument_0))
        }

        0x0c => {
            consume!((input, argument_0) = ty(input)?);
            consume!((input, argument_1) = leb(input)?);
            (input, Instruction::GetField(argument_0, argument_1))
        }

        0x0d => {
            consume!((input, argument_0) = ty(input)?);
            consume!((input, argument_1) = leb(input)?);
            (input, Instruction::Const(argument_0, argument_1))
        }

        0x0e => {
            consume!((input, argument_0) = leb(input)?);
            (input, Instruction::FoldSeq(argument_0))
        }

        0x0f => {
            consume!((input, argument_0) = ty(input)?);
            (input, Instruction::Add(argument_0))
        }

        0x10 => {
            consume!((input, argument_0) = ty(input)?);
            consume!((input, argument_1) = string(input)?);
            (input, Instruction::MemToSeq(argument_0, argument_1))
        }

        0x11 => {
            consume!((input, argument_0) = ty(input)?);
            consume!((input, argument_1) = string(input)?);
            (input, Instruction::Load(argument_0, argument_1))
        }

        0x12 => {
            consume!((input, argument_0) = ty(input)?);
            (input, Instruction::SeqNew(argument_0))
        }

        0x13 => (input, Instruction::ListPush),

        0x14 => {
            consume!((input, argument_0) = leb(input)?);
            consume!((input, argument_1) = leb(input)?);
            (input, Instruction::RepeatUntil(argument_0, argument_1))
        }

        _ => return Err(Err::Error(make_error(input, ErrorKind::ParseTo))),
    })
}

/// Parse a list of exports.
fn exports<'input, E: ParseError<&'input [u8]>>(
    input: &'input [u8],
) -> IResult<&'input [u8], Vec<Export>, E> {
    let mut input = input;

    consume!((input, number_of_exports) = leb(input)?);

    let mut exports = Vec::with_capacity(number_of_exports as usize);

    for _ in 0..number_of_exports {
        consume!((input, export_name) = string(input)?);
        consume!((input, export_input_types) = list(input, ty)?);
        consume!((input, export_output_types) = list(input, ty)?);

        exports.push(Export {
            name: export_name,
            input_types: export_input_types,
            output_types: export_output_types,
        });
    }

    Ok((input, exports))
}

/// Parse a list of types.
fn types<'input, E: ParseError<&'input [u8]>>(
    input: &'input [u8],
) -> IResult<&'input [u8], Vec<Type>, E> {
    let mut input = input;

    consume!((input, number_of_types) = leb(input)?);

    let mut types = Vec::with_capacity(number_of_types as usize);

    for _ in 0..number_of_types {
        consume!((input, type_name) = string(input)?);
        consume!((input, type_fields) = list(input, string)?);
        consume!((input, type_types) = list(input, ty)?);

        types.push(Type::new(type_name, type_fields, type_types));
    }

    Ok((input, types))
}

/// Parse a list of imports.
fn imports<'input, E: ParseError<&'input [u8]>>(
    input: &'input [u8],
) -> IResult<&'input [u8], Vec<Import>, E> {
    let mut input = input;

    consume!((input, number_of_imports) = leb(input)?);

    let mut imports = Vec::with_capacity(number_of_imports as usize);

    for _ in 0..number_of_imports {
        consume!((input, import_namespace) = string(input)?);
        consume!((input, import_name) = string(input)?);
        consume!((input, import_input_types) = list(input, ty)?);
        consume!((input, import_output_types) = list(input, ty)?);

        imports.push(Import {
            namespace: import_namespace,
            name: import_name,
            input_types: import_input_types,
            output_types: import_output_types,
        });
    }

    Ok((input, imports))
}

/// Parse a list of adapters.
fn adapters<'input, E: ParseError<&'input [u8]>>(
    input: &'input [u8],
) -> IResult<&'input [u8], Vec<Adapter>, E> {
    let mut input = input;

    consume!((input, number_of_adapters) = leb(input)?);

    let mut adapters = Vec::with_capacity(number_of_adapters as usize);

    for _ in 0..number_of_adapters {
        consume!((input, adapter_kind) = byte(input)?);
        let adapter_kind = AdapterKind::try_from(adapter_kind)
            .map_err(|_| Err::Error(make_error(input, ErrorKind::ParseTo)))?;

        match adapter_kind {
            AdapterKind::Import => {
                consume!((input, adapter_namespace) = string(input)?);
                consume!((input, adapter_name) = string(input)?);
                consume!((input, adapter_input_types) = list(input, ty)?);
                consume!((input, adapter_output_types) = list(input, ty)?);
                consume!((input, adapter_instructions) = list(input, instruction)?);

                adapters.push(Adapter::Import {
                    namespace: adapter_namespace,
                    name: adapter_name,
                    input_types: adapter_input_types,
                    output_types: adapter_output_types,
                    instructions: adapter_instructions,
                });
            }

            AdapterKind::Export => {
                consume!((input, adapter_name) = string(input)?);
                consume!((input, adapter_input_types) = list(input, ty)?);
                consume!((input, adapter_output_types) = list(input, ty)?);
                consume!((input, adapter_instructions) = list(input, instruction)?);

                adapters.push(Adapter::Export {
                    name: adapter_name,
                    input_types: adapter_input_types,
                    output_types: adapter_output_types,
                    instructions: adapter_instructions,
                });
            }

            AdapterKind::HelperFunction => {
                consume!((input, adapter_name) = string(input)?);
                consume!((input, adapter_input_types) = list(input, ty)?);
                consume!((input, adapter_output_types) = list(input, ty)?);
                consume!((input, adapter_instructions) = list(input, instruction)?);

                adapters.push(Adapter::HelperFunction {
                    name: adapter_name,
                    input_types: adapter_input_types,
                    output_types: adapter_output_types,
                    instructions: adapter_instructions,
                });
            }
        }
    }

    Ok((input, adapters))
}

/// Parse a list of forwarded exports.
fn forwards<'input, E: ParseError<&'input [u8]>>(
    input: &'input [u8],
) -> IResult<&'input [u8], Vec<Forward>, E> {
    let mut input = input;

    consume!((input, number_of_forwards) = leb(input)?);

    let mut forwards = Vec::with_capacity(number_of_forwards as usize);

    for _ in 0..number_of_forwards {
        consume!((input, forward_name) = string(input)?);

        forwards.push(Forward { name: forward_name });
    }

    Ok((input, forwards))
}

/// Parse a sequence of bytes, expecting it to be a valid WIT binary
/// representation, into an `ast::Interfaces`.
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
/// # fn main() {
/// let input = &[
///     0x01, // 1 export
///     0x02, // string of 2 bytes
///     0x61, 0x62, // "a", "b"
///     0x01, // list of 1 item
///     0x7f, // I32
///     0x01, // list of 1 item
///     0x7f, // I32
///     0x01, // 1 type
///     0x02, // string of 2 bytes
///     0x61, 0x62, // "a", "b"
///     0x02, // list of 2 items
///     0x02, // string of 2 bytes
///     0x63, 0x64, // "c", "d"
///     0x01, // string of 1 byte
///     0x65, // "e"
///     0x02, // list of 2 items
///     0x7f, // I32
///     0x7f, // I32
///     0x01, // 1 import
///     0x01, // string of 1 byte
///     0x61, // "a"
///     0x01, // string of 1 byte
///     0x62, // "b"
///     0x01, // list of 1 item
///     0x7f, // I32
///     0x01, // list of 1 item
///     0x7e, // I64
///     0x01, // 1 adapter
///     0x00, // adapter kind: import
///     0x01, // string of 1 byte
///     0x61, // "a"
///     0x01, // string of 1 byte
///     0x62, // "b"
///     0x01, // list of 1 item
///     0x7f, // I32
///     0x01, // list of 1 item
///     0x7f, // I32
///     0x01, // list of 1 item
///     0x00, 0x01, // ArgumentGet { index: 1 }
///     0x01, // 1 adapter
///     0x01, // string of 1 byte
///     0x61, // "a"
/// ];
/// let output = Ok((
///     &[] as &[u8],
///     Interfaces {
///         exports: vec![Export {
///             name: "ab",
///             input_types: vec![InterfaceType::I32],
///             output_types: vec![InterfaceType::I32],
///         }],
///         types: vec![Type::new(
///             "ab",
///             vec!["cd", "e"],
///             vec![InterfaceType::I32, InterfaceType::I32],
///         )],
///         imports: vec![Import {
///             namespace: "a",
///             name: "b",
///             input_types: vec![InterfaceType::I32],
///             output_types: vec![InterfaceType::I64],
///         }],
///         adapters: vec![Adapter::Import {
///             namespace: "a",
///             name: "b",
///             input_types: vec![InterfaceType::I32],
///             output_types: vec![InterfaceType::I32],
///             instructions: vec![Instruction::ArgumentGet { index: 1 }],
///         }],
///         forwards: vec![Forward { name: "a" }],
///     },
/// ));
///
/// assert_eq!(parse::<()>(input), output);
/// # }
/// ```
pub fn parse<'input, E: ParseError<&'input [u8]>>(
    bytes: &'input [u8],
) -> IResult<&'input [u8], Interfaces, E> {
    let mut input = bytes;

    consume!((input, exports) = exports(input)?);
    consume!((input, types) = types(input)?);
    consume!((input, imports) = imports(input)?);
    consume!((input, adapters) = adapters(input)?);
    consume!((input, forwards) = forwards(input)?);

    Ok((
        input,
        Interfaces {
            exports,
            types,
            imports,
            adapters,
            forwards,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byte() {
        let input = &[0x01, 0x02, 0x03];
        let output = Ok((&[0x02, 0x03][..], 0x01u8));

        assert_eq!(byte::<()>(input), output);
    }

    #[test]
    fn test_leb_1_byte() {
        let input = &[0x01, 0x02, 0x03];
        let output = Ok((&[0x02, 0x03][..], 0x01u64));

        assert_eq!(leb::<()>(input), output);
    }

    #[test]
    fn test_leb_3_bytes() {
        let input = &[0xfc, 0xff, 0x01, 0x02];
        let output = Ok((&[0x02][..], 0x7ffcu64));

        assert_eq!(leb::<()>(input), output);
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
            0x0a, // list of 10 items
            0xff, 0xff, 0x01, // Int
            0xfe, 0xff, 0x01, // Float
            0xfd, 0xff, 0x01, // Any
            0xfc, 0xff, 0x01, // String
            0xfb, 0xff, 0x01, // Seq
            0x7f, // I32
            0x7e, // I64
            0x7d, // F32
            0x7c, // F64
            0x6f, // AnyRef
            0x01,
        ];
        let output = Ok((
            &[0x01][..],
            vec![
                InterfaceType::Int,
                InterfaceType::Float,
                InterfaceType::Any,
                InterfaceType::String,
                InterfaceType::Seq,
                InterfaceType::I32,
                InterfaceType::I64,
                InterfaceType::F32,
                InterfaceType::F64,
                InterfaceType::AnyRef,
            ],
        ));

        assert_eq!(list::<InterfaceType, ()>(input, ty), output);
    }

    #[test]
    fn test_instructions() {
        let input = &[
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
        ));

        assert_eq!(list::<Instruction, ()>(input, instruction), output);
    }

    #[test]
    fn test_exports() {
        let input = &[
            0x02, // 2 exports
            0x02, // string of 2 bytes
            0x61, 0x62, // "a", "b"
            0x01, // list of 1 item
            0x7f, // I32
            0x01, // list of 1 item
            0x7f, // I32
            0x02, // string of 2 bytes
            0x63, 0x64, // "c", "d"
            0x00, // list of 0 item
            0x00, // list of 0 item
        ];
        let output = Ok((
            &[] as &[u8],
            vec![
                Export {
                    name: "ab",
                    input_types: vec![InterfaceType::I32],
                    output_types: vec![InterfaceType::I32],
                },
                Export {
                    name: "cd",
                    input_types: vec![],
                    output_types: vec![],
                },
            ],
        ));

        assert_eq!(exports::<()>(input), output);
    }

    #[test]
    fn test_types() {
        let input = &[
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
        ];
        let output = Ok((
            &[] as &[u8],
            vec![Type::new(
                "ab",
                vec!["cd", "e"],
                vec![InterfaceType::I32, InterfaceType::I32],
            )],
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
            0x01, // list of 1 item
            0x7f, // I32
            0x01, // list of 1 item
            0x7e, // I64
            0x01, // string of 1 byte
            0x63, // "c"
            0x01, // string of 1 byte
            0x64, // "d"
            0x01, // list of 1 item
            0x7f, // I32
            0x01, // list of 1 item
            0x7e, // I64
        ];
        let output = Ok((
            &[] as &[u8],
            vec![
                Import {
                    namespace: "a",
                    name: "b",
                    input_types: vec![InterfaceType::I32],
                    output_types: vec![InterfaceType::I64],
                },
                Import {
                    namespace: "c",
                    name: "d",
                    input_types: vec![InterfaceType::I32],
                    output_types: vec![InterfaceType::I64],
                },
            ],
        ));

        assert_eq!(imports::<()>(input), output);
    }

    #[test]
    fn test_adapters() {
        let input = &[
            0x03, // 3 adapters
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
            0x01, // adapter kind: export
            0x01, // string of 1 byte
            0x63, // "c"
            0x01, // list of 1 item
            0x7f, // I32
            0x01, // list of 1 item
            0x7f, // I32
            0x01, // list of 1 item
            0x00, 0x01, // ArgumentGet { index: 1 }
            0x02, // adapter kind: helper function
            0x01, // string of 1 byte
            0x64, // "d"
            0x01, // list of 1 item
            0x7f, // I32
            0x01, // list of 1 item
            0x7f, // I32
            0x01, // list of 1 item
            0x00, 0x01, // ArgumentGet { index: 1 }
        ];
        let output = Ok((
            &[] as &[u8],
            vec![
                Adapter::Import {
                    namespace: "a",
                    name: "b",
                    input_types: vec![InterfaceType::I32],
                    output_types: vec![InterfaceType::I32],
                    instructions: vec![Instruction::ArgumentGet { index: 1 }],
                },
                Adapter::Export {
                    name: "c",
                    input_types: vec![InterfaceType::I32],
                    output_types: vec![InterfaceType::I32],
                    instructions: vec![Instruction::ArgumentGet { index: 1 }],
                },
                Adapter::HelperFunction {
                    name: "d",
                    input_types: vec![InterfaceType::I32],
                    output_types: vec![InterfaceType::I32],
                    instructions: vec![Instruction::ArgumentGet { index: 1 }],
                },
            ],
        ));

        assert_eq!(adapters::<()>(input), output);
    }

    #[test]
    fn test_forwards() {
        let input = &[
            0x02, // 2 adapters
            0x01, // string of 1 byte
            0x61, // "a"
            0x02, // string of 2 bytes
            0x62, 0x63, // "b", "c"
        ];
        let output = Ok((
            &[] as &[u8],
            vec![Forward { name: "a" }, Forward { name: "bc" }],
        ));

        assert_eq!(forwards::<()>(input), output);
    }

    #[test]
    fn test_parse() {
        let input = &[
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
        ];
        let output = Ok((
            &[] as &[u8],
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
        ));

        assert_eq!(parse::<()>(input), output);
    }
}
