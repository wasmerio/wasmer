//! Writes the AST into a string representing WIT with its textual format.
//!
//! # Example
//!
//! ```rust
//! use wasmer_interface_types::{
//!     ast::*,
//!     encoders::wat::*,
//!     interpreter::Instruction,
//! };
//!
//! # fn main() {
//! let input: String = (&Interfaces {
//!     exports: vec![
//!         Export {
//!             name: "foo",
//!             input_types: vec![InterfaceType::I32],
//!             output_types: vec![],
//!         },
//!         Export {
//!             name: "bar",
//!             input_types: vec![],
//!             output_types: vec![],
//!         },
//!     ],
//!     types: vec![],
//!     imports: vec![
//!         Import {
//!             namespace: "ns",
//!             name: "foo",
//!             input_types: vec![],
//!             output_types: vec![InterfaceType::I32],
//!         },
//!         Import {
//!             namespace: "ns",
//!             name: "bar",
//!             input_types: vec![],
//!             output_types: vec![],
//!         },
//!     ],
//!     adapters: vec![
//!         Adapter::Import {
//!             namespace: "ns",
//!             name: "foo",
//!             input_types: vec![InterfaceType::I32],
//!             output_types: vec![],
//!             instructions: vec![Instruction::ArgumentGet { index: 42 }],
//!         },
//!         Adapter::Export {
//!             name: "bar",
//!             input_types: vec![],
//!             output_types: vec![],
//!             instructions: vec![Instruction::ArgumentGet { index: 42 }],
//!         },
//!     ],
//!     forwards: vec![Forward { name: "main" }],
//! })
//!     .to_string();
//! let output = r#";; Interfaces
//!
//! ;; Interface, Export foo
//! (@interface export "foo"
//!   (param i32))
//!
//! ;; Interface, Export bar
//! (@interface export "bar")
//!
//! ;; Interface, Import ns.foo
//! (@interface func $ns_foo (import "ns" "foo")
//!   (result i32))
//!
//! ;; Interface, Import ns.bar
//! (@interface func $ns_bar (import "ns" "bar"))
//!
//! ;; Interface, Adapter ns.foo
//! (@interface adapt (import "ns" "foo")
//!   (param i32)
//!   arg.get 42)
//!
//! ;; Interface, Adapter bar
//! (@interface adapt (export "bar")
//!   arg.get 42)
//!
//! ;; Interface, Forward main
//! (@interface forward (export "main"))"#;
//!
//! assert_eq!(input, output);
//! # }
//! ```

use crate::{
    ast::{Adapter, Export, Forward, Import, InterfaceType, Interfaces, Type},
    interpreter::Instruction,
};
use std::string::ToString;

/// Encode an `InterfaceType` into a string.
impl ToString for &InterfaceType {
    fn to_string(&self) -> String {
        match self {
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

/// Encode an `Instruction` into a string.
impl<'input> ToString for &Instruction<'input> {
    fn to_string(&self) -> String {
        match self {
            Instruction::ArgumentGet { index } => format!("arg.get {}", index),
            Instruction::Call { function_index } => format!("call {}", function_index),
            Instruction::CallExport { export_name } => format!(r#"call-export "{}""#, export_name),
            Instruction::ReadUtf8 => "read-utf8".into(),
            Instruction::WriteUtf8 { allocator_name } => {
                format!(r#"write-utf8 "{}""#, allocator_name)
            }
            Instruction::AsWasm(interface_type) => {
                format!("as-wasm {}", interface_type.to_string())
            }
            Instruction::AsInterface(interface_type) => {
                format!("as-interface {}", interface_type.to_string())
            }
            Instruction::TableRefAdd => "table-ref-add".into(),
            Instruction::TableRefGet => "table-ref-get".into(),
            Instruction::CallMethod(index) => format!("call-method {}", index),
            Instruction::MakeRecord(interface_type) => {
                format!("make-record {}", interface_type.to_string())
            }
            Instruction::GetField(interface_type, field_index) => {
                format!("get-field {} {}", interface_type.to_string(), field_index)
            }
            Instruction::Const(interface_type, value) => {
                format!("const {} {}", interface_type.to_string(), value)
            }
            Instruction::FoldSeq(import_index) => format!("fold-seq {}", import_index),
            Instruction::Add(interface_type) => format!("add {}", interface_type.to_string()),
            Instruction::MemToSeq(interface_type, memory) => {
                format!(r#"mem-to-seq {} "{}""#, interface_type.to_string(), memory)
            }
            Instruction::Load(interface_type, memory) => {
                format!(r#"load {} "{}""#, interface_type.to_string(), memory)
            }
            Instruction::SeqNew(interface_type) => {
                format!("seq.new {}", interface_type.to_string())
            }
            Instruction::ListPush => "list.push".into(),
            Instruction::RepeatUntil(condition_index, step_index) => {
                format!("repeat-until {} {}", condition_index, step_index)
            }
        }
    }
}

/// Encode a list of `InterfaceType` representing inputs into a
/// string.
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
                    accumulator.push_str(&interface_type.to_string());
                    accumulator
                })
        )
    }
}

/// Encode a list of `InterfaceType` representing outputs into a
/// string.
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
                    accumulator.push_str(&interface_type.to_string());
                    accumulator
                })
        )
    }
}

