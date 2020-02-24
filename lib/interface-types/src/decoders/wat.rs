//! Parse the WIT textual representation into an [AST](crate::ast).

use crate::{ast::*, interpreter::Instruction};
pub use wast::parser::ParseBuffer as Buffer;
use wast::{
    parser::{self, Cursor, Parse, Parser, Peek, Result},
    Id, LParen,
};

mod keyword {
    pub use wast::{
        custom_keyword,
        kw::{anyref, export, f32, f64, func, i32, i64, import, param, result},
    };

    // New keywords.
    custom_keyword!(adapt);
    custom_keyword!(r#type = "type");

    // New types.
    custom_keyword!(s8);
    custom_keyword!(s16);
    custom_keyword!(s32);
    custom_keyword!(s64);
    custom_keyword!(u8);
    custom_keyword!(u16);
    custom_keyword!(u32);
    custom_keyword!(u64);
    custom_keyword!(string);

    // Instructions.
    custom_keyword!(argument_get = "arg.get");
    custom_keyword!(call);
    custom_keyword!(call_export = "call-export");
    custom_keyword!(read_utf8 = "read-utf8");
    custom_keyword!(write_utf8 = "write-utf8");
    custom_keyword!(as_wasm = "as-wasm");
    custom_keyword!(as_interface = "as-interface");
    custom_keyword!(table_ref_add = "table-ref-add");
    custom_keyword!(table_ref_get = "table-ref-get");
    custom_keyword!(call_method = "call-method");
    custom_keyword!(make_record = "make-record");
    custom_keyword!(get_field = "get-field");
    custom_keyword!(r#const = "const");
    custom_keyword!(fold_seq = "fold-seq");
    custom_keyword!(add);
    custom_keyword!(mem_to_seq = "mem-to-seq");
    custom_keyword!(load);
    custom_keyword!(seq_new = "seq.new");
    custom_keyword!(list_push = "list.push");
    custom_keyword!(repeat_until = "repeat-until");
}

impl Parse<'_> for InterfaceType {
    fn parse(parser: Parser<'_>) -> Result<Self> {
        let mut lookahead = parser.lookahead1();

        if lookahead.peek::<keyword::s8>() {
            parser.parse::<keyword::s8>()?;

            Ok(InterfaceType::S8)
        } else if lookahead.peek::<keyword::s16>() {
            parser.parse::<keyword::s16>()?;

            Ok(InterfaceType::S16)
        } else if lookahead.peek::<keyword::s32>() {
            parser.parse::<keyword::s32>()?;

            Ok(InterfaceType::S32)
        } else if lookahead.peek::<keyword::s64>() {
            parser.parse::<keyword::s64>()?;

            Ok(InterfaceType::S64)
        } else if lookahead.peek::<keyword::u8>() {
            parser.parse::<keyword::u8>()?;

            Ok(InterfaceType::U8)
        } else if lookahead.peek::<keyword::u16>() {
            parser.parse::<keyword::u16>()?;

            Ok(InterfaceType::U16)
        } else if lookahead.peek::<keyword::u32>() {
            parser.parse::<keyword::u32>()?;

            Ok(InterfaceType::U32)
        } else if lookahead.peek::<keyword::u64>() {
            parser.parse::<keyword::u64>()?;

            Ok(InterfaceType::U64)
        } else if lookahead.peek::<keyword::f32>() {
            parser.parse::<keyword::f32>()?;

            Ok(InterfaceType::F32)
        } else if lookahead.peek::<keyword::f64>() {
            parser.parse::<keyword::f64>()?;

            Ok(InterfaceType::F64)
        } else if lookahead.peek::<keyword::string>() {
            parser.parse::<keyword::string>()?;

            Ok(InterfaceType::String)
        } else if lookahead.peek::<keyword::string>() {
            parser.parse::<keyword::string>()?;

            Ok(InterfaceType::String)
        } else if lookahead.peek::<keyword::anyref>() {
            parser.parse::<keyword::anyref>()?;

            Ok(InterfaceType::Anyref)
        } else if lookahead.peek::<keyword::i32>() {
            parser.parse::<keyword::i32>()?;

            Ok(InterfaceType::I32)
        } else if lookahead.peek::<keyword::i64>() {
            parser.parse::<keyword::i64>()?;

            Ok(InterfaceType::I64)
        } else {
            Err(lookahead.error())
        }
    }
}

impl<'a> Parse<'a> for Instruction<'a> {
    fn parse(parser: Parser<'a>) -> Result<Self> {
        let mut lookahead = parser.lookahead1();

        if lookahead.peek::<keyword::argument_get>() {
            parser.parse::<keyword::argument_get>()?;

            Ok(Instruction::ArgumentGet {
                index: parser.parse()?,
            })
        } else if lookahead.peek::<keyword::call>() {
            parser.parse::<keyword::call>()?;

            Ok(Instruction::Call {
                function_index: parser.parse::<u64>()? as usize,
            })
        } else if lookahead.peek::<keyword::call_export>() {
            parser.parse::<keyword::call_export>()?;

            Ok(Instruction::CallExport {
                export_name: parser.parse()?,
            })
        } else if lookahead.peek::<keyword::read_utf8>() {
            parser.parse::<keyword::read_utf8>()?;

            Ok(Instruction::ReadUtf8)
        } else if lookahead.peek::<keyword::write_utf8>() {
            parser.parse::<keyword::write_utf8>()?;

            Ok(Instruction::WriteUtf8 {
                allocator_name: parser.parse()?,
            })
        } else if lookahead.peek::<keyword::as_wasm>() {
            parser.parse::<keyword::as_wasm>()?;

            Ok(Instruction::AsWasm(parser.parse()?))
        } else if lookahead.peek::<keyword::as_interface>() {
            parser.parse::<keyword::as_interface>()?;

            Ok(Instruction::AsInterface(parser.parse()?))
        } else if lookahead.peek::<keyword::table_ref_add>() {
            parser.parse::<keyword::table_ref_add>()?;

            Ok(Instruction::TableRefAdd)
        } else if lookahead.peek::<keyword::table_ref_get>() {
            parser.parse::<keyword::table_ref_get>()?;

            Ok(Instruction::TableRefGet)
        } else if lookahead.peek::<keyword::call_method>() {
            parser.parse::<keyword::call_method>()?;

            Ok(Instruction::CallMethod(parser.parse()?))
        } else if lookahead.peek::<keyword::make_record>() {
            parser.parse::<keyword::make_record>()?;

            Ok(Instruction::MakeRecord(parser.parse()?))
        } else if lookahead.peek::<keyword::get_field>() {
            parser.parse::<keyword::get_field>()?;

            Ok(Instruction::GetField(parser.parse()?, parser.parse()?))
        } else if lookahead.peek::<keyword::r#const>() {
            parser.parse::<keyword::r#const>()?;

            Ok(Instruction::Const(parser.parse()?, parser.parse()?))
        } else if lookahead.peek::<keyword::fold_seq>() {
            parser.parse::<keyword::fold_seq>()?;

            Ok(Instruction::FoldSeq(parser.parse()?))
        } else if lookahead.peek::<keyword::add>() {
            parser.parse::<keyword::add>()?;

            Ok(Instruction::Add(parser.parse()?))
        } else if lookahead.peek::<keyword::mem_to_seq>() {
            parser.parse::<keyword::mem_to_seq>()?;

            Ok(Instruction::MemToSeq(parser.parse()?, parser.parse()?))
        } else if lookahead.peek::<keyword::load>() {
            parser.parse::<keyword::load>()?;

            Ok(Instruction::Load(parser.parse()?, parser.parse()?))
        } else if lookahead.peek::<keyword::seq_new>() {
            parser.parse::<keyword::seq_new>()?;

            Ok(Instruction::SeqNew(parser.parse()?))
        } else if lookahead.peek::<keyword::list_push>() {
            parser.parse::<keyword::list_push>()?;

            Ok(Instruction::ListPush)
        } else if lookahead.peek::<keyword::repeat_until>() {
            parser.parse::<keyword::repeat_until>()?;

            Ok(Instruction::RepeatUntil(parser.parse()?, parser.parse()?))
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

            if lookahead.peek::<keyword::param>() {
                parser.parse::<keyword::param>()?;

                let mut inputs = vec![];

                while !parser.is_empty() {
                    inputs.push(parser.parse()?);
                }

                Ok(FunctionType::Input(inputs))
            } else if lookahead.peek::<keyword::result>() {
                parser.parse::<keyword::result>()?;

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
    Type(Type),
    Import(Import<'a>),
    Adapter(Adapter<'a>),
}

impl<'a> Parse<'a> for Interface<'a> {
    fn parse(parser: Parser<'a>) -> Result<Self> {
        parser.parens(|parser| {
            let mut lookahead = parser.lookahead1();

            if lookahead.peek::<AtInterface>() {
                parser.parse::<AtInterface>()?;

                let mut lookahead = parser.lookahead1();

                if lookahead.peek::<keyword::export>() {
                    Ok(Interface::Export(parser.parse()?))
                } else if lookahead.peek::<keyword::func>() {
                    Ok(Interface::Import(parser.parse()?))
                } else if lookahead.peek::<keyword::adapt>() {
                    Ok(Interface::Adapter(parser.parse()?))
                } else if lookahead.peek::<keyword::r#type>() {
                    Ok(Interface::Type(parser.parse()?))
                } else {
                    Err(lookahead.error())
                }
            } else {
                Err(lookahead.error())
            }
        })
    }
}

impl<'a> Parse<'a> for Type {
    fn parse(parser: Parser<'a>) -> Result<Self> {
        parser.parse::<keyword::r#type>()?;

        let (inputs, outputs) = parser.parens(|parser| {
            parser.parse::<keyword::func>()?;

            let mut input_types = vec![];
            let mut output_types = vec![];

            while !parser.is_empty() {
                let function_type = parser.parse::<FunctionType>()?;

                match function_type {
                    FunctionType::Input(mut inputs) => input_types.append(&mut inputs),
                    FunctionType::Output(mut outputs) => output_types.append(&mut outputs),
                }
            }

            Ok((input_types, output_types))
        })?;

        Ok(Type { inputs, outputs })
    }
}

impl<'a> Parse<'a> for Export<'a> {
    fn parse(parser: Parser<'a>) -> Result<Self> {
        parser.parse::<keyword::export>()?;
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
        parser.parse::<keyword::func>()?;
        parser.parse::<Id>()?;

        let (namespace, name) = parser.parens(|parser| {
            parser.parse::<keyword::import>()?;

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
        parser.parse::<keyword::adapt>()?;

        let (kind, namespace, name) = parser.parens(|parser| {
            let mut lookahead = parser.lookahead1();

            if lookahead.peek::<keyword::import>() {
                parser.parse::<keyword::import>()?;

                Ok((AdapterKind::Import, parser.parse()?, parser.parse()?))
            } else if lookahead.peek::<keyword::export>() {
                parser.parse::<keyword::export>()?;

                Ok((AdapterKind::Export, "", parser.parse()?))
            } else {
                Err(lookahead.error())
            }
        })?;
        let mut input_types = vec![];
        let mut output_types = vec![];
        let mut instructions = vec![];

        while !parser.is_empty() {
            if parser.peek::<LParen>() {
                let function_type = parser.parse::<FunctionType>()?;

                match function_type {
                    FunctionType::Input(mut inputs) => input_types.append(&mut inputs),
                    FunctionType::Output(mut outputs) => output_types.append(&mut outputs),
                }
            } else {
                instructions.push(parser.parse()?);
            }
        }

        Ok(match kind {
            AdapterKind::Import => Adapter::Import {
                namespace,
                name,
                input_types,
                output_types,
                instructions,
            },

            AdapterKind::Export => Adapter::Export {
                name,
                input_types,
                output_types,
                instructions,
            },
        })
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
            }
        }

        Ok(interfaces)
    }
}

/// Parse a WIT definition in its textual format, and produces an
/// [AST](crate::ast) with the [`Interfaces`](crate::ast::Interfaces)
/// structure upon succesful.
///
/// # Examples
///
/// ```rust
/// use wasmer_interface_types::{
///     ast::*,
///     decoders::wat::{parse, Buffer},
///     interpreter::Instruction,
/// };
///
/// # fn main() {
/// let input = Buffer::new(
///     r#"(@interface export "foo"
///   (param i32))
///
/// (@interface export "bar")
///
/// (@interface func $ns_foo (import "ns" "foo")
///   (result i32))
///
/// (@interface func $ns_bar (import "ns" "bar"))
///
/// (@interface adapt (import "ns" "foo")
///   (param i32)
///   arg.get 42)
///
/// (@interface adapt (export "bar")
///   arg.get 42)"#,
/// )
/// .unwrap();
/// let output = Interfaces {
///     exports: vec![
///         Export {
///             name: "foo",
///             input_types: vec![InterfaceType::I32],
///             output_types: vec![],
///         },
///         Export {
///             name: "bar",
///             input_types: vec![],
///             output_types: vec![],
///         },
///     ],
///     types: vec![],
///     imports: vec![
///         Import {
///             namespace: "ns",
///             name: "foo",
///             input_types: vec![],
///             output_types: vec![InterfaceType::I32],
///         },
///         Import {
///             namespace: "ns",
///             name: "bar",
///             input_types: vec![],
///             output_types: vec![],
///         },
///     ],
///     adapters: vec![
///         Adapter::Import {
///             namespace: "ns",
///             name: "foo",
///             input_types: vec![InterfaceType::I32],
///             output_types: vec![],
///             instructions: vec![Instruction::ArgumentGet { index: 42 }],
///         },
///         Adapter::Export {
///             name: "bar",
///             input_types: vec![],
///             output_types: vec![],
///             instructions: vec![Instruction::ArgumentGet { index: 42 }],
///         },
///     ],
/// };
///
/// assert_eq!(parse(&input).unwrap(), output);
/// # }
/// ```
pub fn parse<'input>(input: &'input Buffer) -> Result<Interfaces<'input>> {
    parser::parse::<Interfaces>(&input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wast::parser;

    fn buffer(input: &str) -> Buffer {
        Buffer::new(input).expect("Failed to build the parser buffer.")
    }

    #[test]
    fn test_interface_type() {
        let inputs = vec![
            "s8", "s16", "s32", "s64", "u8", "u16", "u32", "u64", "f32", "f64", "string", "anyref",
            "i32", "i64",
        ];
        let outputs = vec![
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
        ];

        assert_eq!(inputs.len(), outputs.len());

        for (input, output) in inputs.iter().zip(outputs.iter()) {
            assert_eq!(
                &parser::parse::<InterfaceType>(&buffer(input)).unwrap(),
                output
            );
        }
    }

    #[test]
    fn test_instructions() {
        let inputs = vec![
            "arg.get 7",
            "call 7",
            r#"call-export "foo""#,
            "read-utf8",
            r#"write-utf8 "foo""#,
            "as-wasm s8",
            "as-interface anyref",
            "table-ref-add",
            "table-ref-get",
            "call-method 7",
            "make-record s8",
            "get-field i32 7",
            "const i32 7",
            "fold-seq 7",
            "add i32",
            r#"mem-to-seq i32 "foo""#,
            r#"load i32 "foo""#,
            "seq.new i32",
            "list.push",
            "repeat-until 1 2",
        ];
        let outputs = vec![
            Instruction::ArgumentGet { index: 7 },
            Instruction::Call { function_index: 7 },
            Instruction::CallExport { export_name: "foo" },
            Instruction::ReadUtf8,
            Instruction::WriteUtf8 {
                allocator_name: "foo",
            },
            Instruction::AsWasm(InterfaceType::S8),
            Instruction::AsInterface(InterfaceType::Anyref),
            Instruction::TableRefAdd,
            Instruction::TableRefGet,
            Instruction::CallMethod(7),
            Instruction::MakeRecord(InterfaceType::S8),
            Instruction::GetField(InterfaceType::I32, 7),
            Instruction::Const(InterfaceType::I32, 7),
            Instruction::FoldSeq(7),
            Instruction::Add(InterfaceType::I32),
            Instruction::MemToSeq(InterfaceType::I32, "foo"),
            Instruction::Load(InterfaceType::I32, "foo"),
            Instruction::SeqNew(InterfaceType::I32),
            Instruction::ListPush,
            Instruction::RepeatUntil(1, 2),
        ];

        assert_eq!(inputs.len(), outputs.len());

        for (input, output) in inputs.iter().zip(outputs.iter()) {
            assert_eq!(
                &parser::parse::<Instruction>(&buffer(input)).unwrap(),
                output
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
    fn test_type() {
        let input = buffer(r#"(@interface type (func (param i32 i32) (result i32)))"#);
        let output = Interface::Type(Type {
            inputs: vec![InterfaceType::I32, InterfaceType::I32],
            outputs: vec![InterfaceType::I32],
        });

        assert_eq!(parser::parse::<Interface>(&input).unwrap(), output);
    }

    #[test]
    fn test_export_with_no_param_no_result() {
        let input = buffer(r#"(@interface export "foo")"#);
        let output = Interface::Export(Export {
            name: "foo",
            input_types: vec![],
            output_types: vec![],
        });

        assert_eq!(parser::parse::<Interface>(&input).unwrap(), output);
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
    fn test_interfaces() {
        let input = buffer(
            r#"(@interface export "foo"
  (param i32))

(@interface export "bar")

(@interface func $ns_foo (import "ns" "foo")
  (result i32))

(@interface func $ns_bar (import "ns" "bar"))

(@interface adapt (import "ns" "foo")
  (param i32)
  arg.get 42)

(@interface adapt (export "bar")
  arg.get 42)"#,
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
                    instructions: vec![Instruction::ArgumentGet { index: 42 }],
                },
                Adapter::Export {
                    name: "bar",
                    input_types: vec![],
                    output_types: vec![],
                    instructions: vec![Instruction::ArgumentGet { index: 42 }],
                },
            ],
        };

        assert_eq!(parser::parse::<Interfaces>(&input).unwrap(), output);
    }
}
