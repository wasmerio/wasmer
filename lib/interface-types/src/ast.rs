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

    /// A stirng.
    String,

    /// An `any` reference.
    Anyref,

    /// A 32-bits integer (as defined in WebAssembly core).
    I32,

    /// A 64-bits integer (as defiend in WebAssembly core).
    I64,
}

/// Represents the kind of adapter.
#[derive(PartialEq, Debug)]
pub(crate) enum AdapterKind {
    /// An adapter defined for an imported function of a WebAssembly instance.
    Import,

    /// An adapter defined for an exported function of a WebAssembly instance.
    Export,
}

/// Represents a type signature.
#[derive(PartialEq, Debug)]
pub struct Type {
    /// Types for the parameters.
    pub inputs: Vec<InterfaceType>,

    /// Types for the results.
    pub outputs: Vec<InterfaceType>,
}

/// Represents an exported function signature.
#[derive(PartialEq, Debug)]
pub struct Export<'input> {
    /// The function name.
    pub name: &'input str,

    /// The function input types.
    pub input_types: Vec<InterfaceType>,

    /// The function output types.
    pub output_types: Vec<InterfaceType>,
}

/// Represents an imported function signature.
#[derive(PartialEq, Debug)]
pub struct Import<'input> {
    /// The function namespace.
    pub namespace: &'input str,

    /// The function name.
    pub name: &'input str,

    /// The function input types.
    pub input_types: Vec<InterfaceType>,

    /// The function output types.
    pub output_types: Vec<InterfaceType>,
}

/// Represents an adapter.
#[derive(PartialEq, Debug)]
pub enum Adapter<'input> {
    /// An adapter for an imported function.
    Import {
        /// The function namespace.
        namespace: &'input str,

        /// The function name.
        name: &'input str,

        /// The function input types.
        input_types: Vec<InterfaceType>,

        /// The function output types.
        output_types: Vec<InterfaceType>,

        /// The instructions of the adapter.
        instructions: Vec<Instruction<'input>>,
    },

    /// An adapter for an exported function.
    Export {
        /// The function name.
        name: &'input str,

        /// The function input types.
        input_types: Vec<InterfaceType>,

        /// The function output types.
        output_types: Vec<InterfaceType>,

        /// The instructions of the adapter.
        instructions: Vec<Instruction<'input>>,
    },
}

/// Represents a set of interfaces, i.e. it entirely describes a WIT
/// definition.
#[derive(PartialEq, Default, Debug)]
pub struct Interfaces<'input> {
    /// All the types.
    pub types: Vec<Type>,

    /// All the exported functions.
    pub exports: Vec<Export<'input>>,

    /// All the imported functions.
    pub imports: Vec<Import<'input>>,

    /// All the adapters.
    pub adapters: Vec<Adapter<'input>>,
}
