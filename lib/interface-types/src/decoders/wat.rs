//! Parse the WIT textual representation into an AST.

#![allow(unused)]

use crate::ast::*;
use nom::{
    branch::alt,
    bytes::complete::{escaped, tag, take_while1},
    character::complete::{alphanumeric1, char, one_of},
    combinator::{cut, map, opt, value},
    error::ParseError,
    multi::many0,
    sequence::{delimited, preceded, terminated, tuple},
    AsChar, IResult,
};

/// Parse a whitespace.
fn whitespace<'input, E: ParseError<&'input str>>(
    input: &'input str,
) -> IResult<&'input str, &'input str, E> {
    let whitespaces = " \t\r\n";

    take_while1(move |c| whitespaces.contains(c))(input)
}

/// Parse an `InterfaceType`.
fn interface_type<'input, E: ParseError<&'input str>>(
    input: &'input str,
) -> IResult<&'input str, InterfaceType, E> {
    let int = value(InterfaceType::Int, tag("Int"));
    let float = value(InterfaceType::Float, tag("Float"));
    let any = value(InterfaceType::Any, tag("Any"));
    let string = value(InterfaceType::String, tag("String"));
    let seq = value(InterfaceType::Seq, tag("Seq"));
    let r#i32 = value(InterfaceType::I32, tag("i32"));
    let r#i64 = value(InterfaceType::I64, tag("i64"));
    let r#f32 = value(InterfaceType::F32, tag("f32"));
    let r#f64 = value(InterfaceType::F64, tag("f64"));
    let anyref = value(InterfaceType::AnyRef, tag("anyref"));

    alt((
        int, float, any, string, seq, r#i32, r#i64, r#f32, r#f64, anyref,
    ))(input)
}

/// Parse a string.
fn string<'input, E: ParseError<&'input str>>(
    input: &'input str,
) -> IResult<&'input str, &'input str, E> {
    escaped(alphanumeric1, '\\', one_of(r#""\"#))(input)
}

/// Parse a `(param …)`.
fn param<'input, E: ParseError<&'input str>>(
    input: &'input str,
) -> IResult<&'input str, Vec<InterfaceType>, E> {
    delimited(
        char('('),
        preceded(
            opt(whitespace),
            preceded(tag("param"), many0(preceded(whitespace, interface_type))),
        ),
        char(')'),
    )(input)
}

/// Parse a `(result …)`.
fn result<'input, E: ParseError<&'input str>>(
    input: &'input str,
) -> IResult<&'input str, Vec<InterfaceType>, E> {
    delimited(
        char('('),
        preceded(
            opt(whitespace),
            preceded(tag("result"), many0(preceded(whitespace, interface_type))),
        ),
        char(')'),
    )(input)
}

/// Parse an `Export`.
fn export<'input, E: ParseError<&'input str>>(
    input: &'input str,
) -> IResult<&'input str, Export, E> {
    map(
        delimited(
            char('('),
            preceded(
                opt(whitespace),
                preceded(
                    tag("@interface"),
                    preceded(
                        whitespace,
                        preceded(
                            tag("export"),
                            tuple((
                                preceded(
                                    whitespace,
                                    preceded(char('"'), cut(terminated(string, char('"')))),
                                ),
                                opt(preceded(whitespace, param)),
                                opt(preceded(whitespace, result)),
                            )),
                        ),
                    ),
                ),
            ),
            char(')'),
        ),
        |(name, input_types, output_types)| Export {
            name,
            input_types: input_types.unwrap_or_else(|| vec![]),
            output_types: output_types.unwrap_or_else(|| vec![]),
        },
    )(input)
}

/// Parse a `(import …)`.
fn import_qualifier<'input, E: ParseError<&'input str>>(
    input: &'input str,
) -> IResult<&'input str, (&'input str, &'input str), E> {
    delimited(
        char('('),
        preceded(
            opt(whitespace),
            preceded(
                tag("import"),
                tuple((
                    preceded(
                        whitespace,
                        preceded(char('"'), cut(terminated(string, char('"')))),
                    ),
                    preceded(
                        whitespace,
                        preceded(char('"'), cut(terminated(string, char('"')))),
                    ),
                )),
            ),
        ),
        char(')'),
    )(input)
}

/// Parse a `$…`.
fn index_variable<'input, E: ParseError<&'input str>>(
    input: &'input str,
) -> IResult<&'input str, &'input str, E> {
    preceded(
        char('$'),
        take_while1(move |c: char| c.is_alphanum() || c == '_'),
    )(input)
}

