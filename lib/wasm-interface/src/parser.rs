//! Parsers to get a wasm interface from text
//!
//! The grammar of the text format is:
//! interface = "(" interface name? interface-entry* ")"
//! interface-entry = func | global
//!
//! func = import-fn | export-fn
//! global = import-global | export-global
//!
//! import-fn = "(" "func" import-id param-list? result-list? ")"
//! import-global = "(" "global" import-id type-decl ")"
//! import-id = "(" "import" namespace name ")"
//!
//! export-fn = "(" "func" export-id param-list? result-list? ")"
//! export-global = "(" "global" export-id type-decl ")"
//! export-id = "(" export name ")"
//!
//! param-list = "(" param type* ")"
//! result-list = "(" result type* ")"
//! type-decl = "(" "type" type ")"
//! namespace = "\"" identifier "\""
//! name = "\"" identifier "\""
//! identifier = any character that's not a whitespace character or an open or close parenthesis
//! type = "i32" | "i64" | "f32" | "f64"
//!
//! + means 1 or more
//! * means 0 or more
//! ? means 0 or 1
//! | means "or"
//! "\"" means one `"` character
//!
//! comments start with a `;` character and go until a newline `\n` character is reached
//! comments and whitespace are valid between any tokens

use either::Either;
use nom::{
    branch::*,
    bytes::complete::{escaped, is_not, tag},
    character::complete::{char, multispace0, multispace1, one_of},
    combinator::*,
    error::context,
    multi::many0,
    sequence::{delimited, preceded, tuple},
    IResult,
};

use crate::interface::*;

/// Some example input:
/// (interface "example_interface"
///     (func (import "ns" "name") (param f64 i32) (result f64 i32))
///     (func (export "name") (param f64 i32) (result f64 i32))
///     (global (import "ns" "name") (type f64)))
pub fn parse_interface(mut input: &str) -> Result<Interface, String> {
    let mut interface = Interface::default();
    let interface_inner = preceded(
        tag("interface"),
        tuple((
            opt(preceded(space_comments, identifier)),
            many0(parse_func_or_global),
        )),
    );
    let interface_parser = preceded(space_comments, s_exp(interface_inner));

    if let Result::Ok((inp, (sig_id, out))) = interface_parser(input) {
        interface.name = sig_id.map(|s_id| s_id.to_string());

        for entry in out.into_iter() {
            match entry {
                Either::Left(import) => {
                    if let Some(dup) = interface.imports.insert(import.get_key(), import) {
                        return Err(format!("Duplicate import found {:?}", dup));
                    }
                }
                Either::Right(export) => {
                    if let Some(dup) = interface.exports.insert(export.get_key(), export) {
                        return Err(format!("Duplicate export found {:?}", dup));
                    }
                }
            }
        }
        input = inp;
    }
    // catch trailing comments and spaces
    if let Ok((inp, _)) = space_comments(input) {
        input = inp;
    }
    if !input.is_empty() {
        Err(format!("Could not parse remaining input: {}", input))
    } else {
        Ok(interface)
    }
}

fn parse_comment(input: &str) -> IResult<&str, ()> {
    map(
        preceded(multispace0, preceded(char(';'), many0(is_not("\n")))),
        |_| (),
    )(input)
}

/// Consumes spaces and comments
/// comments must terminate with a new line character
fn space_comments<'a>(mut input: &'a str) -> IResult<&'a str, ()> {
    let mut space_found = true;
    let mut comment_found = true;
    while space_found || comment_found {
        let space: IResult<&'a str, _> = multispace1(input);
        space_found = if let Result::Ok((inp, _)) = space {
            input = inp;
            true
        } else {
            false
        };
        comment_found = if let Result::Ok((inp, _)) = parse_comment(input) {
            input = inp;
            true
        } else {
            false
        };
    }
    Ok((input, ()))
}

/// A quoted identifier, must be valid UTF8
fn identifier(input: &str) -> IResult<&str, &str> {
    let name_inner = escaped(is_not("\"\\"), '\\', one_of("\"n\\"));
    context("identifier", delimited(char('"'), name_inner, char('"')))(input)
}

/// Parses a wasm primitive type
fn wasm_type(input: &str) -> IResult<&str, WasmType> {
    let i32_tag = map(tag("i32"), |_| WasmType::I32);
    let i64_tag = map(tag("i64"), |_| WasmType::I64);
    let f32_tag = map(tag("f32"), |_| WasmType::F32);
    let f64_tag = map(tag("f64"), |_| WasmType::F64);

    alt((i32_tag, i64_tag, f32_tag, f64_tag))(input)
}

