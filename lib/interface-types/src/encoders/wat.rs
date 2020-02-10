use crate::{
    ast::{Adapter, Export, Forward, ImportedFunction, InterfaceType, Interfaces, Type},
    interpreter::Instruction,
};

impl From<&InterfaceType> for String {
    fn from(interface_type: &InterfaceType) -> Self {
        match interface_type {
            InterfaceType::Int => "Int".into(),
            InterfaceType::Float => "Float".into(),
            InterfaceType::Any => "Any".into(),
            InterfaceType::String => "String".into(),
            InterfaceType::Seq => "Seq".into(),
            InterfaceType::I32 => "i32".into(),
            InterfaceType::I64 => "i64".into(),
            InterfaceType::F32 => "f32".into(),
            InterfaceType::F64 => "f64".into(),
            InterfaceType::AnyRef => "anyref".into(),
        }
    }
}

impl<'input> From<&Instruction<'input>> for String {
    fn from(instruction: &Instruction) -> Self {
        match instruction {
            Instruction::ArgumentGet { index } => format!("arg.get {}", index),
            Instruction::Call { function_index } => format!("call {}", function_index),
            Instruction::CallExport { export_name } => format!(r#"call-export "{}""#, export_name),
            Instruction::ReadUtf8 => "read-utf8".into(),
            Instruction::WriteUtf8 { allocator_name } => {
                format!(r#"write-utf8 "{}""#, allocator_name)
            }
            Instruction::AsWasm(interface_type) => {
                format!("as-wasm {}", String::from(interface_type))
            }
            Instruction::AsInterface(interface_type) => {
                format!("as-interface {}", String::from(interface_type))
            }
            Instruction::TableRefAdd => "table-ref-add".into(),
            Instruction::TableRefGet => "table-ref-get".into(),
            Instruction::CallMethod(index) => format!("call-method {}", index),
            Instruction::MakeRecord(interface_type) => {
                format!("make-record {}", String::from(interface_type))
            }
            Instruction::GetField(interface_type, field_index) => {
                format!("get-field {} {}", String::from(interface_type), field_index)
            }
            Instruction::Const(interface_type, value) => {
                format!("const {} {}", String::from(interface_type), value)
            }
            Instruction::FoldSeq(import_index) => format!("fold-seq {}", import_index),
            Instruction::Add(interface_type) => format!("add {}", String::from(interface_type)),
            Instruction::MemToSeq(interface_type, memory) => format!(
                r#"mem-to-seq {} "{}""#,
                String::from(interface_type),
                memory
            ),
            Instruction::Load(interface_type, memory) => {
                format!(r#"load {} "{}""#, String::from(interface_type), memory)
            }
            Instruction::SeqNew(interface_type) => {
                format!("seq.new {}", String::from(interface_type))
            }
            Instruction::ListPush => "list.push".into(),
            Instruction::RepeatWhile(condition_index, step_index) => {
                format!("repeat-while {} {}", condition_index, step_index)
            }
        }
    }
}

fn input_types_to_param(input_types: &[InterfaceType]) -> String {
    if input_types.is_empty() {
        "".into()
    } else {
        format!(
            "\n  (param{})",
            input_types
                .iter()
                .fold(String::new(), |mut accumulator, interface_type| {
                    accumulator.push(' ');
                    accumulator.push_str(&String::from(interface_type));
                    accumulator
                })
        )
    }
}

fn output_types_to_result(output_types: &[InterfaceType]) -> String {
    if output_types.is_empty() {
        "".into()
    } else {
        format!(
            "\n  (result{})",
            output_types
                .iter()
                .fold(String::new(), |mut accumulator, interface_type| {
                    accumulator.push(' ');
                    accumulator.push_str(&String::from(interface_type));
                    accumulator
                })
        )
    }
}

impl<'input> From<&Export<'input>> for String {
    fn from(export: &Export) -> Self {
        format!(
            r#"(@interface export "{name}"{inputs}{outputs})"#,
            name = export.name,
            inputs = input_types_to_param(&export.input_types),
            outputs = output_types_to_result(&export.output_types),
        )
    }
}

impl<'input> From<&Type<'input>> for String {
    fn from(_ty: &Type) -> Self {
        unimplemented!()
    }
}

impl<'input> From<&ImportedFunction<'input>> for String {
    fn from(imported_function: &ImportedFunction) -> Self {
        format!(
            r#"(@interface func ${namespace}_{name} (import "{namespace}" "{name}"){inputs}{outputs})"#,
            namespace = imported_function.namespace,
            name = imported_function.name,
            inputs = input_types_to_param(&imported_function.input_types),
            outputs = output_types_to_result(&imported_function.output_types),
        )
    }
}

impl<'input> From<&Adapter<'input>> for String {
    fn from(adapter: &Adapter) -> Self {
        match adapter {
            Adapter::Import {
                namespace,
                name,
                input_types,
                output_types,
                instructions,
            } => format!(
                r#"(@interface adapt (import "{namespace}" "{name}"){inputs}{outputs}{instructions})"#,
                namespace = namespace,
                name = name,
                inputs = input_types_to_param(&input_types),
                outputs = output_types_to_result(&output_types),
                instructions =
                    instructions
                        .iter()
                        .fold(String::new(), |mut accumulator, instruction| {
                            accumulator.push_str("\n  ");
                            accumulator.push_str(&String::from(instruction));
                            accumulator
                        }),
            ),

            Adapter::Export {
                name,
                input_types,
                output_types,
                instructions,
            } => format!(
                r#"(@interface adapt (export "{name}"){inputs}{outputs}{instructions})"#,
                name = name,
                inputs = input_types_to_param(&input_types),
                outputs = output_types_to_result(&output_types),
                instructions =
                    instructions
                        .iter()
                        .fold(String::new(), |mut accumulator, instruction| {
                            accumulator.push_str("\n  ");
                            accumulator.push_str(&String::from(instruction));
                            accumulator
                        }),
            ),

            _ => unimplemented!(),
        }
    }
}

impl<'input> From<&Forward<'input>> for String {
    fn from(forward: &Forward) -> Self {
        format!(
            r#"(@interface forward (export "{name}"))"#,
            name = forward.name,
        )
    }
}

impl<'input> From<&Interfaces<'input>> for String {
    fn from(interfaces: &Interfaces) -> Self {
        let mut output = String::from(";; Interfaces");

        let exports = interfaces
            .exports
            .iter()
            .fold(String::new(), |mut accumulator, export| {
                accumulator.push_str(&format!("\n\n;; Interface, Export {}\n", export.name));
                accumulator.push_str(&String::from(export));
                accumulator
            });

        let types = interfaces
            .types
            .iter()
            .fold(String::new(), |mut accumulator, ty| {
                accumulator.push_str(&format!("\n\n;; Interface, Ty {}\n", ty.name));
                accumulator.push_str(&String::from(ty));
                accumulator
            });

        let imported_functions = interfaces.imported_functions.iter().fold(
            String::new(),
            |mut accumulator, imported_function| {
                accumulator.push_str(&format!(
                    "\n\n;; Interface, Imported function {}.{}\n",
                    imported_function.namespace, imported_function.name
                ));
                accumulator.push_str(&String::from(imported_function));
                accumulator
            },
        );

        let adapters =
            interfaces
                .adapters
                .iter()
                .fold(String::new(), |mut accumulator, adapter| {
                    match adapter {
                        Adapter::Import {
                            namespace, name, ..
                        } => accumulator.push_str(&format!(
                            "\n\n;; Interface, Adapter {}.{}\n",
                            namespace, name
                        )),

                        Adapter::Export { name, .. } => {
                            accumulator.push_str(&format!("\n\n;; Interface, Adapter {}\n", name))
                        }

                        _ => unimplemented!(),
                    }
                    accumulator.push_str(&String::from(adapter));
                    accumulator
                });

        let forwards =
            interfaces
                .forwards
                .iter()
                .fold(String::new(), |mut accumulator, forward| {
                    accumulator.push_str(&format!("\n\n;; Interface, Forward {}\n", forward.name));
                    accumulator.push_str(&String::from(forward));
                    accumulator
                });

        output.push_str(&exports);
        output.push_str(&types);
        output.push_str(&imported_functions);
        output.push_str(&adapters);
        output.push_str(&forwards);

        output
    }
}

#[cfg(test)]
mod tests {
    use crate::{ast::*, interpreter::Instruction};

    #[test]
    fn test_interface_types() {
        let inputs: Vec<String> = vec![
            (&InterfaceType::Int).into(),
            (&InterfaceType::Float).into(),
            (&InterfaceType::Any).into(),
            (&InterfaceType::String).into(),
            (&InterfaceType::Seq).into(),
            (&InterfaceType::I32).into(),
            (&InterfaceType::I64).into(),
            (&InterfaceType::F32).into(),
            (&InterfaceType::F64).into(),
            (&InterfaceType::AnyRef).into(),
        ];
        let outputs = vec![
            "Int", "Float", "Any", "String", "Seq", "i32", "i64", "f32", "f64", "anyref",
        ];

        assert_eq!(inputs, outputs);
    }

    #[test]
    fn test_instructions() {
        let inputs: Vec<String> = vec![
            (&Instruction::ArgumentGet { index: 7 }).into(),
            (&Instruction::Call { function_index: 7 }).into(),
            (&Instruction::CallExport { export_name: "foo" }).into(),
            (&Instruction::ReadUtf8).into(),
            (&Instruction::WriteUtf8 {
                allocator_name: "foo",
            })
                .into(),
            (&Instruction::AsWasm(InterfaceType::Int)).into(),
            (&Instruction::AsInterface(InterfaceType::AnyRef)).into(),
            (&Instruction::TableRefAdd).into(),
            (&Instruction::TableRefGet).into(),
            (&Instruction::CallMethod(7)).into(),
            (&Instruction::MakeRecord(InterfaceType::Int)).into(),
            (&Instruction::GetField(InterfaceType::Int, 7)).into(),
            (&Instruction::Const(InterfaceType::I32, 7)).into(),
            (&Instruction::FoldSeq(7)).into(),
            (&Instruction::Add(InterfaceType::Int)).into(),
            (&Instruction::MemToSeq(InterfaceType::Int, "foo")).into(),
            (&Instruction::Load(InterfaceType::Int, "foo")).into(),
            (&Instruction::SeqNew(InterfaceType::Int)).into(),
            (&Instruction::ListPush).into(),
            (&Instruction::RepeatWhile(1, 2)).into(),
        ];
        let outputs = vec![
            "arg.get 7",
            "call 7",
            r#"call-export "foo""#,
            "read-utf8",
            r#"write-utf8 "foo""#,
            "as-wasm Int",
            "as-interface anyref",
            "table-ref-add",
            "table-ref-get",
            "call-method 7",
            "make-record Int",
            "get-field Int 7",
            "const i32 7",
            "fold-seq 7",
            "add Int",
            r#"mem-to-seq Int "foo""#,
            r#"load Int "foo""#,
            "seq.new Int",
            "list.push",
            "repeat-while 1 2",
        ];

        assert_eq!(inputs, outputs);
    }

    #[test]
    fn test_exports() {
        let inputs: Vec<String> = vec![
            (&Export {
                name: "foo",
                input_types: vec![InterfaceType::I32, InterfaceType::F32],
                output_types: vec![InterfaceType::I32],
            })
                .into(),
            (&Export {
                name: "foo",
                input_types: vec![InterfaceType::I32],
                output_types: vec![],
            })
                .into(),
            (&Export {
                name: "foo",
                input_types: vec![],
                output_types: vec![InterfaceType::I32],
            })
                .into(),
            (&Export {
                name: "foo",
                input_types: vec![],
                output_types: vec![],
            })
                .into(),
        ];
        let outputs = vec![
            r#"(@interface export "foo"
  (param i32 f32)
  (result i32))"#,
            r#"(@interface export "foo"
  (param i32))"#,
            r#"(@interface export "foo"
  (result i32))"#,
            r#"(@interface export "foo")"#,
        ];

        assert_eq!(inputs, outputs);
    }

