pub mod ast;
#[macro_use]
mod macros;
pub mod decoders;
pub mod encoders;
pub mod instructions;

pub use decoders::binary::parse as parse_binary;

#[cfg(test)]
mod tests {
    use crate::{ast::*, encoders::wat::*, instructions::Instruction, parse_binary};
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
    fn test_parse_binary_from_custom_section() {
        let module = get_module();
        let custom_section_bytes = module
            .info()
            .custom_sections
            .get("interface-types")
            .unwrap()
            .as_slice();

        match parse_binary::<()>(custom_section_bytes) {
            Ok((remainder, interfaces)) => {
                assert!(remainder.is_empty());
                assert_eq!(
                    interfaces,
                    Interfaces {
                        exports: vec![
                            Export {
                                name: "strlen",
                                input_types: vec![InterfaceType::I32],
                                output_types: vec![InterfaceType::I32]
                            },
                            Export {
                                name: "write_null_byte",
                                input_types: vec![InterfaceType::I32, InterfaceType::I32],
                                output_types: vec![InterfaceType::I32],
                            }
                        ],
                        types: vec![],
                        imported_functions: vec![
                            ImportedFunction {
                                namespace: "host",
                                name: "console_log",
                                input_types: vec![InterfaceType::String],
                                output_types: vec![],
                            },
                            ImportedFunction {
                                namespace: "host",
                                name: "document_title",
                                input_types: vec![],
                                output_types: vec![InterfaceType::String],
                            }
                        ],
                        adapters: vec![
                            Adapter::Import {
                                namespace: "host",
                                name: "console_log",
                                input_types: vec![InterfaceType::I32],
                                output_types: vec![],
                                instructions: vec![
                                    Instruction::ArgumentGet(0),
                                    Instruction::ArgumentGet(0),
                                    Instruction::CallExport("strlen"),
                                    Instruction::ReadUtf8,
                                    Instruction::Call(0),
                                ]
                            },
                            Adapter::Import {
                                namespace: "host",
                                name: "document_title",
                                input_types: vec![],
                                output_types: vec![InterfaceType::I32],
                                instructions: vec![
                                    Instruction::Call(1),
                                    Instruction::WriteUtf8("alloc"),
                                    Instruction::CallExport("write_null_byte"),
                                ]
                            }
                        ],
                        forwards: vec![Forward { name: "main" }]
                    }
                );

                let wat = String::from(&interfaces);

                assert_eq!(
                    wat,
                    r#";; Interfaces

;; Interface, Export strlen
(@interface export "strlen"
  (param i32)
  (result i32))

;; Interface, Export write_null_byte
(@interface export "write_null_byte"
  (param i32 i32)
  (result i32))

;; Interface, Imported function host.console_log
(@interface func $host_console_log (import "host" "console_log")
  (param String))

;; Interface, Imported function host.document_title
(@interface func $host_document_title (import "host" "document_title")
  (result String))

;; Interface, Adapter host.console_log
(@interface adapt (import "host" "console_log")
  (param i32)
  arg.get 0
  arg.get 0
  call-export "strlen"
  read-utf8
  call 0)

;; Interface, Adapter host.document_title
(@interface adapt (import "host" "document_title")
  (result i32)
  call 1
  write-utf8 "alloc"
  call-export "write_null_byte")

;; Interface, Forward main
(@interface forward (export "main"))"#,
                );
            }

            Err(_) => assert!(false),
        }
    }
}
