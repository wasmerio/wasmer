use crate::ast::*;
use nom::{
    error::{make_error, ErrorKind, ParseError},
    Err, IResult,
};
use std::{convert::TryFrom, str};

fn byte<'input, E: ParseError<&'input [u8]>>(input: &'input [u8]) -> IResult<&'input [u8], u8, E> {
    if input.is_empty() {
        return Err(Err::Error(make_error(input, ErrorKind::Eof)));
    }

    Ok((&input[1..], input[0]))
}

fn leb<'input, E: ParseError<&'input [u8]>>(input: &'input [u8]) -> IResult<&'input [u8], u64, E> {
    if input.is_empty() {
        return Err(Err::Error(make_error(input, ErrorKind::Eof)));
    }

    let (output, bytes) = match input.iter().position(|&byte| byte & 0x80 == 0) {
        Some(position) => (&input[position + 1..], &input[..position + 1]),
        None => (&[] as &[u8], input),
    };

    Ok((
        output,
        bytes
            .iter()
            .rev()
            .fold(0, |acc, byte| (acc << 7) | (byte & 0x7f) as u64),
    ))
}

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

    let mut items = vec![];

    for _ in 0..length {
        consume!((input, item) = item_parser(input)?);
        items.push(item);
    }

    Ok((input, items))
}

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

fn instructions<'input, E: ParseError<&'input [u8]>>(
    input: &'input [u8],
) -> IResult<&'input [u8], Instruction, E> {
    let (mut input, opcode) = byte(input)?;

    Ok(match opcode {
        0x00 => {
            consume!((input, argument_0) = leb(input)?);
            (input, Instruction::ArgumentGet(argument_0))
        }

        0x01 => {
            consume!((input, argument_0) = leb(input)?);
            (input, Instruction::Call(argument_0))
        }

        0x02 => {
            consume!((input, argument_0) = string(input)?);
            (input, Instruction::CallExport(argument_0))
        }

        0x03 => (input, Instruction::ReadUtf8),

        0x04 => {
            consume!((input, argument_0) = string(input)?);
            (input, Instruction::WriteUtf8(argument_0))
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
            consume!((input, argument_0) = leb(input)?);
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

        _ => return Err(Err::Error(make_error(input, ErrorKind::ParseTo))),
    })
}

pub fn exports<'input, E: ParseError<&'input [u8]>>(
    input: &'input [u8],
) -> IResult<&'input [u8], Vec<Export>, E> {
    let mut input = input;
    let mut exports = vec![];

    consume!((input, number_of_exports) = leb(input)?);

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

pub fn types<'input, E: ParseError<&'input [u8]>>(
    input: &'input [u8],
) -> IResult<&'input [u8], Vec<Type>, E> {
    let mut input = input;
    let mut types = vec![];

    consume!((input, number_of_types) = leb(input)?);

    for _ in 0..number_of_types {
        consume!((input, type_name) = string(input)?);
        consume!((input, type_fields) = list(input, string)?);
        consume!((input, type_types) = list(input, ty)?);

        types.push(Type {
            name: type_name,
            fields: type_fields,
            types: type_types,
        });
    }

    Ok((input, types))
}

pub fn imported_functions<'input, E: ParseError<&'input [u8]>>(
    input: &'input [u8],
) -> IResult<&'input [u8], Vec<ImportedFunction>, E> {
    let mut input = input;
    let mut imported_functions = vec![];

    consume!((input, number_of_imported_functions) = leb(input)?);

    for _ in 0..number_of_imported_functions {
        consume!((input, imported_function_namespace) = string(input)?);
        consume!((input, imported_function_name) = string(input)?);
        consume!((input, imported_function_input_types) = list(input, ty)?);
        consume!((input, imported_function_output_types) = list(input, ty)?);

        imported_functions.push(ImportedFunction {
            namespace: imported_function_namespace,
            name: imported_function_name,
            input_types: imported_function_input_types,
            output_types: imported_function_output_types,
        });
    }

    Ok((input, imported_functions))
}

pub fn adapters<'input, E: ParseError<&'input [u8]>>(
    input: &'input [u8],
) -> IResult<&'input [u8], Vec<Adapter>, E> {
    let mut input = input;
    let mut adapters = vec![];

    consume!((input, number_of_adapters) = leb(input)?);

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
                consume!((input, adapter_instructions) = list(input, instructions)?);

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
                consume!((input, adapter_instructions) = list(input, instructions)?);

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
                consume!((input, adapter_instructions) = list(input, instructions)?);

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

pub fn forwards<'input, E: ParseError<&'input [u8]>>(
    input: &'input [u8],
) -> IResult<&'input [u8], Vec<Forward>, E> {
    let mut input = input;
    let mut forwards = vec![];

    consume!((input, number_of_forwards) = leb(input)?);

    for _ in 0..number_of_forwards {
        consume!((input, forward_name) = string(input)?);

        forwards.push(Forward { name: forward_name });
    }

    Ok((input, forwards))
}

pub fn parse<'input, E: ParseError<&'input [u8]>>(
    bytes: &'input [u8],
) -> IResult<&'input [u8], Interfaces, E> {
    let mut input = bytes;

    consume!((input, exports) = exports(input)?);
    consume!((input, types) = types(input)?);
    consume!((input, imported_functions) = imported_functions(input)?);
    consume!((input, adapters) = adapters(input)?);
    consume!((input, forwards) = forwards(input)?);

    Ok((
        input,
        Interfaces {
            exports,
            types,
            imported_functions,
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
            0x0e, // list of 14 items
            0x00, 0x01, // ArgumentGet(1)
            0x01, 0x01, // Call(1)
            0x02, 0x03, 0x61, 0x62, 0x63, // CallExport("abc")
            0x03, // ReadUtf8
            0x04, 0x03, 0x61, 0x62, 0x63, // WriteUtf8("abc")
            0x05, 0xff, 0xff, 0x01, // AsWasm(Int)
            0x06, 0x7e, // AsInterface(I64)
            0x07, // TableRefAdd
            0x08, // TableRefGet
            0x09, 0x01, // CallMethod(1)
            0x0a, 0x7f, // MakeRecord(I32)
            0x0c, 0x01, 0x02, // GetField(1, 2)
            0x0d, 0x7f, 0x01, // Const(I32, 1)
            0x0e, 0x01, // FoldSeq(1)
            0x0a,
        ];
        let output = Ok((
            &[0x0a][..],
            vec![
                Instruction::ArgumentGet(1),
                Instruction::Call(1),
                Instruction::CallExport("abc"),
                Instruction::ReadUtf8,
                Instruction::WriteUtf8("abc"),
                Instruction::AsWasm(InterfaceType::Int),
                Instruction::AsInterface(InterfaceType::I64),
                Instruction::TableRefAdd,
                Instruction::TableRefGet,
                Instruction::CallMethod(1),
                Instruction::MakeRecord(InterfaceType::I32),
                Instruction::GetField(1, 2),
                Instruction::Const(InterfaceType::I32, 1),
                Instruction::FoldSeq(1),
            ],
        ));

        assert_eq!(list::<Instruction, ()>(input, instructions), output);
    }
}
