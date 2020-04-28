//! Represents the WIT language as a tree. This is the central
//! representation of the language.

use crate::{
    interpreter::Instruction,
    types::{InterfaceType, RecordType},
};
use std::str;

/// Represents the kind of type.
#[derive(PartialEq, Debug)]
pub enum TypeKind {
    /// A function type.
    Function,

    /// A record type.
    Record,
}

/// Represents a type.
#[derive(PartialEq, Debug)]
pub enum Type {
    /// A function type, like:
    ///
    /// ```wasm,ignore
    /// (@interface type (func (param i32 i32) (result string)))
    /// ```
    Function {
        /// Types for the parameters (`(param …)`).
        inputs: Vec<InterfaceType>,

        /// Types for the results (`(result …)`).
        outputs: Vec<InterfaceType>,
    },

    /// A record type, like:
    ///
    /// ```wasm,ignore
    /// (@interface type (record string i32))
    /// ```
    Record(RecordType),
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
pub struct Adapter {
    /// The adapter function type.
    pub function_type: u32,

    /// The instructions.
    pub instructions: Vec<Instruction>,
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
    pub adapters: Vec<Adapter>,

    /// All the exported functions.
    pub exports: Vec<Export<'input>>,

    /// All the implementations.
    pub implementations: Vec<Implementation>,
}
