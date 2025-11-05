//! Misc utilities used in a compiler.

use wasmer_types::Type;

/// Converts a kind into a filename, that we will use to dump
/// the contents of the IR object file to.
pub fn types_to_signature(types: &[Type]) -> String {
    types
        .iter()
        .map(|ty| match ty {
            Type::I32 => "i",
            Type::I64 => "I",
            Type::F32 => "f",
            Type::F64 => "F",
            Type::V128 => "v",
            Type::ExternRef => "e",
            Type::FuncRef => "r",
            Type::ExceptionRef => "x",
        })
        .map(|t| t.to_string())
        .collect::<Vec<_>>()
        .join("")
}
