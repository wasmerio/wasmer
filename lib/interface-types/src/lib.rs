use nom::{
    error::{make_error, ErrorKind, ParseError},
    Err, IResult,
};
use std::{convert::TryFrom, str};

macro_rules! d {
    ($expression:expr) => {
        match $expression {
            tmp => {
                eprintln!(
                    "[{}:{}] {} = {:?}",
                    file!(),
                    line!(),
                    stringify!($expression),
                    &tmp
                );

                tmp
            }
        }
    };
}

#[derive(PartialEq, Debug)]
enum Type {
    Int,
    Float,
    Any,
    String,
    Seq,

    I32,
    I64,
    F32,
    F64,
    AnyRef,
}

#[derive(Debug)]
struct Export<'input> {
    name: &'input str,
    input_types: Vec<Type>,
    output_types: Vec<Type>,
}

impl TryFrom<u64> for Type {
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

            _ => return Err("Unknown type code."),
        })
    }
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

fn list<'input, I: ::std::fmt::Debug, E: ParseError<&'input [u8]>>(
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
        let (next_input, item) = item_parser(input)?;
        items.push(item);
        input = next_input;
    }

    Ok((input, items))
}

fn ty<'input, E: ParseError<&'input [u8]>>(input: &'input [u8]) -> IResult<&'input [u8], Type, E> {
    if input.is_empty() {
        return Err(Err::Error(make_error(input, ErrorKind::Eof)));
    }

    let (input, ty) = leb(input)?;

    match Type::try_from(ty) {
        Ok(ty) => Ok((input, ty)),
        Err(_) => Err(Err::Error(make_error(input, ErrorKind::ParseTo))),
    }
}

pub fn parse<'input, E: ParseError<&'input [u8]>>(
    bytes: &'input [u8],
) -> IResult<&'input [u8], bool, E> {
    let input = bytes;
    let (mut input, number_of_exports) = leb(input)?;
    d!(number_of_exports);

    let mut exports = vec![];

    for export_nth in 0..number_of_exports {
        let (next_input, export_name) = string(input)?;
        input = next_input;

        let (next_input, export_input_types) = list(input, ty)?;
        input = next_input;

        let (next_input, export_output_types) = list(input, ty)?;
        input = next_input;

        exports.push(Export {
            name: export_name,
            input_types: export_input_types,
            output_types: export_output_types,
        });
    }

    d!(exports);

    Ok((&[] as &[u8], true))
}

#[cfg(test)]
mod tests {
    use super::parse;
    use std::fs;
    use wasmer_clif_backend::CraneliftCompiler;
    use wasmer_runtime_core as runtime;

    fn get_module() -> runtime::Module {
        runtime::compile_with(
            fs::read("tests/assets/hello_world.wasm")
                .expect("Failed to read `tests/assets/hello_world.wasm`.")
                .as_slice(),
            &CraneliftCompiler::new(),
        )
        .expect("Failed to parse the `hello_world.wasm` module.")
    }

    #[test]
    fn test_has_custom_section() {
        let module = get_module();
        let custom_section = module.info().custom_sections.get("interface-types");

        assert!(custom_section.is_some());
    }

    #[test]
    fn test_parse() {
        let module = get_module();
        let custom_section_bytes = module
            .info()
            .custom_sections
            .get("interface-types")
            .unwrap()
            .as_slice();

        parse::<()>(custom_section_bytes);
    }
}
