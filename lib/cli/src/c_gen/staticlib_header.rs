//! Generate a header file for the static object file produced.

use wasmer_compiler::types::symbols::{Symbol, SymbolRegistry};
use wasmer_types::ModuleInfo;

use super::{generate_c, CStatement, CType};

/// Helper functions to simplify the usage of the static artifact.
fn gen_helper_functions(atom_name: &str, module_name: &str) -> String {
    format!("
    wasm_module_t* wasmer_object_module_new_{atom_name}(wasm_store_t* store, const char* wasm_name) {{
            // wasm_name intentionally unused for now: will be used in the future.
            wasm_byte_vec_t module_byte_vec = {{
                .size = module_bytes_len_{atom_name},
                .data = (char*)&{module_name}[0],
            }};
            wasm_module_t* module = wasm_module_deserialize(store, &module_byte_vec);

            return module;
    }}
    ")
}

/// Generate the header file that goes with the generated object file.
pub fn generate_header_file(
    atom_name: &str,
    module_info: &ModuleInfo,
    symbol_registry: &dyn SymbolRegistry,
    metadata_length: usize,
) -> String {
    let mut c_statements = vec![
        CStatement::LiteralConstant {
            value: "#include \"wasmer.h\"\n#include <stdlib.h>\n#include <string.h>\n\n"
                .to_string(),
        },
        CStatement::LiteralConstant {
            value: "#ifdef __cplusplus\nextern \"C\" {\n#endif\n\n".to_string(),
        },
        CStatement::Declaration {
            name: format!("module_bytes_len_{atom_name}"),
            is_extern: false,
            is_const: true,
            ctype: CType::U32,
            definition: Some(Box::new(CStatement::LiteralConstant {
                value: metadata_length.to_string(),
            })),
        },
        CStatement::Declaration {
            name: symbol_registry.symbol_to_name(Symbol::Metadata),
            is_extern: true,
            is_const: true,
            ctype: CType::Array {
                inner: Box::new(CType::U8),
            },
            definition: None,
        },
    ];
    let function_declarations = module_info
        .functions
        .iter()
        .filter_map(|(f_index, sig_index)| {
            Some((module_info.local_func_index(f_index)?, sig_index))
        })
        .map(|(function_local_index, _sig_index)| {
            let function_name =
                symbol_registry.symbol_to_name(Symbol::LocalFunction(function_local_index));
            // TODO: figure out the signature here too
            CStatement::Declaration {
                name: function_name,
                is_extern: true,
                is_const: false,
                ctype: CType::Function {
                    arguments: vec![CType::Void],
                    return_value: None,
                },
                definition: None,
            }
        });
    c_statements.push(CStatement::LiteralConstant {
        value: r#"
// Compiled Wasm function pointers ordered by function index: the order they
// appeared in in the Wasm module.
"#
        .to_string(),
    });
    c_statements.extend(function_declarations);

    let func_trampoline_declarations =
        module_info
            .signatures
            .iter()
            .map(|(sig_index, _func_type)| {
                let function_name =
                    symbol_registry.symbol_to_name(Symbol::FunctionCallTrampoline(sig_index));

                CStatement::Declaration {
                    name: function_name,
                    is_extern: true,
                    is_const: false,
                    ctype: CType::Function {
                        arguments: vec![CType::void_ptr(), CType::void_ptr(), CType::void_ptr()],
                        return_value: None,
                    },
                    definition: None,
                }
            });
    c_statements.push(CStatement::LiteralConstant {
        value: r#"
// Trampolines (functions by which we can call into Wasm) ordered by signature.
// There is 1 trampoline per function signature in the order they appear in
// the Wasm module.
"#
        .to_string(),
    });
    c_statements.extend(func_trampoline_declarations);

    let dyn_func_declarations = module_info
        .functions
        .keys()
        .take(module_info.num_imported_functions)
        .map(|func_index| {
            let function_name =
                symbol_registry.symbol_to_name(Symbol::DynamicFunctionTrampoline(func_index));
            // TODO: figure out the signature here
            CStatement::Declaration {
                name: function_name,
                is_extern: true,
                is_const: false,
                ctype: CType::Function {
                    arguments: vec![CType::void_ptr(), CType::void_ptr(), CType::void_ptr()],
                    return_value: None,
                },
                definition: None,
            }
        });
    c_statements.push(CStatement::LiteralConstant {
        value: r#"
// Dynamic trampolines are per-function and are used for each function where
// the type signature is not known statically. In this case, this corresponds to
// the imported functions.
"#
        .to_string(),
    });
    c_statements.extend(dyn_func_declarations);

    c_statements.push(CStatement::TypeDef {
        source_type: CType::Function {
            arguments: vec![CType::void_ptr(), CType::void_ptr(), CType::void_ptr()],
            return_value: None,
        },
        new_name: "dyn_func_trampoline_t".to_string(),
    });

    c_statements.push(CStatement::LiteralConstant {
        value: gen_helper_functions(atom_name, &symbol_registry.symbol_to_name(Symbol::Metadata)),
    });

    c_statements.push(CStatement::LiteralConstant {
        value: "\n#ifdef __cplusplus\n}\n#endif\n\n".to_string(),
    });

    generate_c(&c_statements)
}
