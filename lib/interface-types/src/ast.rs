//! Represents the WIT language as a tree. This is the central
//! representation of the language.

use crate::interpreter::Instruction;
use std::str;

/// Represents the types supported by WIT.
#[derive(PartialEq, Debug)]
pub enum InterfaceType {
    /// A 8-bits signed integer.
    S8,

    /// A 16-bits signed integer.
    S16,

    /// A 32-bits signed integer.
    S32,

    /// A 64-bits signed integer.
    S64,

    /// A 8-bits unsigned integer.
    U8,

    /// A 16-bits unsigned integer.
    U16,

    /// A 32-bits unsigned integer.
    U32,

    /// A 64-bits unsigned integer.
    U64,

    /// A 32-bits float.
    F32,

    /// A 64-bits float.
    F64,

    /// A string.
    String,

    /// An `any` reference.
    Anyref,

    /// A 32-bits integer (as defined in WebAssembly core).
    I32,

    /// A 64-bits integer (as defiend in WebAssembly core).
    I64,
}

/// Represents a type signature.
#[derive(PartialEq, Debug)]
pub struct Type {
    /// Types for the parameters.
    pub inputs: Vec<InterfaceType>,

    /// Types for the results.
    pub outputs: Vec<InterfaceType>,
}

/// Represents an imported function.
#[derive(PartialEq, Debug)]
pub struct Import<'input> {
    /// The function namespace.
    pub namespace: &'input str,

    /// The function name.
    pub name: &'input str,

    /// The type signature.
    pub signature_type: u32,
}

/// Represents an exported function signature.
#[derive(PartialEq, Debug)]
pub struct Export<'input> {
    /// The export name.
    pub name: &'input str,

    /// The WIT function type being exported.
    pub function_type: u32,
}

/// Represents an adapter.
#[derive(PartialEq, Debug)]
pub struct Adapter<'input> {
    /// The adapter function type.
    pub function_type: u32,

    /// The instructions.
    pub instructions: Vec<Instruction<'input>>,
}

/// Represents an implementation.
#[derive(PartialEq, Debug)]
pub struct Implementation {
    /// The core function type.
    pub core_function_type: u32,

    /// The adapter function type.
    pub adapter_function_type: u32,
}

/// Represents the kind of interface.
#[derive(PartialEq, Debug)]
pub(crate) enum InterfaceKind {
    /// A type.
    Type,

    /// An imported function.
    Import,

    /// An adapter.
    Adapter,

    /// An exported function.
    Export,

    /// An implementation.
    Implementation,
}

/// Represents a set of interfaces, i.e. it entirely describes a WIT
/// definition.
#[derive(PartialEq, Default, Debug)]
pub struct Interfaces<'input> {
    /// All the types.
    pub types: Vec<Type>,

    /// All the imported functions.
    pub imports: Vec<Import<'input>>,

    /// All the adapters.
    pub adapters: Vec<Adapter<'input>>,

    /// All the exported functions.
    pub exports: Vec<Export<'input>>,

    /// All the implementations.
    pub implementations: Vec<Implementation>,
}
