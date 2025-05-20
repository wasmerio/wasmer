//! A convenient little abstraction for building up C expressions and generating
//! simple C code.

#![allow(dead_code)]

pub mod staticlib_header;

/// An identifier in C.
pub type CIdent = String;

/// A Type in the C language.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub enum CType {
    /// C `void` type.
    #[default]
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
    /// C constant array.
    Array {
        /// The type of the array.
        inner: Box<CType>,
    },
    /// A user defined type.
    TypeDef(String),
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
            is_const: true,
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
                w.push('*');
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
                #[allow(clippy::borrowed_box)]
                let ret: CType = return_value
                    .as_ref()
                    .map(|i: &Box<CType>| (**i).clone())
                    .unwrap_or_default();
                ret.generate_c(w);
                w.push(' ');
                w.push_str("(*)");
                w.push('(');
                match arguments.len() {
                    l if l > 1 => {
                        for arg in &arguments[..arguments.len() - 1] {
                            arg.generate_c(w);
                            w.push_str(", ");
                        }
                        arguments.last().unwrap().generate_c(w);
                    }
                    1 => {
                        arguments[0].generate_c(w);
                    }
                    _ => {}
                }
                w.push(')');
            }
            Self::Array { inner } => {
                inner.generate_c(w);
                w.push_str("[]");
            }
            Self::TypeDef(inner) => {
                w.push_str(inner);
            }
        }
    }

    /// Generate the C source code for a type with a nameinto the given `String`.
    fn generate_c_with_name(&self, name: &str, w: &mut String) {
        match &self {
            Self::PointerTo { .. }
            | Self::TypeDef { .. }
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
                #[allow(clippy::borrowed_box)]
                let ret: CType = return_value
                    .as_ref()
                    .map(|i: &Box<CType>| (**i).clone())
                    .unwrap_or_default();
                ret.generate_c(w);
                w.push(' ');
                w.push_str(name);
                w.push('(');
                match arguments.len() {
                    l if l > 1 => {
                        for arg in &arguments[..arguments.len() - 1] {
                            arg.generate_c(w);
                            w.push_str(", ");
                        }
                        arguments.last().unwrap().generate_c(w);
                    }
                    1 => {
                        arguments[0].generate_c(w);
                    }
                    _ => {}
                }
                w.push(')');
            }
            Self::Array { inner } => {
                inner.generate_c(w);
                w.push(' ');
                w.push_str(name);
                w.push_str("[]");
            }
        }
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
        name: CIdent,
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

    /// A C-style cast
    Cast {
        /// The type to cast to.
        target_type: CType,
        /// The thing being cast.
        expression: Box<CStatement>,
    },

    /// Typedef one type to another.
    TypeDef {
        /// The type of the thing being typedef'd.
        source_type: CType,
        /// The new name by which this type may be called.
        new_name: CIdent,
    },
}

