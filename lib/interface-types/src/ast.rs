//! Represents the WIT language as a tree. This is the central
//! representation of the language.

use crate::interpreter::Instruction;
use std::str;

/// Represents the types supported by WIT.
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum InterfaceType {
    /// An integer.
    Int,

    /// A float.
    Float,

    /// Opaque reference.
    Any,

    /// A string.
    String,

    /// A sequence.
    Seq,

    /// A 32-bits integer.
    I32,

    /// A 64-bits integer.
    I64,

    /// A 32-bits float.
    F32,

    /// A 64-bits float.
    F64,

    /// An `any` reference.
    AnyRef,
}

/// Represents the kind of adapter.
#[derive(PartialEq, Debug)]
pub(crate) enum AdapterKind {
    /// An adapter defined for an imported function of a WebAssembly instance.
    Import,

    /// An adapter defined for an exported function of a WebAssembly instance.
    Export,

    /// A helper function.
    HelperFunction,
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

/// Represents a structural type.
#[derive(PartialEq, Debug)]
pub struct Type<'input> {
    /// The type name.
    pub name: &'input str,

    /// The field names.
    field_names: Vec<&'input str>,

    /// The field types.
    field_types: Vec<InterfaceType>,
}

impl<'input> Type<'input> {
    /// Creates a new `Type`.
    ///
    /// The constructor panics if there is the length of `names` is
    /// different than the length of `types`.
    pub fn new(type_name: &'input str, names: Vec<&'input str>, types: Vec<InterfaceType>) -> Self {
        assert_eq!(
            names.len(),
            types.len(),
            "There must be the same number of field names than field types."
        );

        Self {
            name: type_name,
            field_names: names,
            field_types: types,
        }
    }

    /// Adds a new field to the type.
    pub fn add_field(&mut self, name: &'input str, ty: InterfaceType) {
        self.field_names.push(name);
        self.field_types.push(ty);
    }

    /// Returns the field names.
    pub fn field_names(&self) -> &Vec<&'input str> {
        &self.field_names
    }

    /// Returns the field types.
    pub fn field_types(&self) -> &Vec<InterfaceType> {
        &self.field_types
    }
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

    /// An adapter for a helper function.
    HelperFunction {
        /// The helper name.
        name: &'input str,

        /// The helper input types.
        input_types: Vec<InterfaceType>,

        /// The helper output types.
        output_types: Vec<InterfaceType>,

        /// The instructions of the adapter.
        instructions: Vec<Instruction<'input>>,
    },
}

/// Represented a forwarded export.
#[derive(PartialEq, Debug)]
pub struct Forward<'input> {
    /// The forwarded export name.
    pub name: &'input str,
}

/// Represents a set of interfaces, i.e. it entirely describes a WIT
/// definition.
#[derive(PartialEq, Default, Debug)]
pub struct Interfaces<'input> {
    /// All the exported functions.
    pub exports: Vec<Export<'input>>,

    /// All the types.
    pub types: Vec<Type<'input>>,

    /// All the imported functions.
    pub imports: Vec<Import<'input>>,

    /// All the adapters.
    pub adapters: Vec<Adapter<'input>>,

    /// All the forwarded functions.
    pub forwards: Vec<Forward<'input>>,
}
