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

macro_rules! consume {
    (($input:ident, $parser_output:ident) = $parser_expression:expr) => {
        let (next_input, $parser_output) = $parser_expression;
        $input = next_input;
    };
}

#[derive(PartialEq, Debug)]
enum InterfaceType {
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

#[derive(PartialEq, Debug)]
enum AdapterKind {
    Import,
    Export,
    HelperFunction,
}

impl TryFrom<u8> for AdapterKind {
    type Error = &'static str;

    fn try_from(code: u8) -> Result<Self, Self::Error> {
        Ok(match code {
            0 => Self::Import,
            1 => Self::Export,
            2 => Self::HelperFunction,
            _ => return Err("Unknown adapter kind code."),
        })
    }
}

#[derive(Debug)]
struct Export<'input> {
    name: &'input str,
    input_types: Vec<InterfaceType>,
    output_types: Vec<InterfaceType>,
}

#[derive(Debug)]
struct Type<'input> {
    name: &'input str,
    fields: Vec<&'input str>,
    types: Vec<InterfaceType>,
}

#[derive(Debug)]
struct ImportedFunction<'input> {
    namespace: &'input str,
    name: &'input str,
    input_types: Vec<InterfaceType>,
    output_types: Vec<InterfaceType>,
}

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

    let (input, ty) = leb(input)?;

    match InterfaceType::try_from(ty) {
        Ok(ty) => Ok((input, ty)),
        Err(_) => Err(Err::Error(make_error(input, ErrorKind::ParseTo))),
    }
}

pub fn parse<'input, E: ParseError<&'input [u8]>>(
    bytes: &'input [u8],
) -> IResult<&'input [u8], bool, E> {
    let mut input = bytes;

    let mut exports = vec![];
    let mut types = vec![];
    let mut imported_functions = vec![];

    {
        consume!((input, number_of_exports) = leb(input)?);
        d!(number_of_exports);

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
    }

    {
        consume!((input, number_of_types) = leb(input)?);
        d!(number_of_types);

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
    }

    {
        consume!((input, number_of_imported_functions) = leb(input)?);
        d!(number_of_imported_functions);

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
    }

    {
        consume!((input, number_of_adapters) = leb(input)?);
        d!(number_of_adapters);

        for _ in 0..number_of_adapters {
            consume!((input, adapter_kind) = byte(input)?);
            let adapter_kind = AdapterKind::try_from(adapter_kind)
                .map_err(|_| Err::Error(make_error(input, ErrorKind::ParseTo)))?;
            d!(&adapter_kind);

            match adapter_kind {
                AdapterKind::Import => {
                    consume!((input, adapter_namespace) = string(input)?);
                    d!(adapter_namespace);

                    consume!((input, adapter_name) = string(input)?);
                    d!(adapter_name);

                    consume!((input, adapter_input_types) = list(input, ty)?);
                    d!(adapter_input_types);

                    consume!((input, adapter_output_types) = list(input, ty)?);
                    d!(adapter_output_types);
                }

                _ => println!("kind = {:?}", adapter_kind),
            }
        }
    }

    d!(exports);
    d!(types);
    d!(imported_functions);

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
