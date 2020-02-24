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
//!   arg.get 42)"#;
//!
//! assert_eq!(input, output);
//! # }
//! ```

use crate::{
    ast::{Adapter, Export, Import, InterfaceType, Interfaces, Type},
    interpreter::Instruction,
};
use std::string::ToString;

/// Encode an `InterfaceType` into a string.
impl ToString for &InterfaceType {
    fn to_string(&self) -> String {
        match self {
            InterfaceType::S8 => "s8".into(),
            InterfaceType::S16 => "s16".into(),
            InterfaceType::S32 => "s32".into(),
            InterfaceType::S64 => "s64".into(),
            InterfaceType::U8 => "u8".into(),
            InterfaceType::U16 => "u16".into(),
            InterfaceType::U32 => "u32".into(),
            InterfaceType::U64 => "u64".into(),
            InterfaceType::F32 => "f32".into(),
            InterfaceType::F64 => "f64".into(),
            InterfaceType::String => "string".into(),
            InterfaceType::Anyref => "anyref".into(),
            InterfaceType::I32 => "i32".into(),
            InterfaceType::I64 => "i64".into(),
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
impl<'input> ToString for &Type {
    fn to_string(&self) -> String {
        format!(
            r#"(@interface type (func{inputs}{outputs}))"#,
            inputs = input_types_to_param(&self.inputs),
            outputs = output_types_to_result(&self.outputs),
        )
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
        }
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
                }
                accumulator.push_str(&adapter.to_string());
                accumulator
            });

        output.push_str(&exports);
        output.push_str(&types);
        output.push_str(&imports);
        output.push_str(&adapters);

        output
    }
}

#[cfg(test)]
mod tests {
    use crate::{ast::*, interpreter::Instruction};

    #[test]
    fn test_interface_types() {
        let inputs: Vec<String> = vec![
            (&InterfaceType::S8).to_string(),
            (&InterfaceType::S16).to_string(),
            (&InterfaceType::S32).to_string(),
            (&InterfaceType::S64).to_string(),
            (&InterfaceType::U8).to_string(),
            (&InterfaceType::U16).to_string(),
            (&InterfaceType::U32).to_string(),
            (&InterfaceType::U64).to_string(),
            (&InterfaceType::F32).to_string(),
            (&InterfaceType::F64).to_string(),
            (&InterfaceType::String).to_string(),
            (&InterfaceType::Anyref).to_string(),
            (&InterfaceType::I32).to_string(),
            (&InterfaceType::I64).to_string(),
        ];
        let outputs = vec![
            "s8", "s16", "s32", "s64", "u8", "u16", "u32", "u64", "f32", "f64", "string", "anyref",
            "i32", "i64",
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
            (&Instruction::AsWasm(InterfaceType::I32)).to_string(),
            (&Instruction::AsInterface(InterfaceType::I32)).to_string(),
            (&Instruction::TableRefAdd).to_string(),
            (&Instruction::TableRefGet).to_string(),
            (&Instruction::CallMethod(7)).to_string(),
            (&Instruction::MakeRecord(InterfaceType::I32)).to_string(),
            (&Instruction::GetField(InterfaceType::I32, 7)).to_string(),
            (&Instruction::Const(InterfaceType::I32, 7)).to_string(),
            (&Instruction::FoldSeq(7)).to_string(),
            (&Instruction::Add(InterfaceType::I32)).to_string(),
            (&Instruction::MemToSeq(InterfaceType::I32, "foo")).to_string(),
            (&Instruction::Load(InterfaceType::I32, "foo")).to_string(),
            (&Instruction::SeqNew(InterfaceType::I32)).to_string(),
            (&Instruction::ListPush).to_string(),
            (&Instruction::RepeatUntil(1, 2)).to_string(),
        ];
        let outputs = vec![
            "arg.get 7",
            "call 7",
            r#"call-export "foo""#,
            "read-utf8",
            r#"write-utf8 "foo""#,
            "as-wasm i32",
            "as-interface i32",
            "table-ref-add",
            "table-ref-get",
            "call-method 7",
            "make-record i32",
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

        assert_eq!(inputs, outputs);
    }

    #[test]
    fn test_types() {
        let inputs: Vec<String> = vec![
            (&Type {
                inputs: vec![InterfaceType::I32, InterfaceType::F32],
                outputs: vec![InterfaceType::I32],
            })
                .to_string(),
            (&Type {
                inputs: vec![InterfaceType::I32],
                outputs: vec![],
            })
                .to_string(),
            (&Type {
                inputs: vec![],
                outputs: vec![InterfaceType::I32],
            })
                .to_string(),
            (&Type {
                inputs: vec![],
                outputs: vec![],
            })
                .to_string(),
        ];
        let outputs = vec![
            r#"(@interface type (func
  (param i32 f32)
  (result i32)))"#,
            r#"(@interface type (func
  (param i32)))"#,
            r#"(@interface type (func
  (result i32)))"#,
            r#"(@interface type (func))"#,
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
                input_types: vec![InterfaceType::I32, InterfaceType::String],
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
  (param i32 string)
  (result string))"#,
            r#"(@interface func $ns_foo (import "ns" "foo")
  (param string))"#,
            r#"(@interface func $ns_foo (import "ns" "foo")
  (result string))"#,
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
  arg.get 42)"#;

        assert_eq!(input, output);
    }
}
