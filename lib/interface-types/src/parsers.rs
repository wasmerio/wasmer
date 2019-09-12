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
