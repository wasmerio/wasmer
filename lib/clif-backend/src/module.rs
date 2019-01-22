use crate::resolver::FuncResolverBuilder;
use cranelift_codegen::{ir, isa};
use cranelift_entity::EntityRef;
use cranelift_wasm;
use hashbrown::HashMap;
use std::{
    ops::{Deref, DerefMut},
    ptr::NonNull,
};
use wasmer_runtime::{
    backend::FuncResolver,
    backend::SigRegistry,
    error::CompileResult,
    module::ModuleInner,
    structures::{Map, TypedIndex},
    types::{
        FuncIndex, FuncSig, GlobalIndex, LocalFuncIndex, MemoryIndex, SigIndex, TableIndex, Type,
    },
    vm,
};

struct PlaceholderFuncResolver;

impl FuncResolver for PlaceholderFuncResolver {
    fn get(
        &self,
        _module: &ModuleInner,
        _local_func_index: LocalFuncIndex,
    ) -> Option<NonNull<vm::Func>> {
        None
    }
}

/// This contains all of the items in a `ModuleInner` except the `func_resolver`.
pub struct Module {
    pub module: ModuleInner,
}

impl Module {
    pub fn empty() -> Self {
        Self {
            module: ModuleInner {
                // this is a placeholder
                func_resolver: Box::new(PlaceholderFuncResolver),
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
                sig_registry: SigRegistry::new(),
            },
        }
    }

    pub fn compile(
        mut self,
        isa: &isa::TargetIsa,
        functions: Map<LocalFuncIndex, ir::Function>,
    ) -> CompileResult<ModuleInner> {
        // we have to deduplicate `module.func_assoc`
        let func_assoc = &mut self.module.func_assoc;
        let sig_registry = &self.module.sig_registry;
        func_assoc.iter_mut().for_each(|(_, sig_index)| {
            *sig_index = sig_registry.lookup_deduplicated_sigindex(*sig_index);
        });
        let imported_functions_len = self.module.imported_functions.len();
        let func_resolver_builder = FuncResolverBuilder::new(isa, functions, imported_functions_len)?;
        self.module.func_resolver = Box::new(func_resolver_builder.finalize()?);
        Ok(self.module)
    }
}

impl Deref for Module {
    type Target = ModuleInner;

    fn deref(&self) -> &ModuleInner {
        &self.module
    }
}

impl DerefMut for Module {
    fn deref_mut(&mut self) -> &mut ModuleInner {
        &mut self.module
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
        FuncSig {
            params: signature
                .0
                .params
                .iter()
                .map(|param| Converter(param.value_type).into())
                .collect(),
            returns: signature
                .0
                .returns
                .iter()
                .map(|ret| Converter(ret.value_type).into())
                .collect(),
        }
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
