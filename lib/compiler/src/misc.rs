//! Misc utilities used in a compiler.

use wasmer_types::Type;

/// Converts a slice of WebAssembly types into a compact signature string (e.g., "iI" for [i32, i64]).
/// This signature string can be used as part of a filename or for other identification purposes.
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
        .collect::<String>()
}
