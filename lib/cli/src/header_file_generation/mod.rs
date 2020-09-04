//! A convenient little abstraction for building up C expressions and generating
//! simple C code.

/// A Type in the C language.
#[derive(Debug, Clone)]
pub enum CType {
    /// C `void` type.
    Void,
    /// A pointer to some other type.
    PointerTo {
        /// Whether the pointer is `const`.
        is_const: bool,
        /// The type that the pointer points to.
        inner: Box<CType>,
    },
    /// C 8 bit unsigned integer type.
    U8,
    /// C 16 bit unsigned integer type.
    U16,
    /// C 32 bit unsigned integer type.
    U32,
    /// C 64 bit unsigned integer type.
    U64,
    /// C pointer sized unsigned integer type.
    USize,
    /// C 8 bit signed integer type.
    I8,
    /// C 16 bit signed integer type.
    I16,
    /// C 32 bit signed integer type.
    I32,
    /// C 64 bit signed integer type.
    I64,
    /// C pointer sized signed integer type.
    ISize,
    /// A function or function pointer.
    Function {
        /// The arguments the function takes.
        arguments: Vec<CType>,
        /// The return value if it has one
        ///
        /// None is equivalent to Some(Box(Ctype::Void)).
        return_value: Option<Box<CType>>,
    },
}

impl CType {
    /// Convenience function to get a mutable void pointer type.
    pub fn void_ptr() -> Self {
        CType::PointerTo {
            is_const: false,
            inner: Box::new(CType::Void),
        }
    }

    /// Convenience function to get a const void pointer type.
    #[allow(dead_code)]
    pub fn const_void_ptr() -> Self {
        CType::PointerTo {
            is_const: false,
            inner: Box::new(CType::Void),
        }
    }

    /// Generate the C source code for a type into the given `String`.
    fn generate_c(&self, w: &mut String) {
        match &self {
            Self::Void => {
                w.push_str("void");
            }
            Self::PointerTo { is_const, inner } => {
                if *is_const {
                    w.push_str("const ");
                }
                inner.generate_c(w);
                w.push_str("*");
            }
            Self::U8 => {
                w.push_str("unsigned char");
            }
            Self::U16 => {
                w.push_str("unsigned short");
            }
            Self::U32 => {
                w.push_str("unsigned int");
            }
            Self::U64 => {
                w.push_str("unsigned long long");
            }
            Self::USize => {
                w.push_str("unsigned size_t");
            }
            Self::I8 => {
                w.push_str("char");
            }
            Self::I16 => {
                w.push_str("short");
            }
            Self::I32 => {
                w.push_str("int");
            }
            Self::I64 => {
                w.push_str("long long");
            }
            Self::ISize => {
                w.push_str("size_t");
            }
            Self::Function {
                arguments,
                return_value,
            } => {
                // function with no, name, assume it's a function pointer
                let ret: CType = return_value
                    .as_ref()
                    .map(|i: &Box<CType>| (&**i).clone())
                    .unwrap_or_default();
                ret.generate_c(w);
                w.push(' ');
                w.push_str("(*)");
                w.push('(');
                if arguments.len() > 1 {
                    for arg in &arguments[..arguments.len() - 1] {
                        arg.generate_c(w);
                        w.push_str(", ");
                    }
                    arguments.last().unwrap().generate_c(w);
                } else if arguments.len() == 1 {
                    arguments[0].generate_c(w);
                }
                w.push(')');
            }
        }
    }

    /// Generate the C source code for a type with a nameinto the given `String`.
    fn generate_c_with_name(&self, name: &String, w: &mut String) {
        match &self {
            Self::PointerTo { .. }
            | Self::Void
            | Self::U8
            | Self::U16
            | Self::U32
            | Self::U64
            | Self::USize
            | Self::I8
            | Self::I16
            | Self::I32
            | Self::I64
            | Self::ISize => {
                self.generate_c(w);
                w.push(' ');
                w.push_str(name);
            }
            Self::Function {
                arguments,
                return_value,
            } => {
                let ret: CType = return_value
                    .as_ref()
                    .map(|i: &Box<CType>| (&**i).clone())
                    .unwrap_or_default();
                ret.generate_c(w);
                w.push(' ');
                w.push_str(&name);
                w.push('(');
                if arguments.len() > 1 {
                    for arg in &arguments[..arguments.len() - 1] {
                        arg.generate_c(w);
                        w.push_str(", ");
                    }
                    arguments.last().unwrap().generate_c(w);
                } else if arguments.len() == 1 {
                    arguments[0].generate_c(w);
                }
                w.push(')');
            }
        }
    }
}