/// Parse an `Import`.
fn import<'input, E: ParseError<&'input str>>(
    input: &'input str,
) -> IResult<&'input str, Import, E> {
    map(
        delimited(
            char('('),
            preceded(
                opt(whitespace),
                preceded(
                    tag("@interface"),
                    preceded(
                        whitespace,
                        preceded(
                            tag("func"),
                            tuple((
                                opt(preceded(whitespace, index_variable)),
                                preceded(whitespace, import_qualifier),
                                opt(preceded(whitespace, param)),
                                opt(preceded(whitespace, result)),
                            )),
                        ),
                    ),
                ),
            ),
            char(')'),
        ),
        |(_index, (namespace, name), input_types, output_types)| Import {
            namespace,
            name,
            input_types: input_types.unwrap_or_else(|| vec![]),
            output_types: output_types.unwrap_or_else(|| vec![]),
        },
    )(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_whitespace() {
        let inputs = vec![" a", "  a", "\n  a", "\r\n a"];
        let outputs = vec![" ", "  ", "\n  ", "\r\n "];

        for (nth, input) in inputs.iter().enumerate() {
            assert_eq!(whitespace::<()>(input), Ok(("a", outputs[nth])));
        }
    }

    #[test]
    fn test_interface_type() {
        let inputs = vec![
            "Int", "Float", "Any", "String", "Seq", "i32", "i64", "f32", "f64", "anyref",
        ];
        let outputs = vec![
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
        ];

        assert_eq!(inputs.len(), outputs.len());

        for (nth, input) in inputs.iter().enumerate() {
            assert_eq!(interface_type::<()>(input), Ok(("", outputs[nth])));
        }
    }

    #[test]
    fn test_param_empty() {
        let input = "(param)";
        let output = vec![];

        assert_eq!(param::<()>(input), Ok(("", output)));
    }

    #[test]
    fn test_param() {
        let input = "(param i32 String)";
        let output = vec![InterfaceType::I32, InterfaceType::String];

        assert_eq!(param::<()>(input), Ok(("", output)));
    }

    #[test]
    fn test_result_empty() {
        let input = "(result)";
        let output = vec![];

        assert_eq!(result::<()>(input), Ok(("", output)));
    }

    #[test]
    fn test_result() {
        let input = "(result i32 String)";
        let output = vec![InterfaceType::I32, InterfaceType::String];

        assert_eq!(result::<()>(input), Ok(("", output)));
    }

    #[test]
    fn test_export_with_no_param_no_result() {
        let input = r#"(@interface export "foo")"#;
        let output = Export {
            name: "foo",
            input_types: vec![],
            output_types: vec![],
        };

        assert_eq!(export::<()>(input), Ok(("", output)));
    }

    #[test]
    fn test_export_with_some_param_no_result() {
        let input = r#"(@interface export "foo" (param i32))"#;
        let output = Export {
            name: "foo",
            input_types: vec![InterfaceType::I32],
            output_types: vec![],
        };

        assert_eq!(export::<()>(input), Ok(("", output)));
    }

    #[test]
    fn test_export_with_no_param_some_result() {
        let input = r#"(@interface export "foo" (result i32))"#;
        let output = Export {
            name: "foo",
            input_types: vec![],
            output_types: vec![InterfaceType::I32],
        };

        assert_eq!(export::<()>(input), Ok(("", output)));
    }

    #[test]
    fn test_export_with_some_param_some_result() {
        let input = r#"(@interface export "foo" (param String) (result i32 i32))"#;
        let output = Export {
            name: "foo",
            input_types: vec![InterfaceType::String],
            output_types: vec![InterfaceType::I32, InterfaceType::I32],
        };

        assert_eq!(export::<()>(input), Ok(("", output)));
    }

    #[test]
    fn test_export_escaped_name() {
        let input = r#"(@interface export "fo\"o")"#;
        let output = Export {
            name: r#"fo\"o"#,
            input_types: vec![],
            output_types: vec![],
        };

        assert_eq!(export::<()>(input), Ok(("", output)));
    }

    #[test]
    fn test_import_qualifier() {
        let input = r#"(import "ns" "name")"#;
        let output = ("ns", "name");

        assert_eq!(import_qualifier::<()>(input), Ok(("", output)));
    }

    #[test]
    fn test_import_with_no_param_no_result() {
        let input = r#"(@interface func $ns_foo (import "ns" "foo"))"#;
        let output = Import {
            namespace: "ns",
            name: "foo",
            input_types: vec![],
            output_types: vec![],
        };

        assert_eq!(import::<()>(input), Ok(("", output)));
    }

    #[test]
    fn test_import_with_no_index_variable_no_param_no_result() {
        let input = r#"(@interface func (import "ns" "foo"))"#;
        let output = Import {
            namespace: "ns",
            name: "foo",
            input_types: vec![],
            output_types: vec![],
        };

        assert_eq!(import::<()>(input), Ok(("", output)));
    }

    #[test]
    fn test_import_with_some_param_no_result() {
        let input = r#"(@interface func $ns_foo (import "ns" "foo") (param i32))"#;
        let output = Import {
            namespace: "ns",
            name: "foo",
            input_types: vec![InterfaceType::I32],
            output_types: vec![],
        };

        assert_eq!(import::<()>(input), Ok(("", output)));
    }

    #[test]
    fn test_import_with_no_param_some_result() {
        let input = r#"(@interface func $ns_foo (import "ns" "foo") (result i32))"#;
        let output = Import {
            namespace: "ns",
            name: "foo",
            input_types: vec![],
            output_types: vec![InterfaceType::I32],
        };

        assert_eq!(import::<()>(input), Ok(("", output)));
    }

    #[test]
    fn test_import_with_some_param_some_result() {
        let input =
            r#"(@interface func $ns_foo (import "ns" "foo") (param String) (result i32 i32))"#;
        let output = Import {
            namespace: "ns",
            name: "foo",
            input_types: vec![InterfaceType::String],
            output_types: vec![InterfaceType::I32, InterfaceType::I32],
        };

        assert_eq!(import::<()>(input), Ok(("", output)));
    }
}
