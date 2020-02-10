use wasmer_runtime_core::parse::wp_type_to_type;
use wasmer_runtime_core::types::Type;
use wasmparser::{BinaryReaderError, TypeOrFuncType as WpTypeOrFuncType};

pub fn blocktype_to_type(ty: WpTypeOrFuncType) -> Result<Type, BinaryReaderError> {
    match ty {
        WpTypeOrFuncType::Type(inner_ty) => wp_type_to_type(inner_ty),
        _ => {
            return Err(BinaryReaderError {
                message:
                    "the wasmer llvm backend does not yet support the multi-value return extension",
                offset: -1isize as usize,
            });
        }
    }
}
