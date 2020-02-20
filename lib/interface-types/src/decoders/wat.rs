//! Parse the WIT textual representation into an AST.

#![allow(unused)]

use crate::ast::*;
use wast::{
    parser::{Cursor, Parse, Parser, Peek, Result},
    Id,
};

mod kw {
    pub use wast::{
        custom_keyword,
        kw::{anyref, export, f32, f64, func, i32, i64, import, param, result},
    };

    custom_keyword!(adapt);
    custom_keyword!(forward);
    custom_keyword!(int);
    custom_keyword!(float);
    custom_keyword!(any);
    custom_keyword!(string);
    custom_keyword!(seq);
}

/// Issue: Uppercased keyword aren't supported for the moment.
impl Parse<'_> for InterfaceType {
    fn parse(parser: Parser<'_>) -> Result<Self> {
        let mut lookahead = parser.lookahead1();

        if lookahead.peek::<kw::int>() {
            parser.parse::<kw::int>()?;

            Ok(InterfaceType::Int)
        } else if lookahead.peek::<kw::float>() {
            parser.parse::<kw::float>()?;

            Ok(InterfaceType::Float)
        } else if lookahead.peek::<kw::any>() {
            parser.parse::<kw::any>()?;

            Ok(InterfaceType::Any)
        } else if lookahead.peek::<kw::string>() {
            parser.parse::<kw::string>()?;

            Ok(InterfaceType::String)
        } else if lookahead.peek::<kw::seq>() {
            parser.parse::<kw::seq>()?;

            Ok(InterfaceType::Seq)
        } else if lookahead.peek::<kw::i32>() {
            parser.parse::<kw::i32>()?;

            Ok(InterfaceType::I32)
        } else if lookahead.peek::<kw::i64>() {
            parser.parse::<kw::i64>()?;

            Ok(InterfaceType::I64)
        } else if lookahead.peek::<kw::f32>() {
            parser.parse::<kw::f32>()?;

            Ok(InterfaceType::F32)
        } else if lookahead.peek::<kw::f64>() {
            parser.parse::<kw::f64>()?;

            Ok(InterfaceType::F64)
        } else if lookahead.peek::<kw::anyref>() {
            parser.parse::<kw::anyref>()?;

            Ok(InterfaceType::AnyRef)
        } else {
            Err(lookahead.error())
        }
    }
}

struct AtInterface;

impl Peek for AtInterface {
    fn peek(cursor: Cursor<'_>) -> bool {
        cursor.reserved().map(|(string, _)| string) == Some("@interface")
    }

    fn display() -> &'static str {
        "`@interface`"
    }
}

impl Parse<'_> for AtInterface {
    fn parse(parser: Parser<'_>) -> Result<Self> {
        parser.step(|cursor| {
            if let Some(("@interface", rest)) = cursor.reserved() {
                return Ok((AtInterface, rest));
            }

            Err(cursor.error("expected `@interface`"))
        })
    }
}

#[derive(PartialEq, Debug)]
enum FunctionType {
    Input(Vec<InterfaceType>),
    Output(Vec<InterfaceType>),
}

impl Parse<'_> for FunctionType {
    fn parse(parser: Parser<'_>) -> Result<Self> {
        parser.parens(|parser| {
            let mut lookahead = parser.lookahead1();

            if lookahead.peek::<kw::param>() {
                parser.parse::<kw::param>()?;

                let mut inputs = vec![];

                while !parser.is_empty() {
                    inputs.push(parser.parse()?);
                }

                Ok(FunctionType::Input(inputs))
            } else if lookahead.peek::<kw::result>() {
                parser.parse::<kw::result>()?;

                let mut outputs = vec![];

                while !parser.is_empty() {
                    outputs.push(parser.parse()?);
                }

                Ok(FunctionType::Output(outputs))
            } else {
                Err(lookahead.error())
            }
        })
    }
}

