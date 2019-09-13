use std::{convert::TryFrom, str};

#[derive(PartialEq, Debug)]
pub enum InterfaceType {
    Int,
    Float,
    Any,
    String,
    Seq,

    I32,
    I64,
    F32,
    F64,
    AnyRef,
}

impl TryFrom<u64> for InterfaceType {
    type Error = &'static str;

    fn try_from(code: u64) -> Result<Self, Self::Error> {
        Ok(match code {
            0x7fff => Self::Int,
            0x7ffe => Self::Float,
            0x7ffd => Self::Any,
            0x7ffc => Self::String,
            0x7ffb => Self::Seq,
            0x7f => Self::I32,
            0x7e => Self::I64,
            0x7d => Self::F32,
            0x7c => Self::F64,
            0x6f => Self::AnyRef,
            _ => return Err("Unknown interface type code."),
        })
    }
}

#[derive(PartialEq, Debug)]
pub(crate) enum AdapterKind {
    Import,
    Export,
    HelperFunction,
}

impl TryFrom<u8> for AdapterKind {
    type Error = &'static str;

    fn try_from(code: u8) -> Result<Self, Self::Error> {
        Ok(match code {
            0x0 => Self::Import,
            0x1 => Self::Export,
            0x2 => Self::HelperFunction,
            _ => return Err("Unknown adapter kind code."),
        })
    }
}

#[derive(PartialEq, Debug)]
pub enum Instruction<'input> {
    ArgumentGet(u64),
    Call(u64),
    CallExport(&'input str),
    ReadUtf8,
    WriteUtf8(&'input str),
    AsWasm(InterfaceType),
    AsInterface(InterfaceType),
    TableRefAdd,
    TableRefGet,
    CallMethod(u64),
    MakeRecord(InterfaceType),
    GetField(u64, u64),
    Const(InterfaceType, u64),
    FoldSeq(u64),
}

#[derive(PartialEq, Debug)]
pub struct Export<'input> {
    pub name: &'input str,
    pub input_types: Vec<InterfaceType>,
    pub output_types: Vec<InterfaceType>,
}

#[derive(PartialEq, Debug)]
pub struct Type<'input> {
    pub name: &'input str,
    pub fields: Vec<&'input str>,
    pub types: Vec<InterfaceType>,
}

#[derive(PartialEq, Debug)]
pub struct ImportedFunction<'input> {
    pub namespace: &'input str,
    pub name: &'input str,
    pub input_types: Vec<InterfaceType>,
    pub output_types: Vec<InterfaceType>,
}

#[derive(PartialEq, Debug)]
pub enum Adapter<'input> {
    Import {
        namespace: &'input str,
        name: &'input str,
        input_types: Vec<InterfaceType>,
        output_types: Vec<InterfaceType>,
        instructions: Vec<Instruction<'input>>,
    },
    Export {
        name: &'input str,
        input_types: Vec<InterfaceType>,
        output_types: Vec<InterfaceType>,
        instructions: Vec<Instruction<'input>>,
    },
    HelperFunction {
        name: &'input str,
        input_types: Vec<InterfaceType>,
        output_types: Vec<InterfaceType>,
        instructions: Vec<Instruction<'input>>,
    },
}

#[derive(PartialEq, Debug)]
pub struct Forward<'input> {
    pub name: &'input str,
}

#[derive(PartialEq, Debug)]
pub struct Interfaces<'input> {
    pub exports: Vec<Export<'input>>,
    pub types: Vec<Type<'input>>,
    pub imported_functions: Vec<ImportedFunction<'input>>,
    pub adapters: Vec<Adapter<'input>>,
    pub forwards: Vec<Forward<'input>>,
}