impl Default for CType {
    fn default() -> CType {
        CType::Void
    }
}

/// A statement in the C programming language. This may not be exact to what an
/// AST would look like or what the C standard says about the C language, it's
/// simply a structed way to organize data for generating C code.
#[derive(Debug, Clone)]
pub enum CStatement {
    /// A declaration of some kind.
    Declaration {
        /// The name of the thing being declared.
        name: String,
        /// Whether the thing being declared is an array.
        // TODO: probably make this part of CType
        array: bool,
        /// Whether the thing being declared is `extern`.
        is_extern: bool,
        /// Whether the thing being declared is `const`.
        is_const: bool,
        /// The type of the thing being declared.
        ctype: CType,
        /// The definition of the thing being declared.
        ///
        /// This is useful for initializing constant arrays, for example.
        definition: Option<Box<CStatement>>,
    },
    /// A literal array of CStatements.
    LiteralArray {
        /// The contents of the array.
        items: Vec<CStatement>,
    },

    /// A literal constant value, passed through directly as a string.
    LiteralConstant {
        /// The raw value acting as a constant.
        value: String,
    },
}

impl CStatement {
    /// Generate C source code for the given CStatement.
    fn generate_c(&self, w: &mut String) {
        match &self {
            Self::Declaration {
                name,
                array,
                is_extern,
                is_const,
                ctype,
                definition,
            } => {
                if *is_const {
                    w.push_str("const ");
                }
                if *is_extern {
                    w.push_str("extern ");
                }
                ctype.generate_c_with_name(name, w);
                // TODO: array should be part of the type
                if *array {
                    w.push_str("[]");
                }
                if let Some(def) = definition {
                    w.push_str(" = ");
                    def.generate_c(w);
                }
                w.push(';');
                w.push('\n');
            }
            Self::LiteralArray { items } => {
                w.push('{');
                if !items.is_empty() {
                    w.push('\n');
                }
                for item in items {
                    w.push('\t');
                    item.generate_c(w);
                    w.push(',');
                    w.push('\n');
                }
                w.push('}');
            }
            Self::LiteralConstant { value } => {
                w.push_str(&value);
            }
        }
    }
}

/// Generate C source code from some `CStatements` into a String.
// TODO: add config section
pub fn generate_c(statements: &[CStatement]) -> String {
    let mut out = String::new();
    for statement in statements {
        statement.generate_c(&mut out);
    }
    out
}

// TODO: split the bottom part into its own file

use wasmer_compiler::{Symbol, SymbolRegistry};
use wasmer_vm::ModuleInfo;

/// Generate the header file that goes with the generated object file.
pub fn generate_header_file(
    module_info: &ModuleInfo,
    symbol_registry: &dyn SymbolRegistry,
) -> String {
    let mut c_statements = vec![];
    c_statements.push(CStatement::Declaration {
        name: "module_bytes_len".to_string(),
        array: false,
        is_extern: false,
        is_const: true,
        ctype: CType::U32,
        definition: Some(Box::new(CStatement::LiteralConstant {
            value: "0".to_string(), //todo!("get the metadata length from somewhere"), //format!("{}", self.metadata_length),
        })),
    });
    c_statements.push(CStatement::Declaration {
        name: "WASMER_METADATA".to_string(),
        array: true,
        is_extern: true,
        is_const: true,
        ctype: CType::U8,
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
            array: false,
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

            function_pointer_array_statements.push(CStatement::LiteralConstant {
                value: function_name.clone(),
            });
        }

        c_statements.push(CStatement::Declaration {
            name: "function_pointers".to_string(),
            array: true,
            is_extern: false,
            is_const: true,
            ctype: CType::void_ptr(),
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
            array: false,
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
            array: true,
            is_extern: false,
            is_const: true,
            ctype: CType::void_ptr(),
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
            array: false,
            is_extern: false,
            is_const: false,
            ctype: CType::Function {
                arguments: vec![CType::void_ptr(), CType::void_ptr(), CType::void_ptr()],
                return_value: None,
            },
            definition: None,
        });
    }

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
            array: true,
            is_extern: false,
            is_const: true,
            ctype: CType::void_ptr(),
            definition: Some(Box::new(CStatement::LiteralArray {
                items: dynamic_function_trampoline_statements,
            })),
        });
    }

    generate_c(&c_statements)
}
