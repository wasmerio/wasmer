use crate::cache::{BackendCache, CacheGenerator};
use crate::{resolver::FuncResolverBuilder, signal::Caller};

use cranelift_codegen::ir;
use cranelift_entity::EntityRef;
use cranelift_wasm;
use std::sync::Arc;

use wasmer_runtime_core::cache::{Artifact, Error as CacheError};

use wasmer_runtime_core::{
    module::{ModuleInfo, ModuleInner},
    structures::TypedIndex,
    types::{FuncIndex, FuncSig, GlobalIndex, MemoryIndex, SigIndex, TableIndex, Type},
};

/// This contains all of the items in a `ModuleInner` except the `func_resolver`.
pub struct Module {
    pub info: ModuleInfo,
}

impl Module {
    pub fn from_cache(cache: Artifact) -> Result<ModuleInner, CacheError> {
        let (info, compiled_code, backend_cache) = BackendCache::from_cache(cache)?;

        let (func_resolver_builder, trampolines, handler_data) =
            FuncResolverBuilder::new_from_backend_cache(backend_cache, compiled_code, &info)?;

        let (func_resolver, backend_cache) = func_resolver_builder
            .finalize(
                &info.signatures,
                Arc::clone(&trampolines),
                handler_data.clone(),
            )
            .map_err(|e| CacheError::Unknown(format!("{:?}", e)))?;

        let cache_gen = Box::new(CacheGenerator::new(
            backend_cache,
            Arc::clone(&func_resolver.memory),
        ));

        let runnable_module = Caller::new(handler_data, trampolines, func_resolver);

        Ok(ModuleInner {
            runnable_module: Arc::new(Box::new(runnable_module)),
            cache_gen,
            info,
        })
    }
}

pub struct Converter<T>(pub T);

macro_rules! convert_clif_to_runtime_index {
    ($clif_index:ident, $runtime_index:ident) => {
        impl From<Converter<cranelift_wasm::$clif_index>> for $runtime_index {
            fn from(clif_index: Converter<cranelift_wasm::$clif_index>) -> Self {
                $runtime_index::new(clif_index.0.index())
            }
        }

        impl From<Converter<$runtime_index>> for cranelift_wasm::$clif_index {
            fn from(runtime_index: Converter<$runtime_index>) -> Self {
                cranelift_wasm::$clif_index::new(runtime_index.0.index())
            }
        }
    };
    ($(($clif_index:ident: $runtime_index:ident),)*) => {
        $(
            convert_clif_to_runtime_index!($clif_index, $runtime_index);
        )*
    };
}

convert_clif_to_runtime_index![
    (FuncIndex: FuncIndex),
    (MemoryIndex: MemoryIndex),
    (TableIndex: TableIndex),
    (GlobalIndex: GlobalIndex),
    (SignatureIndex: SigIndex),
];

impl From<Converter<ir::Signature>> for FuncSig {
    fn from(signature: Converter<ir::Signature>) -> Self {
        FuncSig::new(
            signature
                .0
                .params
                .iter()
                .map(|param| Converter(param.value_type).into())
                .collect::<Vec<_>>(),
            signature
                .0
                .returns
                .iter()
                .map(|ret| Converter(ret.value_type).into())
                .collect::<Vec<_>>(),
        )
    }
}

impl From<Converter<ir::Type>> for Type {
    fn from(ty: Converter<ir::Type>) -> Self {
        match ty.0 {
            ir::types::I32 => Type::I32,
            ir::types::I64 => Type::I64,
            ir::types::F32 => Type::F32,
            ir::types::F64 => Type::F64,
            ir::types::I32X4 => Type::V128,
            _ => unimplemented!("unsupported wasm type"),
        }
    }
}

impl From<Converter<Type>> for ir::Type {
    fn from(ty: Converter<Type>) -> Self {
        match ty.0 {
            Type::I32 => ir::types::I32,
            Type::I64 => ir::types::I64,
            Type::F32 => ir::types::F32,
            Type::F64 => ir::types::F64,
            Type::V128 => ir::types::I32X4,
        }
    }
}

impl From<Converter<Type>> for ir::AbiParam {
    fn from(ty: Converter<Type>) -> Self {
        match ty.0 {
            Type::I32 => ir::AbiParam::new(ir::types::I32),
            Type::I64 => ir::AbiParam::new(ir::types::I64),
            Type::F32 => ir::AbiParam::new(ir::types::F32),
            Type::F64 => ir::AbiParam::new(ir::types::F64),
            Type::V128 => ir::AbiParam::new(ir::types::I32X4),
        }
    }
}