    #[test]
    fn test_imported_functions() {
        let inputs: Vec<String> = vec![
            (&ImportedFunction {
                namespace: "ns",
                name: "foo",
                input_types: vec![InterfaceType::Int, InterfaceType::String],
                output_types: vec![InterfaceType::String],
            })
                .into(),
            (&ImportedFunction {
                namespace: "ns",
                name: "foo",
                input_types: vec![InterfaceType::String],
                output_types: vec![],
            })
                .into(),
            (&ImportedFunction {
                namespace: "ns",
                name: "foo",
                input_types: vec![],
                output_types: vec![InterfaceType::String],
            })
                .into(),
            (&ImportedFunction {
                namespace: "ns",
                name: "foo",
                input_types: vec![],
                output_types: vec![],
            })
                .into(),
        ];
        let outputs = vec![
            r#"(@interface func $ns_foo (import "ns" "foo")
  (param Int String)
  (result String))"#,
            r#"(@interface func $ns_foo (import "ns" "foo")
  (param String))"#,
            r#"(@interface func $ns_foo (import "ns" "foo")
  (result String))"#,
            r#"(@interface func $ns_foo (import "ns" "foo"))"#,
        ];

        assert_eq!(inputs, outputs);
    }

    #[test]
    fn test_adapters() {
        let inputs: Vec<String> = vec![
            (&Adapter::Import {
                namespace: "ns",
                name: "foo",
                input_types: vec![InterfaceType::I32, InterfaceType::F32],
                output_types: vec![InterfaceType::I32],
                instructions: vec![
                    Instruction::ArgumentGet { index: 0 },
                    Instruction::WriteUtf8 {
                        allocator_name: "hello",
                    },
                    Instruction::CallExport { export_name: "f" },
                ],
            })
                .into(),
            (&Adapter::Import {
                namespace: "ns",
                name: "foo",
                input_types: vec![InterfaceType::I32],
                output_types: vec![],
                instructions: vec![Instruction::CallExport { export_name: "f" }],
            })
                .into(),
            (&Adapter::Import {
                namespace: "ns",
                name: "foo",
                input_types: vec![],
                output_types: vec![InterfaceType::I32],
                instructions: vec![Instruction::CallExport { export_name: "f" }],
            })
                .into(),
            (&Adapter::Export {
                name: "foo",
                input_types: vec![InterfaceType::I32, InterfaceType::F32],
                output_types: vec![InterfaceType::I32],
                instructions: vec![
                    Instruction::ArgumentGet { index: 0 },
                    Instruction::WriteUtf8 {
                        allocator_name: "hello",
                    },
                    Instruction::CallExport { export_name: "f" },
                ],
            })
                .into(),
            (&Adapter::Export {
                name: "foo",
                input_types: vec![InterfaceType::I32],
                output_types: vec![],
                instructions: vec![Instruction::CallExport { export_name: "f" }],
            })
                .into(),
            (&Adapter::Export {
                name: "foo",
                input_types: vec![],
                output_types: vec![InterfaceType::I32],
                instructions: vec![Instruction::CallExport { export_name: "f" }],
            })
                .into(),
        ];
        let outputs = vec![
            r#"(@interface adapt (import "ns" "foo")
  (param i32 f32)
  (result i32)
  arg.get 0
  write-utf8 "hello"
  call-export "f")"#,
            r#"(@interface adapt (import "ns" "foo")
  (param i32)
  call-export "f")"#,
            r#"(@interface adapt (import "ns" "foo")
  (result i32)
  call-export "f")"#,
            r#"(@interface adapt (export "foo")
  (param i32 f32)
  (result i32)
  arg.get 0
  write-utf8 "hello"
  call-export "f")"#,
            r#"(@interface adapt (export "foo")
  (param i32)
  call-export "f")"#,
            r#"(@interface adapt (export "foo")
  (result i32)
  call-export "f")"#,
        ];

        assert_eq!(inputs, outputs);
    }

    #[test]
    fn test_forward() {
        let input: String = (&Forward { name: "main" }).into();
        let output = r#"(@interface forward (export "main"))"#;

        assert_eq!(input, output);
    }

    #[test]
    fn test_interfaces() {
        let input: String = (&Interfaces {
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
            imported_functions: vec![
                ImportedFunction {
                    namespace: "ns",
                    name: "foo",
                    input_types: vec![],
                    output_types: vec![InterfaceType::I32],
                },
                ImportedFunction {
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
            forwards: vec![Forward { name: "main" }],
        })
            .into();
        let output = r#";; Interfaces

;; Interface, Export foo
(@interface export "foo"
  (param i32))

;; Interface, Export bar
(@interface export "bar")

;; Interface, Imported function ns.foo
(@interface func $ns_foo (import "ns" "foo")
  (result i32))

;; Interface, Imported function ns.bar
(@interface func $ns_bar (import "ns" "bar"))

;; Interface, Adapter ns.foo
(@interface adapt (import "ns" "foo")
  (param i32)
  arg.get 42)

;; Interface, Adapter bar
(@interface adapt (export "bar")
  arg.get 42)

;; Interface, Forward main
(@interface forward (export "main"))"#;

        assert_eq!(input, output);
    }
}
