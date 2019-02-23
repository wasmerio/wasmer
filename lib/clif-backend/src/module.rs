use crate::cache::{BackendCache, CacheGenerator};
use crate::{resolver::FuncResolverBuilder, signal::Caller, trampoline::Trampolines};

use cranelift_codegen::{ir, isa};
use cranelift_entity::EntityRef;
use cranelift_wasm;
use hashbrown::HashMap;
use std::sync::Arc;

use wasmer_runtime_core::cache::{Artifact, Error as CacheError};

use wasmer_runtime_core::{
    backend::Backend,
    error::CompileResult,
    module::{ModuleInfo, ModuleInner, StringTable},
    structures::{Map, TypedIndex},
    types::{
        FuncIndex, FuncSig, GlobalIndex, LocalFuncIndex, MemoryIndex, SigIndex, TableIndex, Type,
    },
};

/// This contains all of the items in a `ModuleInner` except the `func_resolver`.
pub struct Module {
    pub info: ModuleInfo,
}

impl Module {
    pub fn new(wasm: &[u8]) -> Self {
        Self {
            info: ModuleInfo {
                memories: Map::new(),
                globals: Map::new(),
                tables: Map::new(),

                imported_functions: Map::new(),
                imported_memories: Map::new(),
                imported_tables: Map::new(),
                imported_globals: Map::new(),

                exports: HashMap::new(),

                data_initializers: Vec::new(),
                elem_initializers: Vec::new(),

                start_func: None,

                func_assoc: Map::new(),
                signatures: Map::new(),
                backend: Backend::Cranelift,

                namespace_table: StringTable::new(),
                name_table: StringTable::new(),
            },
        }
    }

    pub fn compile(
        self,
        isa: &isa::TargetIsa,
        functions: Map<LocalFuncIndex, ir::Function>,
    ) -> CompileResult<ModuleInner> {
        let (func_resolver_builder, handler_data) =
            FuncResolverBuilder::new(isa, functions, &self.info)?;

        let trampolines = Arc::new(Trampolines::new(isa, &self.info));

        let (func_resolver, backend_cache) = func_resolver_builder.finalize(
            &self.info.signatures,
            Arc::clone(&trampolines),
            handler_data.clone(),
        )?;

        let protected_caller = Caller::new(&self.info, handler_data, trampolines);

        let cache_gen = Box::new(CacheGenerator::new(
            backend_cache,
            Arc::clone(&func_resolver.memory),
        ));

        Ok(ModuleInner {
            func_resolver: Box::new(func_resolver),
            protected_caller: Box::new(protected_caller),
            cache_gen,

            info: self.info,
        })
    }

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

        let protected_caller = Caller::new(&info, handler_data, trampolines);

        let cache_gen = Box::new(CacheGenerator::new(
            backend_cache,
            Arc::clone(&func_resolver.memory),
        ));

        Ok(ModuleInner {
            func_resolver: Box::new(func_resolver),
            protected_caller: Box::new(protected_caller),
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

impl<'a> From<Converter<&'a ir::Signature>> for FuncSig {
    fn from(signature: Converter<&'a ir::Signature>) -> Self {
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
            _ => panic!("unsupported wasm type"),
        }
    }
}
