use wasmer_runtime_core::types::Type;
use wasmparser::{BinaryReaderError, Type as WpType, TypeOrFuncType as WpTypeOrFuncType};

pub fn type_to_type(ty: WpType) -> Result<Type, BinaryReaderError> {
    Ok(match ty {
        WpType::I32 => Type::I32,
        WpType::I64 => Type::I64,
        WpType::F32 => Type::F32,
        WpType::F64 => Type::F64,
        WpType::V128 => {
            return Err(BinaryReaderError {
                message: "the wasmer llvm backend does not yet support the simd extension",
                offset: -1isize as usize,
            });
        }
        _ => {
            return Err(BinaryReaderError {
                message: "that type is not supported as a wasmer type",
                offset: -1isize as usize,
            });
        }
    })
}

pub fn blocktype_to_type(ty: WpTypeOrFuncType) -> Result<Type, BinaryReaderError> {
    match ty {
        WpTypeOrFuncType::Type(inner_ty) => type_to_type(inner_ty),
        _ => {
            return Err(BinaryReaderError {
                message:
                    "the wasmer llvm backend does not yet support the multi-value return extension",
                offset: -1isize as usize,
            });
        }
    }
}
