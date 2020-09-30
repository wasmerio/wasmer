//! Generate a header file for the object file produced by the ObjectFile engine.

use super::{generate_c, CStatement, CType};
use wasmer_compiler::{Symbol, SymbolRegistry};
use wasmer_vm::ModuleInfo;

/// Generate the header file that goes with the generated object file.
pub fn generate_header_file(
    module_info: &ModuleInfo,
    symbol_registry: &dyn SymbolRegistry,
    metadata_length: usize,
) -> String {
    let mut c_statements = vec![];
    c_statements.push(CStatement::Declaration {
        name: "module_bytes_len".to_string(),
        is_extern: false,
        is_const: true,
        ctype: CType::U32,
        definition: Some(Box::new(CStatement::LiteralConstant {
            value: format!("{}", metadata_length),
        })),
    });
    c_statements.push(CStatement::Declaration {
        name: "WASMER_METADATA".to_string(),
        is_extern: true,
        is_const: true,
        ctype: CType::Array {
            inner: Box::new(CType::U8),
        },
        definition: None,
    });
    for (function_local_index, _sig_index) in
        module_info
            .functions
            .iter()
            .filter_map(|(f_index, sig_index)| {
                Some((module_info.local_func_index(f_index)?, sig_index))
            })
    {
        let function_name =
            symbol_registry.symbol_to_name(Symbol::LocalFunction(function_local_index));
        // TODO: figure out the signature here too
        c_statements.push(CStatement::Declaration {
            name: function_name.clone(),
            is_extern: false,
            is_const: false,
            ctype: CType::Function {
                arguments: vec![CType::Void],
                return_value: None,
            },
            definition: None,
        });
    }

    // function pointer array
    {
        let mut function_pointer_array_statements = vec![];
        for (function_local_index, _sig_index) in
            module_info
                .functions
                .iter()
                .filter_map(|(f_index, sig_index)| {
                    Some((module_info.local_func_index(f_index)?, sig_index))
                })
        {
            let function_name =
                symbol_registry.symbol_to_name(Symbol::LocalFunction(function_local_index));
            // TODO: figure out the signature here too

            function_pointer_array_statements.push(CStatement::Cast {
                target_type: CType::void_ptr(),
                expression: Box::new(CStatement::LiteralConstant {
                    value: function_name.clone(),
                }),
            });
        }

        c_statements.push(CStatement::Declaration {
            name: "function_pointers".to_string(),
            is_extern: false,
            is_const: true,
            ctype: CType::Array {
                inner: Box::new(CType::void_ptr()),
            },
            definition: Some(Box::new(CStatement::LiteralArray {
                items: function_pointer_array_statements,
            })),
        });
    }

    for (sig_index, _func_type) in module_info.signatures.iter() {
        let function_name =
            symbol_registry.symbol_to_name(Symbol::FunctionCallTrampoline(sig_index));

        c_statements.push(CStatement::Declaration {
            name: function_name.clone(),
            is_extern: false,
            is_const: false,
            ctype: CType::Function {
                arguments: vec![CType::void_ptr(), CType::void_ptr(), CType::void_ptr()],
                return_value: None,
            },
            definition: None,
        });
    }

    // function trampolines
    {
        let mut function_trampoline_statements = vec![];
        for (sig_index, _vm_shared_index) in module_info.signatures.iter() {
            let function_name =
                symbol_registry.symbol_to_name(Symbol::FunctionCallTrampoline(sig_index));
            function_trampoline_statements.push(CStatement::LiteralConstant {
                value: function_name,
            });
        }

        c_statements.push(CStatement::Declaration {
            name: "function_trampolines".to_string(),
            is_extern: false,
            is_const: true,
            ctype: CType::Array {
                inner: Box::new(CType::void_ptr()),
            },
            definition: Some(Box::new(CStatement::LiteralArray {
                items: function_trampoline_statements,
            })),
        });
    }

    for func_index in module_info
        .functions
        .keys()
        .take(module_info.num_imported_functions)
    {
        let function_name =
            symbol_registry.symbol_to_name(Symbol::DynamicFunctionTrampoline(func_index));
        // TODO: figure out the signature here
        c_statements.push(CStatement::Declaration {
            name: function_name,
            is_extern: false,
            is_const: false,
            ctype: CType::Function {
                arguments: vec![CType::void_ptr(), CType::void_ptr(), CType::void_ptr()],
                return_value: None,
            },
            definition: None,
        });
    }

    c_statements.push(CStatement::TypeDef {
        source_type: CType::Function {
            arguments: vec![CType::void_ptr(), CType::void_ptr(), CType::void_ptr()],
            return_value: None,
        },
        new_name: "dyn_func_trampoline_t".to_string(),
    });

    // dynamic function trampoline pointer array
    {
        let mut dynamic_function_trampoline_statements = vec![];
        for func_index in module_info
            .functions
            .keys()
            .take(module_info.num_imported_functions)
        {
            let function_name =
                symbol_registry.symbol_to_name(Symbol::DynamicFunctionTrampoline(func_index));
            dynamic_function_trampoline_statements.push(CStatement::LiteralConstant {
                value: function_name,
            });
        }
        c_statements.push(CStatement::Declaration {
            name: "dynamic_function_trampoline_pointers".to_string(),
            is_extern: false,
            is_const: true,
            ctype: CType::Array {
                inner: Box::new(CType::TypeDef("dyn_func_trampoline_t".to_string())),
            },
            definition: Some(Box::new(CStatement::LiteralArray {
                items: dynamic_function_trampoline_statements,
            })),
        });
    }

    let inner_c = generate_c(&c_statements);

    // we wrap the inner C to work with C++ too
    format!(
        "#ifdef __cplusplus\nextern \"C\" {{\n#endif\n\n{}\n\n#ifdef __cplusplus\n}}\n#endif\n",
        inner_c
    )
}
