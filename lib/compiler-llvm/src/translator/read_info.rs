/*
use wasmer_runtime_core::parse::{wp_type_to_type, LoadError};
use wasmer_runtime_core::types::Type;
 */
use wasm_common::Type;
use wasmer_compiler::CompileError;
use wasmparser::Type as WpType;
use wasmparser::TypeOrFuncType as WpTypeOrFuncType;

fn wp_type_to_type(ty: WpType) -> Result<Type, CompileError> {
    match ty {
        WpType::I32 => Ok(Type::I32),
        WpType::I64 => Ok(Type::I64),
        WpType::F32 => Ok(Type::F32),
        WpType::F64 => Ok(Type::F64),
        WpType::V128 => Ok(Type::V128),
        _ => {
            return Err(CompileError::Codegen(
                "broken invariant, invalid type".to_string(),
            ));
        }
    }
}

pub fn blocktype_to_type(ty: WpTypeOrFuncType) -> Result<Type, CompileError> {
    match ty {
        WpTypeOrFuncType::Type(inner_ty) => Ok(wp_type_to_type(inner_ty)?),
        _ => {
            return Err(CompileError::Codegen(
                "the wasmer llvm backend does not yet support the multi-value return extension"
                    .to_string(),
            ));
        }
    }
}
