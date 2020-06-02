use wasm_common::{SignatureIndex, Type};
use wasmer_compiler::wasmparser::Type as WpType;
use wasmer_compiler::wasmparser::TypeOrFuncType as WpTypeOrFuncType;
use wasmer_compiler::CompileError;
use wasmer_runtime::ModuleInfo;

pub fn blocktype_to_types(ty: WpTypeOrFuncType, info: &ModuleInfo) -> Vec<Type> {
    match ty {
        WpTypeOrFuncType::Type(WpType::EmptyBlockType) => vec![],
        WpTypeOrFuncType::Type(inner_ty) => vec![wp_type_to_type(inner_ty).unwrap()],
        WpTypeOrFuncType::FuncType(sig_index) => {
            let ty = &info.signatures[SignatureIndex::from_u32(sig_index)];
            ty.results().to_vec()
        }
    }
}

pub fn blocktype_to_param_types(ty: WpTypeOrFuncType, info: &ModuleInfo) -> Vec<Type> {
    match ty {
        WpTypeOrFuncType::Type(_) => vec![],
        WpTypeOrFuncType::FuncType(sig_index) => {
            let ty = &info.signatures[SignatureIndex::from_u32(sig_index)];
            ty.params().to_vec()
        }
    }
}

fn wp_type_to_type(ty: WpType) -> Result<Type, CompileError> {
    match ty {
        WpType::I32 => Ok(Type::I32),
        WpType::I64 => Ok(Type::I64),
        WpType::F32 => Ok(Type::F32),
        WpType::F64 => Ok(Type::F64),
        WpType::V128 => Ok(Type::V128),
        _ => Err(CompileError::Codegen(
            "broken invariant, invalid type".to_string(),
        )),
    }
}
