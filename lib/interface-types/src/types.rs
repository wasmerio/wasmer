//! This module defines the WIT types.

use crate::vec1::Vec1;

/// Represents the types supported by WIT.
#[derive(PartialEq, Debug, Clone)]
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

    /// A record.
    Record(RecordType),
}

/// Represents a record type.
#[derive(PartialEq, Debug, Clone)]
pub struct RecordType {
    /// Types representing the fields.
    /// A record must have at least one field, hence the
    /// [`Vec1`][crate::vec1::Vec1].
    pub fields: Vec1<InterfaceType>,
}
