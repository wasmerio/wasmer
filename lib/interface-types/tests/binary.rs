use wasmer_interface_types::{
    ast::*, decoders::binary::parse, encoders::binary::ToBytes, interpreter::Instruction,
};

/// Tests an AST to binary, then binary to AST roundtrip.
#[test]
fn test_binary_encoding_decoding_roundtrip() {
    let original_ast = Interfaces {
        types: vec![
            Type {
                inputs: vec![],
                outputs: vec![],
            },
            Type {
                inputs: vec![InterfaceType::I32, InterfaceType::I32],
                outputs: vec![InterfaceType::S32],
            },
        ],
        imports: vec![Import {
            namespace: "a",
            name: "b",
            signature_type: 0,
        }],
        adapters: vec![Adapter {
            function_type: 0,
            instructions: vec![Instruction::ArgumentGet { index: 1 }],
        }],
        exports: vec![Export {
            name: "ab",
            function_type: 1,
        }],
        implementations: vec![Implementation {
            core_function_type: 0,
            adapter_function_type: 0,
        }],
    };

    let mut binary = vec![];

    original_ast
        .to_bytes(&mut binary)
        .expect("Failed to encode the AST.");

    let (remainder, ast) = parse::<()>(binary.as_slice()).expect("Failed to decode the AST.");

    assert!(remainder.is_empty());

    assert_eq!(original_ast, ast);
}
