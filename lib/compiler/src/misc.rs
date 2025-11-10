//! A common functionality used among various compilers.

use wasmer_types::{FunctionType, LocalFunctionIndex, Type, entity::EntityRef};

/// The compiled function kind, used for debugging in the `LLVMCallbacks`.
#[derive(Debug, Clone)]
pub enum CompiledKind {
    /// A locally-defined function in the Wasm file.
    Local(LocalFunctionIndex),
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
    types
        .iter()
        .map(|ty| match ty {
            Type::I32 => "i".to_string(),
            Type::I64 => "I".to_string(),
            Type::F32 => "f".to_string(),
            Type::F64 => "F".to_string(),
            Type::V128 => "v".to_string(),
            Type::ExternRef => "e".to_string(),
            Type::FuncRef => "r".to_string(),
            Type::ExceptionRef => "x".to_string(),
        })
        .collect::<Vec<_>>()
        .join("")
}
/// Converts a kind into a filename, that we will use to dump
/// the contents of the IR object file to.
pub fn function_kind_to_filename(kind: &CompiledKind) -> String {
    match kind {
        CompiledKind::Local(local_index) => {
            format!("function_{}", local_index.index())
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
