//! Represents the WIT language as a tree. This is the central
//! representation of the language.

use crate::interpreter::Instruction;
use std::str;

#[derive(PartialEq, Clone, Debug)]
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

#[derive(PartialEq, Debug)]
pub(crate) enum AdapterKind {
    Import,
    Export,
    HelperFunction,
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