/// Parses an S-expression
fn s_exp<'a, O1, F>(inner: F) -> impl Fn(&'a str) -> IResult<&'a str, O1>
where
    F: Fn(&'a str) -> IResult<&'a str, O1>,
{
    delimited(
        char('('),
        preceded(space_comments, inner),
        preceded(space_comments, char(')')),
    )
}

fn parse_func_or_global(input: &str) -> IResult<&str, Either<Import, Export>> {
    preceded(space_comments, alt((func, global)))(input)
}

/// (func (import "ns" "name") (param f64 i32) (result f64 i32))
/// (func (export "name") (param f64 i32) (result f64 i32))
fn func(input: &str) -> IResult<&str, Either<Import, Export>> {
    let param_list_inner = preceded(tag("param"), many0(preceded(space_comments, wasm_type)));
    let param_list = opt(s_exp(param_list_inner));
    let result_list_inner = preceded(tag("result"), many0(preceded(space_comments, wasm_type)));
    let result_list = opt(s_exp(result_list_inner));
    let import_id_inner = preceded(
        tag("import"),
        tuple((
            preceded(space_comments, identifier),
            preceded(space_comments, identifier),
        )),
    );
    let export_id_inner = preceded(tag("export"), preceded(space_comments, identifier));
    let func_id_inner = alt((
        map(import_id_inner, |(ns, name)| {
            Either::Left((ns.to_string(), name.to_string()))
        }),
        map(export_id_inner, |name| Either::Right(name.to_string())),
    ));
    let func_id = s_exp(func_id_inner);
    let func_import_inner = context(
        "func import inner",
        preceded(
            tag("func"),
            map(
                tuple((
                    preceded(space_comments, func_id),
                    preceded(space_comments, param_list),
                    preceded(space_comments, result_list),
                )),
                |(func_id, pl, rl)| match func_id {
                    Either::Left((ns, name)) => Either::Left(Import::Func {
                        namespace: ns,
                        name,
                        params: pl.unwrap_or_default(),
                        result: rl.unwrap_or_default(),
                    }),
                    Either::Right(name) => Either::Right(Export::Func {
                        name,
                        params: pl.unwrap_or_default(),
                        result: rl.unwrap_or_default(),
                    }),
                },
            ),
        ),
    );
    s_exp(func_import_inner)(input)
}

/// (global (import "ns" "name") (type f64))
/// (global (export "name") (type f64))
fn global(input: &str) -> IResult<&str, Either<Import, Export>> {
    let global_type_inner = preceded(tag("type"), preceded(space_comments, wasm_type));
    let type_s_exp = s_exp(global_type_inner);
    let export_inner = preceded(tag("export"), preceded(space_comments, identifier));
    let import_inner = preceded(
        tag("import"),
        tuple((
            preceded(space_comments, identifier),
            preceded(space_comments, identifier),
        )),
    );
    let global_id_inner = alt((
        map(import_inner, |(ns, name)| {
            Either::Left(Import::Global {
                namespace: ns.to_string(),
                name: name.to_string(),
                // placeholder type, overwritten in `global_inner`
                var_type: WasmType::I32,
            })
        }),
        map(export_inner, |name| {
            Either::Right(Export::Global {
                name: name.to_string(),
                // placeholder type, overwritten in `global_inner`
                var_type: WasmType::I32,
            })
        }),
    ));
    let global_id = s_exp(global_id_inner);
    let global_inner = context(
        "global inner",
        preceded(
            tag("global"),
            map(
                tuple((
                    preceded(space_comments, global_id),
                    preceded(space_comments, type_s_exp),
                )),
                |(import_or_export, var_type)| match import_or_export {
                    Either::Left(Import::Global {
                        namespace, name, ..
                    }) => Either::Left(Import::Global {
                        namespace,
                        name,
                        var_type,
                    }),
                    Either::Right(Export::Global { name, .. }) => {
                        Either::Right(Export::Global { name, var_type })
                    }
                    _ => unreachable!("Invalid value interonally in parse global function"),
                },
            ),
        ),
    );
    s_exp(global_inner)(input)
}

#[cfg(test)]
mod test {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn parse_wasm_type() {
        let i32_res = wasm_type("i32").unwrap();
        assert_eq!(i32_res, ("", WasmType::I32));
        let i64_res = wasm_type("i64").unwrap();
        assert_eq!(i64_res, ("", WasmType::I64));
        let f32_res = wasm_type("f32").unwrap();
        assert_eq!(f32_res, ("", WasmType::F32));
        let f64_res = wasm_type("f64").unwrap();
        assert_eq!(f64_res, ("", WasmType::F64));

        assert!(wasm_type("i128").is_err());
    }

    #[test]
    fn parse_identifier() {
        let inner_str = "柴は可愛すぎるだと思います";
        let input = format!("\"{}\"", &inner_str);
        let parse_res = identifier(&input).unwrap();
        assert_eq!(parse_res, ("", inner_str))
    }

    #[test]
    fn parse_global_import() {
        let parse_res = global(r#"(global (import "env" "length") (type i32))"#)
            .ok()
            .and_then(|(a, b)| Some((a, b.left()?)))
            .unwrap();
        assert_eq!(
            parse_res,
            (
                "",
                Import::Global {
                    namespace: "env".to_string(),
                    name: "length".to_string(),
                    var_type: WasmType::I32,
                }
            )
        );
    }

    #[test]
    fn parse_global_export() {
        let parse_res = global(r#"(global (export "length") (type i32))"#)
            .ok()
            .and_then(|(a, b)| Some((a, b.right()?)))
            .unwrap();
        assert_eq!(
            parse_res,
            (
                "",
                Export::Global {
                    name: "length".to_string(),
                    var_type: WasmType::I32,
                }
            )
        );
    }

    #[test]
    fn parse_func_import() {
        let parse_res = func(r#"(func (import "ns" "name") (param f64 i32) (result f64 i32))"#)
            .ok()
            .and_then(|(a, b)| Some((a, b.left()?)))
            .unwrap();
        assert_eq!(
            parse_res,
            (
                "",
                Import::Func {
                    namespace: "ns".to_string(),
                    name: "name".to_string(),
                    params: vec![WasmType::F64, WasmType::I32],
                    result: vec![WasmType::F64, WasmType::I32],
                }
            )
        );
    }

    #[test]
    fn parse_func_export() {
        let parse_res = func(r#"(func (export "name") (param f64 i32) (result f64 i32))"#)
            .ok()
            .and_then(|(a, b)| Some((a, b.right()?)))
            .unwrap();
        assert_eq!(
            parse_res,
            (
                "",
                Export::Func {
                    name: "name".to_string(),
                    params: vec![WasmType::F64, WasmType::I32],
                    result: vec![WasmType::F64, WasmType::I32],
                }
            )
        );

        let parse_res = func(r#"(func (export "name"))"#)
            .ok()
            .and_then(|(a, b)| Some((a, b.right()?)))
            .unwrap();
        assert_eq!(
            parse_res,
            (
                "",
                Export::Func {
                    name: "name".to_string(),
                    params: vec![],
                    result: vec![],
                }
            )
        )
    }

    #[test]
    fn parse_imports_test() {
        let parse_imports = |in_str| {
            many0(parse_func_or_global)(in_str)
                .map(|(a, b)| {
                    (
                        a,
                        b.into_iter().filter_map(|x| x.left()).collect::<Vec<_>>(),
                    )
                })
                .unwrap()
        };
        let parse_res =
            parse_imports(r#"(func (import "ns" "name") (param f64 i32) (result f64 i32))"#);
        assert_eq!(
            parse_res,
            (
                "",
                vec![Import::Func {
                    namespace: "ns".to_string(),
                    name: "name".to_string(),
                    params: vec![WasmType::F64, WasmType::I32],
                    result: vec![WasmType::F64, WasmType::I32],
                }]
            )
        );

        let parse_res = parse_imports(
            r#"(func (import "ns" "name")
                                                   (param f64 i32) (result f64 i32))
        ( global ( import "env" "length" ) ( type
    ;; i32 is the best type
    i32 )
    )
                                              (func (import "ns" "name2") (param f32
                                                                          i64)
                                   ;; The return value comes next
                                                                    (
                                                                     result
                                                                     f64
                                                                     i32
                                                                     )
                                              )"#,
        );
        assert_eq!(
            parse_res,
            (
                "",
                vec![
                    Import::Func {
                        namespace: "ns".to_string(),
                        name: "name".to_string(),
                        params: vec![WasmType::F64, WasmType::I32],
                        result: vec![WasmType::F64, WasmType::I32],
                    },
                    Import::Global {
                        namespace: "env".to_string(),
                        name: "length".to_string(),
                        var_type: WasmType::I32,
                    },
                    Import::Func {
                        namespace: "ns".to_string(),
                        name: "name2".to_string(),
                        params: vec![WasmType::F32, WasmType::I64],
                        result: vec![WasmType::F64, WasmType::I32],
                    },
                ]
            )
        );
    }

    #[test]
    fn top_level_test() {
        let parse_res = parse_interface(
            r#" (interface 
 (func (import "ns" "name") (param f64 i32) (result f64 i32))
 (func (export "name2") (param) (result i32))
 (global (import "env" "length") (type f64)))"#,
        )
        .unwrap();

        let imports = vec![
            Import::Func {
                namespace: "ns".to_string(),
                name: "name".to_string(),
                params: vec![WasmType::F64, WasmType::I32],
                result: vec![WasmType::F64, WasmType::I32],
            },
            Import::Global {
                namespace: "env".to_string(),
                name: "length".to_string(),
                var_type: WasmType::F64,
            },
        ];
        let exports = vec![Export::Func {
            name: "name2".to_string(),
            params: vec![],
            result: vec![WasmType::I32],
        }];
        let import_map = imports
            .into_iter()
            .map(|entry| (entry.get_key(), entry))
            .collect::<HashMap<(String, String), Import>>();
        let export_map = exports
            .into_iter()
            .map(|entry| (entry.get_key(), entry))
            .collect::<HashMap<String, Export>>();
        assert_eq!(
            parse_res,
            Interface {
                name: None,
                imports: import_map,
                exports: export_map,
            }
        );
    }

    #[test]
    fn duplicates_not_allowed() {
        let parse_res = parse_interface(
            r#" (interface "sig_name" (func (import "ns" "name") (param f64 i32) (result f64 i32))
; test comment
  ;; hello
 (func (import "ns" "name") (param) (result i32))
 (global (export "length") (type f64)))

"#,
        );

        assert!(parse_res.is_err());
    }

    #[test]
    fn test_comment_space_parsing() {
        let parse_res = space_comments(" ").unwrap();
        assert_eq!(parse_res, ("", ()));
        let parse_res = space_comments("").unwrap();
        assert_eq!(parse_res, ("", ()));
        let parse_res = space_comments("; hello\n").unwrap();
        assert_eq!(parse_res, ("", ()));
        let parse_res = space_comments("abc").unwrap();
        assert_eq!(parse_res, ("abc", ()));
        let parse_res = space_comments("\n ; hello\n ").unwrap();
        assert_eq!(parse_res, ("", ()));
        let parse_res = space_comments("\n ; hello\n ; abc\n\n ; hello\n").unwrap();
        assert_eq!(parse_res, ("", ()));
    }

    #[test]
    fn test_param_elision() {
        let parse_res = parse_interface(
            r#" (interface "interface_name" (func (import "ns" "name") (result f64 i32))
(func (export "name")))
"#,
        )
        .unwrap();

        let imports = vec![Import::Func {
            namespace: "ns".to_string(),
            name: "name".to_string(),
            params: vec![],
            result: vec![WasmType::F64, WasmType::I32],
        }];
        let exports = vec![Export::Func {
            name: "name".to_string(),
            params: vec![],
            result: vec![],
        }];
        let import_map = imports
            .into_iter()
            .map(|entry| (entry.get_key(), entry))
            .collect::<HashMap<(String, String), Import>>();
        let export_map = exports
            .into_iter()
            .map(|entry| (entry.get_key(), entry))
            .collect::<HashMap<String, Export>>();
        assert_eq!(
            parse_res,
            Interface {
                name: Some("interface_name".to_string()),
                imports: import_map,
                exports: export_map,
            }
        );
    }

    #[test]
    fn typo_gets_caught() {
        let interface_src = r#"
(interface "interface_id"
(func (import "env" "do_panic") (params i32 i64))
(global (import "length") (type i32)))"#;
        let result = parse_interface(interface_src);
        assert!(result.is_err());
    }

    #[test]
    fn parse_trailing_spaces_on_interface() {
        let parse_res = parse_interface(
            r#" (interface "really_good_interface" (func (import "ns" "name") (param f64 i32) (result f64 i32))
; test comment
  ;; hello
 (global (import "ns" "length") (type f64))
)

"#,
        );

        assert!(parse_res.is_ok());
    }
}
