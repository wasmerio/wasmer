use crate::{call::Caller, resolver::FuncResolverBuilder, trampoline::Trampolines};
use cranelift_codegen::{ir, isa};
use cranelift_entity::EntityRef;
use cranelift_wasm;
use hashbrown::HashMap;
use std::{
    ops::{Deref, DerefMut},
    ptr::NonNull,
};
use wasmer_runtime_core::{
    backend::SigRegistry,
    backend::{FuncResolver, ProtectedCaller, Token},
    error::{CompileResult, RuntimeResult},
    module::ModuleInner,
    structures::{Map, TypedIndex},
    types::{
        FuncIndex, FuncSig, GlobalIndex, LocalFuncIndex, MemoryIndex, SigIndex, TableIndex, Type,
        Value,
    },
    vm::{self, ImportBacking},
};

struct Placeholder;

impl FuncResolver for Placeholder {
    fn get(
        &self,
        _module: &ModuleInner,
        _local_func_index: LocalFuncIndex,
    ) -> Option<NonNull<vm::Func>> {
        None
    }
}

impl ProtectedCaller for Placeholder {
    fn call(
        &self,
        _module: &ModuleInner,
        _func_index: FuncIndex,
        _params: &[Value],
        _import_backing: &ImportBacking,
        _vmctx: *mut vm::Ctx,
        _: Token,
    ) -> RuntimeResult<Vec<Value>> {
        Ok(vec![])
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
                func_resolver: Box::new(Placeholder),
                protected_caller: Box::new(Placeholder),

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
                sig_registry: SigRegistry,
            },
        }
    }

    pub fn compile(
        mut self,
        isa: &isa::TargetIsa,
        functions: Map<LocalFuncIndex, ir::Function>,
    ) -> CompileResult<ModuleInner> {
        let imported_functions_len = self.module.imported_functions.len();
        let (func_resolver_builder, handler_data) =
            FuncResolverBuilder::new(isa, functions, imported_functions_len)?;

        self.module.func_resolver = Box::new(func_resolver_builder.finalize()?);

        let trampolines = Trampolines::new(isa, &self.module);

        self.module.protected_caller =
            Box::new(Caller::new(&self.module, handler_data, trampolines));

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
