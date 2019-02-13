#![feature(proc_macro_hygiene)]

#[macro_use]
extern crate dynasmrt;

#[macro_use]
extern crate dynasm;

mod codegen;
mod codegen_x64;
mod parse;
mod stack;

use std::ptr::NonNull;
use wasmer_runtime_core::{
    backend::{Backend, Compiler, FuncResolver, ProtectedCaller, Token, UserTrapper},
    error::{CompileError, CompileResult, RuntimeResult},
    module::{ModuleInfo, ModuleInner, StringTable},
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

    fn get_early_trapper(&self) -> Box<dyn UserTrapper> {
        pub struct Trapper;

        impl UserTrapper for Trapper {
            unsafe fn do_early_trap(&self, msg: String) -> ! {
                panic!("{}", msg);
            }
        }

        Box::new(Trapper)
    }
}

pub struct SinglePassCompiler {}

impl Compiler for SinglePassCompiler {
    fn compile(&self, wasm: &[u8], _: Token) -> CompileResult<ModuleInner> {
        let mut mcg = codegen_x64::X64ModuleCodeGenerator::new();
        let info = match parse::read_module(wasm, Backend::Dynasm, &mut mcg) {
            Ok(x) => x,
            Err(e) => {
                return Err(CompileError::InternalError {
                    msg: format!("{:?}", e),
                })
            }
        };
        Ok(ModuleInner {
            func_resolver: Box::new(Placeholder),
            protected_caller: Box::new(Placeholder),
            info: info,
        })
    }
}
