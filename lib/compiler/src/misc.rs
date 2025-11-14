//! A common functionality used among various compilers.

use itertools::Itertools;
use wasmer_types::{FunctionType, LocalFunctionIndex, Type};

/// Represents the kind of compiled function or module, used for debugging and identification
/// purposes across multiple compiler backends (e.g., LLVM, Cranelift).
#[derive(Debug, Clone)]
pub enum CompiledKind {
    /// A locally-defined function in the Wasm file.
    Local(LocalFunctionIndex, String),
    /// A function call trampoline for a given signature.
    FunctionCallTrampoline(FunctionType),
    /// A dynamic function trampoline for a given signature.
    DynamicFunctionTrampoline(FunctionType),
    /// An entire Wasm module.
    Module,
}

/// Converts a slice of `Type` into a string signature, mapping each type to a specific character.
/// Used to represent function signatures in a compact string form.
pub fn types_to_signature(types: &[Type]) -> String {
    let tokens = types
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
        .collect_vec();
    // Apparently, LLVM has issues if the filename is too long, thus we compact it.
    tokens
        .chunk_by(|a, b| a == b)
        .map(|chunk| {
            if chunk.len() >= 8 {
                format!("{}x{}", chunk.len(), chunk[0])
            } else {
                chunk.to_owned().join("")
            }
        })
        .join("")
}

/// Sanitizes a string so it can be safely used as a filename.
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Converts a kind into a filename, that we will use to dump
/// the contents of the IR object file to.
pub fn function_kind_to_filename(kind: &CompiledKind, suffix: &str) -> String {
    match kind {
        CompiledKind::Local(local_func_index, name) => {
            let mut name = sanitize_filename(name);

            // Limit to 255 characters to comply with common filesystem path component restrictions.
            const PATH_LIMIT: usize = 255;

            if name.len() + suffix.len() > PATH_LIMIT {
                let id_string = local_func_index.as_u32().to_string();
                name.truncate(PATH_LIMIT - id_string.len() - suffix.len() - 1);
                name.push_str(format!("_{id_string}{suffix}").as_str());
            } else {
                name.push_str(suffix);
            }

            debug_assert!(name.len() <= PATH_LIMIT);
            name
        }
        CompiledKind::FunctionCallTrampoline(func_type) => format!(
            "trampoline_call_{}_{}",
            types_to_signature(func_type.params()),
            types_to_signature(func_type.results())
        ),
        CompiledKind::DynamicFunctionTrampoline(func_type) => format!(
            "trampoline_dynamic_{}_{}",
            types_to_signature(func_type.params()),
            types_to_signature(func_type.results())
        ),
        CompiledKind::Module => "module".into(),
    }
}