/// Encode an `Export` into a string.
impl<'input> ToString for &Export<'input> {
    fn to_string(&self) -> String {
        format!(
            r#"(@interface export "{name}"{inputs}{outputs})"#,
            name = self.name,
            inputs = input_types_to_param(&self.input_types),
            outputs = output_types_to_result(&self.output_types),
        )
    }
}

/// Encode a `Type` into a string.
impl<'input> ToString for &Type<'input> {
    fn to_string(&self) -> String {
        unimplemented!()
    }
}

/// Encode an `Import` into a string.
impl<'input> ToString for &Import<'input> {
    fn to_string(&self) -> String {
        format!(
            r#"(@interface func ${namespace}_{name} (import "{namespace}" "{name}"){inputs}{outputs})"#,
            namespace = self.namespace,
            name = self.name,
            inputs = input_types_to_param(&self.input_types),
            outputs = output_types_to_result(&self.output_types),
        )
    }
}

/// Encode an `Adapter` into a string.
impl<'input> ToString for &Adapter<'input> {
    fn to_string(&self) -> String {
        match self {
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
                            accumulator.push_str(&instruction.to_string());
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
                            accumulator.push_str(&instruction.to_string());
                            accumulator
                        }),
            ),

            _ => unimplemented!(),
        }
    }
}

/// Encode a `Forward` into a string.
impl<'input> ToString for &Forward<'input> {
    fn to_string(&self) -> String {
        format!(
            r#"(@interface forward (export "{name}"))"#,
            name = self.name,
        )
    }
}

