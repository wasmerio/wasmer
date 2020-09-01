#[derive(Debug, Clone)]
pub enum CType {
    Void,
    PointerTo {
        is_const: bool,
        inner: Box<CType>,
    },
    U8,
    U16,
    U32,
    U64,
    USize,
    I8,
    I16,
    I32,
    I64,
    ISize,
    Function {
        arguments: Vec<CType>,
        return_value: Option<Box<CType>>,
    },
}

impl CType {
    pub fn void_ptr() -> Self {
        CType::PointerTo {
            is_const: false,
            inner: Box::new(CType::Void),
        }
    }

    #[allow(dead_code)]
    pub fn const_void_ptr() -> Self {
        CType::PointerTo {
            is_const: false,
            inner: Box::new(CType::Void),
        }
    }

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

#[derive(Debug, Clone)]
pub enum CStatement {
    Declaration {
        name: String,
        array: bool,
        is_extern: bool,
        is_const: bool,
        ctype: CType,
        definition: Option<Box<CStatement>>,
    },
    LiteralArray {
        items: Vec<CStatement>,
    },
    LiteralConstant {
        value: String,
    },
}

impl CStatement {
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

// TODO: add config section
pub fn generate_c(statements: &[CStatement]) -> String {
    let mut out = String::new();
    for statement in statements {
        statement.generate_c(&mut out);
    }
    out
}