impl CStatement {
    /// Generate C source code for the given CStatement.
    fn generate_c(&self, w: &mut String) {
        match &self {
            Self::Declaration {
                name,
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
                w.push_str(value);
            }
            Self::Cast {
                target_type,
                expression,
            } => {
                w.push('(');
                target_type.generate_c(w);
                w.push(')');
                w.push(' ');
                expression.generate_c(w);
            }
            Self::TypeDef {
                source_type,
                new_name,
            } => {
                w.push_str("typedef ");
                // leaky abstraction / hack, doesn't fully solve the problem
                if let CType::Function { .. } = source_type {
                    source_type.generate_c_with_name(&format!("(*{new_name})"), w);
                } else {
                    source_type.generate_c(w);
                    w.push(' ');
                    w.push_str(new_name);
                }
                w.push(';');
                w.push('\n');
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn generate_types() {
        macro_rules! assert_c_type {
            ($ctype:expr, $expected:expr) => {
                let mut w = String::new();
                let ctype = $ctype;
                ctype.generate_c(&mut w);
                assert_eq!(w, $expected);
            };
        }

        assert_c_type!(CType::Void, "void");
        assert_c_type!(CType::void_ptr(), "void*");
        assert_c_type!(CType::const_void_ptr(), "const void*");
        assert_c_type!(CType::U8, "unsigned char");
        assert_c_type!(CType::U16, "unsigned short");
        assert_c_type!(CType::U32, "unsigned int");
        assert_c_type!(CType::U64, "unsigned long long");
        assert_c_type!(CType::USize, "unsigned size_t");
        assert_c_type!(CType::I8, "char");
        assert_c_type!(CType::I16, "short");
        assert_c_type!(CType::I32, "int");
        assert_c_type!(CType::I64, "long long");
        assert_c_type!(CType::ISize, "size_t");
        assert_c_type!(CType::TypeDef("my_type".to_string()), "my_type");
        assert_c_type!(
            CType::Function {
                arguments: vec![CType::U8, CType::ISize],
                return_value: None
            },
            "void (*)(unsigned char, size_t)"
        );
        assert_c_type!(
            CType::Function {
                arguments: vec![],
                return_value: Some(Box::new(CType::ISize))
            },
            "size_t (*)()"
        );
        assert_c_type!(
            CType::PointerTo {
                is_const: true,
                inner: Box::new(CType::PointerTo {
                    is_const: false,
                    inner: Box::new(CType::U32)
                })
            },
            "const unsigned int**"
        );
        // TODO: test more complicated const correctness rules: there are bugs relating to it.
    }

    #[test]
    fn generate_types_with_names() {
        macro_rules! assert_c_type {
            ($ctype:expr, $name:literal, $expected:expr) => {
                let mut w = String::new();
                let ctype = $ctype;
                ctype.generate_c_with_name($name, &mut w);
                assert_eq!(w, $expected);
            };
        }

        assert_c_type!(CType::Void, "main", "void main");
        assert_c_type!(CType::void_ptr(), "data", "void* data");
        assert_c_type!(CType::const_void_ptr(), "data", "const void* data");
        assert_c_type!(CType::U8, "data", "unsigned char data");
        assert_c_type!(CType::U16, "data", "unsigned short data");
        assert_c_type!(CType::U32, "data", "unsigned int data");
        assert_c_type!(CType::U64, "data", "unsigned long long data");
        assert_c_type!(CType::USize, "data", "unsigned size_t data");
        assert_c_type!(CType::I8, "data", "char data");
        assert_c_type!(CType::I16, "data", "short data");
        assert_c_type!(CType::I32, "data", "int data");
        assert_c_type!(CType::I64, "data", "long long data");
        assert_c_type!(CType::ISize, "data", "size_t data");
        assert_c_type!(
            CType::TypeDef("my_type".to_string()),
            "data",
            "my_type data"
        );
        assert_c_type!(
            CType::Function {
                arguments: vec![CType::U8, CType::ISize],
                return_value: None
            },
            "my_func",
            "void my_func(unsigned char, size_t)"
        );
        assert_c_type!(
            CType::Function {
                arguments: vec![],
                return_value: Some(Box::new(CType::ISize))
            },
            "my_func",
            "size_t my_func()"
        );
        assert_c_type!(
            CType::PointerTo {
                is_const: true,
                inner: Box::new(CType::PointerTo {
                    is_const: false,
                    inner: Box::new(CType::U32)
                })
            },
            "data",
            "const unsigned int** data"
        );
        // TODO: test more complicated const correctness rules: there are bugs relating to it.
    }

    #[test]
    fn generate_expressions_works() {
        macro_rules! assert_c_expr {
            ($cexpr:expr, $expected:expr) => {
                let mut w = String::new();
                let cexpr = $cexpr;
                cexpr.generate_c(&mut w);
                assert_eq!(w, $expected);
            };
        }

        assert_c_expr!(
            CStatement::LiteralConstant {
                value: "\"Hello, world!\"".to_string()
            },
            "\"Hello, world!\""
        );
        assert_c_expr!(
            CStatement::TypeDef {
                source_type: CType::Function {
                    arguments: vec![CType::I32, CType::I32],
                    return_value: None,
                },
                new_name: "my_func_ptr".to_string(),
            },
            "typedef void (*my_func_ptr)(int, int);\n"
        );
        assert_c_expr!(
            CStatement::LiteralArray {
                items: vec![
                    CStatement::LiteralConstant {
                        value: "1".to_string()
                    },
                    CStatement::LiteralConstant {
                        value: "2".to_string()
                    },
                    CStatement::LiteralConstant {
                        value: "3".to_string()
                    },
                ]
            },
            "{\n\t1,\n\t2,\n\t3,\n}"
        );
        assert_c_expr!(CStatement::LiteralArray { items: vec![] }, "{}");
        assert_c_expr!(
            CStatement::Declaration {
                name: "my_array".to_string(),
                is_extern: false,
                is_const: true,
                ctype: CType::Array {
                    inner: Box::new(CType::I32)
                },
                definition: Some(Box::new(CStatement::LiteralArray {
                    items: vec![
                        CStatement::LiteralConstant {
                            value: "1".to_string()
                        },
                        CStatement::LiteralConstant {
                            value: "2".to_string()
                        },
                        CStatement::LiteralConstant {
                            value: "3".to_string()
                        },
                    ]
                }))
            },
            "const int my_array[] = {\n\t1,\n\t2,\n\t3,\n};\n"
        );
        assert_c_expr!(
            CStatement::Declaration {
                name: "my_array".to_string(),
                is_extern: true,
                is_const: true,
                ctype: CType::Array {
                    inner: Box::new(CType::I32)
                },
                definition: None,
            },
            "const extern int my_array[];\n"
        );
    }
}