/// Encode an `Interfaces` into a string.
impl<'input> ToString for &Interfaces<'input> {
    fn to_string(&self) -> String {
        let mut output = String::from(";; Interfaces");

        let exports = self
            .exports
            .iter()
            .fold(String::new(), |mut accumulator, export| {
                accumulator.push_str(&format!("\n\n;; Interface, Export {}\n", export.name));
                accumulator.push_str(&export.to_string());
                accumulator
            });

        let types = self
            .types
            .iter()
            .fold(String::new(), |mut accumulator, ty| {
                accumulator.push_str(&format!("\n\n;; Interface, Ty {}\n", ty.name));
                accumulator.push_str(&ty.to_string());
                accumulator
            });

        let imports = self
            .imports
            .iter()
            .fold(String::new(), |mut accumulator, import| {
                accumulator.push_str(&format!(
                    "\n\n;; Interface, Import {}.{}\n",
                    import.namespace, import.name
                ));
                accumulator.push_str(&import.to_string());
                accumulator
            });

        let adapters = self
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
                accumulator.push_str(&adapter.to_string());
                accumulator
            });

        let forwards = self
            .forwards
            .iter()
            .fold(String::new(), |mut accumulator, forward| {
                accumulator.push_str(&format!("\n\n;; Interface, Forward {}\n", forward.name));
                accumulator.push_str(&forward.to_string());
                accumulator
            });

        output.push_str(&exports);
        output.push_str(&types);
        output.push_str(&imports);
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
            (&InterfaceType::Int).to_string(),
            (&InterfaceType::Float).to_string(),
            (&InterfaceType::Any).to_string(),
            (&InterfaceType::String).to_string(),
            (&InterfaceType::Seq).to_string(),
            (&InterfaceType::I32).to_string(),
            (&InterfaceType::I64).to_string(),
            (&InterfaceType::F32).to_string(),
            (&InterfaceType::F64).to_string(),
            (&InterfaceType::AnyRef).to_string(),
        ];
        let outputs = vec![
            "Int", "Float", "Any", "String", "Seq", "i32", "i64", "f32", "f64", "anyref",
        ];

        assert_eq!(inputs, outputs);
    }

    #[test]
    fn test_instructions() {
        let inputs: Vec<String> = vec![
            (&Instruction::ArgumentGet { index: 7 }).to_string(),
            (&Instruction::Call { function_index: 7 }).to_string(),
            (&Instruction::CallExport { export_name: "foo" }).to_string(),
            (&Instruction::ReadUtf8).to_string(),
            (&Instruction::WriteUtf8 {
                allocator_name: "foo",
            })
                .to_string(),
            (&Instruction::AsWasm(InterfaceType::Int)).to_string(),
            (&Instruction::AsInterface(InterfaceType::AnyRef)).to_string(),
            (&Instruction::TableRefAdd).to_string(),
            (&Instruction::TableRefGet).to_string(),
            (&Instruction::CallMethod(7)).to_string(),
            (&Instruction::MakeRecord(InterfaceType::Int)).to_string(),
            (&Instruction::GetField(InterfaceType::Int, 7)).to_string(),
            (&Instruction::Const(InterfaceType::I32, 7)).to_string(),
            (&Instruction::FoldSeq(7)).to_string(),
            (&Instruction::Add(InterfaceType::Int)).to_string(),
            (&Instruction::MemToSeq(InterfaceType::Int, "foo")).to_string(),
            (&Instruction::Load(InterfaceType::Int, "foo")).to_string(),
            (&Instruction::SeqNew(InterfaceType::Int)).to_string(),
            (&Instruction::ListPush).to_string(),
            (&Instruction::RepeatUntil(1, 2)).to_string(),
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
            "repeat-until 1 2",
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
                .to_string(),
            (&Export {
                name: "foo",
                input_types: vec![InterfaceType::I32],
                output_types: vec![],
            })
                .to_string(),
            (&Export {
                name: "foo",
                input_types: vec![],
                output_types: vec![InterfaceType::I32],
            })
                .to_string(),
            (&Export {
                name: "foo",
                input_types: vec![],
                output_types: vec![],
            })
                .to_string(),
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
    fn test_imports() {
        let inputs: Vec<String> = vec![
            (&Import {
                namespace: "ns",
                name: "foo",
                input_types: vec![InterfaceType::Int, InterfaceType::String],
                output_types: vec![InterfaceType::String],
            })
                .to_string(),
            (&Import {
                namespace: "ns",
                name: "foo",
                input_types: vec![InterfaceType::String],
                output_types: vec![],
            })
                .to_string(),
            (&Import {
                namespace: "ns",
                name: "foo",
                input_types: vec![],
                output_types: vec![InterfaceType::String],
            })
                .to_string(),
            (&Import {
                namespace: "ns",
                name: "foo",
                input_types: vec![],
                output_types: vec![],
            })
                .to_string(),
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
                .to_string(),
            (&Adapter::Import {
                namespace: "ns",
                name: "foo",
                input_types: vec![InterfaceType::I32],
                output_types: vec![],
                instructions: vec![Instruction::CallExport { export_name: "f" }],
            })
                .to_string(),
            (&Adapter::Import {
                namespace: "ns",
                name: "foo",
                input_types: vec![],
                output_types: vec![InterfaceType::I32],
                instructions: vec![Instruction::CallExport { export_name: "f" }],
            })
                .to_string(),
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
                .to_string(),
            (&Adapter::Export {
                name: "foo",
                input_types: vec![InterfaceType::I32],
                output_types: vec![],
                instructions: vec![Instruction::CallExport { export_name: "f" }],
            })
                .to_string(),
            (&Adapter::Export {
                name: "foo",
                input_types: vec![],
                output_types: vec![InterfaceType::I32],
                instructions: vec![Instruction::CallExport { export_name: "f" }],
            })
                .to_string(),
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
        let input: String = (&Forward { name: "main" }).to_string();
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
            forwards: vec![Forward { name: "main" }],
        })
            .to_string();
        let output = r#";; Interfaces

;; Interface, Export foo
(@interface export "foo"
  (param i32))

;; Interface, Export bar
(@interface export "bar")

;; Interface, Import ns.foo
(@interface func $ns_foo (import "ns" "foo")
  (result i32))

;; Interface, Import ns.bar
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