#[derive(PartialEq, Debug)]
enum Interface<'a> {
    Export(Export<'a>),
    Type(Type<'a>),
    Import(Import<'a>),
    Adapter(Adapter<'a>),
    Forward(Forward<'a>),
}

impl<'a> Parse<'a> for Interface<'a> {
    fn parse(parser: Parser<'a>) -> Result<Self> {
        parser.parens(|parser| {
            let mut lookahead = parser.lookahead1();

            if lookahead.peek::<AtInterface>() {
                parser.parse::<AtInterface>()?;

                let mut lookahead = parser.lookahead1();

                if lookahead.peek::<kw::export>() {
                    Ok(Interface::Export(parser.parse()?))
                } else if lookahead.peek::<kw::func>() {
                    Ok(Interface::Import(parser.parse()?))
                } else if lookahead.peek::<kw::adapt>() {
                    Ok(Interface::Adapter(parser.parse()?))
                } else if lookahead.peek::<kw::forward>() {
                    Ok(Interface::Forward(parser.parse()?))
                } else {
                    Err(lookahead.error())
                }
            } else {
                Err(lookahead.error())
            }
        })
    }
}

impl<'a> Parse<'a> for Export<'a> {
    fn parse(parser: Parser<'a>) -> Result<Self> {
        parser.parse::<kw::export>()?;
        let name = parser.parse()?;

        let mut input_types = vec![];
        let mut output_types = vec![];

        while !parser.is_empty() {
            let function_type = parser.parse::<FunctionType>()?;

            match function_type {
                FunctionType::Input(mut inputs) => input_types.append(&mut inputs),
                FunctionType::Output(mut outputs) => output_types.append(&mut outputs),
            }
        }

        Ok(Export {
            name,
            input_types,
            output_types,
        })
    }
}

impl<'a> Parse<'a> for Import<'a> {
    fn parse(parser: Parser<'a>) -> Result<Self> {
        parser.parse::<kw::func>()?;
        parser.parse::<Id>()?;

        let (namespace, name) = parser.parens(|parser| {
            parser.parse::<kw::import>()?;

            Ok((parser.parse()?, parser.parse()?))
        })?;
        let mut input_types = vec![];
        let mut output_types = vec![];

        while !parser.is_empty() {
            let function_type = parser.parse::<FunctionType>()?;

            match function_type {
                FunctionType::Input(mut inputs) => input_types.append(&mut inputs),
                FunctionType::Output(mut outputs) => output_types.append(&mut outputs),
            }
        }

        Ok(Import {
            namespace,
            name,
            input_types,
            output_types,
        })
    }
}

impl<'a> Parse<'a> for Adapter<'a> {
    fn parse(parser: Parser<'a>) -> Result<Self> {
        parser.parse::<kw::adapt>()?;

        let (kind, namespace, name) = parser.parens(|parser| {
            let mut lookahead = parser.lookahead1();

            if lookahead.peek::<kw::import>() {
                parser.parse::<kw::import>()?;

                Ok((AdapterKind::Import, parser.parse()?, parser.parse()?))
            } else if lookahead.peek::<kw::export>() {
                parser.parse::<kw::export>()?;

                Ok((AdapterKind::Export, "", parser.parse()?))
            } else {
                Err(lookahead.error())
            }
        })?;
        let mut input_types = vec![];
        let mut output_types = vec![];

        while !parser.is_empty() {
            let function_type = parser.parse::<FunctionType>()?;

            match function_type {
                FunctionType::Input(mut inputs) => input_types.append(&mut inputs),
                FunctionType::Output(mut outputs) => output_types.append(&mut outputs),
            }
        }

        Ok(match kind {
            AdapterKind::Import => Adapter::Import {
                namespace,
                name,
                input_types,
                output_types,
                instructions: vec![],
            },

            AdapterKind::Export => Adapter::Export {
                name,
                input_types,
                output_types,
                instructions: vec![],
            },

            _ => unimplemented!("Adapter of kind “helper” is not implemented yet."),
        })
    }
}

impl<'a> Parse<'a> for Forward<'a> {
    fn parse(parser: Parser<'a>) -> Result<Self> {
        parser.parse::<kw::forward>()?;

        let name = parser.parens(|parser| {
            parser.parse::<kw::export>()?;

            Ok((parser.parse()?))
        })?;

        Ok(Forward { name })
    }
}

impl<'a> Parse<'a> for Interfaces<'a> {
    fn parse(parser: Parser<'a>) -> Result<Self> {
        let mut interfaces: Interfaces = Default::default();

        while !parser.is_empty() {
            let interface = parser.parse::<Interface>()?;

            match interface {
                Interface::Export(export) => interfaces.exports.push(export),
                Interface::Type(ty) => interfaces.types.push(ty),
                Interface::Import(import) => interfaces.imports.push(import),
                Interface::Adapter(adapter) => interfaces.adapters.push(adapter),
                Interface::Forward(forward) => interfaces.forwards.push(forward),
            }
        }

        Ok(interfaces)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wast::parser::{self, ParseBuffer};

    fn buffer(input: &str) -> ParseBuffer {
        ParseBuffer::new(input).expect("Failed to build the parser buffer.")
    }

    #[test]
    fn test_interface_type() {
        let inputs = vec![
            "int", "float", "any", "string", "seq", "i32", "i64", "f32", "f64", "anyref",
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
            assert_eq!(
                parser::parse::<InterfaceType>(&buffer(input)).unwrap(),
                outputs[nth]
            );
        }
    }

    #[test]
    fn test_param_empty() {
        let input = buffer("(param)");
        let output = FunctionType::Input(vec![]);

        assert_eq!(parser::parse::<FunctionType>(&input).unwrap(), output);
    }

    #[test]
    fn test_param() {
        let input = buffer("(param i32 string)");
        let output = FunctionType::Input(vec![InterfaceType::I32, InterfaceType::String]);

        assert_eq!(parser::parse::<FunctionType>(&input).unwrap(), output);
    }

    #[test]
    fn test_result_empty() {
        let input = buffer("(result)");
        let output = FunctionType::Output(vec![]);

        assert_eq!(parser::parse::<FunctionType>(&input).unwrap(), output);
    }

    #[test]
    fn test_result() {
        let input = buffer("(result i32 string)");
        let output = FunctionType::Output(vec![InterfaceType::I32, InterfaceType::String]);

        assert_eq!(parser::parse::<FunctionType>(&input).unwrap(), output);
    }

    #[test]
    fn test_export_with_no_param_no_result() {
        let input = buffer(r#"(@interface export "foo")"#);
    }

    #[test]
    fn test_export_with_some_param_no_result() {
        let input = buffer(r#"(@interface export "foo" (param i32))"#);
        let output = Interface::Export(Export {
            name: "foo",
            input_types: vec![InterfaceType::I32],
            output_types: vec![],
        });

        assert_eq!(parser::parse::<Interface>(&input).unwrap(), output);
    }

    #[test]
    fn test_export_with_no_param_some_result() {
        let input = buffer(r#"(@interface export "foo" (result i32))"#);
        let output = Interface::Export(Export {
            name: "foo",
            input_types: vec![],
            output_types: vec![InterfaceType::I32],
        });

        assert_eq!(parser::parse::<Interface>(&input).unwrap(), output);
    }

    #[test]
    fn test_export_with_some_param_some_result() {
        let input = buffer(r#"(@interface export "foo" (param string) (result i32 i32))"#);
        let output = Interface::Export(Export {
            name: "foo",
            input_types: vec![InterfaceType::String],
            output_types: vec![InterfaceType::I32, InterfaceType::I32],
        });

        assert_eq!(parser::parse::<Interface>(&input).unwrap(), output);
    }

    #[test]
    fn test_export_escaped_name() {
        let input = buffer(r#"(@interface export "fo\"o")"#);
        let output = Interface::Export(Export {
            name: r#"fo"o"#,
            input_types: vec![],
            output_types: vec![],
        });

        assert_eq!(parser::parse::<Interface>(&input).unwrap(), output);
    }

    #[test]
    fn test_import_with_no_param_no_result() {
        let input = buffer(r#"(@interface func $ns_foo (import "ns" "foo"))"#);
        let output = Interface::Import(Import {
            namespace: "ns",
            name: "foo",
            input_types: vec![],
            output_types: vec![],
        });

        assert_eq!(parser::parse::<Interface>(&input).unwrap(), output);
    }

    #[test]
    fn test_import_with_some_param_no_result() {
        let input = buffer(r#"(@interface func $ns_foo (import "ns" "foo") (param i32))"#);
        let output = Interface::Import(Import {
            namespace: "ns",
            name: "foo",
            input_types: vec![InterfaceType::I32],
            output_types: vec![],
        });

        assert_eq!(parser::parse::<Interface>(&input).unwrap(), output);
    }

    #[test]
    fn test_import_with_no_param_some_result() {
        let input = buffer(r#"(@interface func $ns_foo (import "ns" "foo") (result i32))"#);
        let output = Interface::Import(Import {
            namespace: "ns",
            name: "foo",
            input_types: vec![],
            output_types: vec![InterfaceType::I32],
        });

        assert_eq!(parser::parse::<Interface>(&input).unwrap(), output);
    }

    #[test]
    fn test_import_with_some_param_some_result() {
        let input = buffer(
            r#"(@interface func $ns_foo (import "ns" "foo") (param string) (result i32 i32))"#,
        );
        let output = Interface::Import(Import {
            namespace: "ns",
            name: "foo",
            input_types: vec![InterfaceType::String],
            output_types: vec![InterfaceType::I32, InterfaceType::I32],
        });

        assert_eq!(parser::parse::<Interface>(&input).unwrap(), output);
    }

    #[test]
    fn test_adapter_import() {
        let input =
            buffer(r#"(@interface adapt (import "ns" "foo") (param i32 i32) (result i32))"#);
        let output = Interface::Adapter(Adapter::Import {
            namespace: "ns",
            name: "foo",
            input_types: vec![InterfaceType::I32, InterfaceType::I32],
            output_types: vec![InterfaceType::I32],
            instructions: vec![],
        });

        assert_eq!(parser::parse::<Interface>(&input).unwrap(), output);
    }

    #[test]
    fn test_adapter_export() {
        let input = buffer(r#"(@interface adapt (export "foo") (param i32 i32) (result i32))"#);
        let output = Interface::Adapter(Adapter::Export {
            name: "foo",
            input_types: vec![InterfaceType::I32, InterfaceType::I32],
            output_types: vec![InterfaceType::I32],
            instructions: vec![],
        });

        assert_eq!(parser::parse::<Interface>(&input).unwrap(), output);
    }
    #[test]
    fn test_forward() {
        let input = buffer(r#"(@interface forward (export "foo"))"#);
        let output = Interface::Forward(Forward { name: "foo" });

        assert_eq!(parser::parse::<Interface>(&input).unwrap(), output);
    }

    #[test]
    fn test_interfaces() {
        let input = buffer(
            r#"(@interface export "foo"
  (param i32))

(@interface export "bar")

(@interface func $ns_foo (import "ns" "foo")
  (result i32))

(@interface func $ns_bar (import "ns" "bar"))

(@interface adapt (import "ns" "foo")
  (param i32))

(@interface adapt (export "bar"))

(@interface forward (export "main"))"#,
        );
        let output = Interfaces {
            exports: vec![
                Export {
                    name: "foo",
                    input_types: vec![InterfaceType::I32],
                    output_types: vec![],
                },
                Export {
                    name: "bar",
                    input_types: vec![],
                    output_types: vec![],
                },
            ],
            types: vec![],
            imports: vec![
                Import {
                    namespace: "ns",
                    name: "foo",
                    input_types: vec![],
                    output_types: vec![InterfaceType::I32],
                },
                Import {
                    namespace: "ns",
                    name: "bar",
                    input_types: vec![],
                    output_types: vec![],
                },
            ],
            adapters: vec![
                Adapter::Import {
                    namespace: "ns",
                    name: "foo",
                    input_types: vec![InterfaceType::I32],
                    output_types: vec![],
                    instructions: vec![],
                },
                Adapter::Export {
                    name: "bar",
                    input_types: vec![],
                    output_types: vec![],
                    instructions: vec![],
                },
            ],
            forwards: vec![Forward { name: "main" }],
        };

        assert_eq!(parser::parse::<Interfaces>(&input).unwrap(), output);
    }
}
