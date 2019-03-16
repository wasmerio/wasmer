#![feature(proc_macro_hygiene)]

#[macro_use]
extern crate dynasmrt;

#[macro_use]
extern crate dynasm;

#[macro_use]
extern crate lazy_static;

extern crate byteorder;

mod codegen;
mod codegen_x64;
mod parse;
mod stack;

use crate::codegen::{CodegenError, ModuleCodeGenerator};
use crate::parse::LoadError;
use std::ptr::NonNull;
use wasmer_runtime_core::{
    backend::{sys::Memory, Backend, CacheGen, Compiler, FuncResolver, ProtectedCaller, Token, UserTrapper},
    cache::{Artifact, Error as CacheError},
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
impl CacheGen for Placeholder {
    fn generate_cache(
        &self,
        module: &ModuleInner,
    ) -> Result<(Box<ModuleInfo>, Box<[u8]>, Memory), CacheError> {
        unimplemented!()
    }
}

impl FuncResolver for Placeholder {
    fn get(
        &self,
        _module: &ModuleInner,
        _local_func_index: LocalFuncIndex,
    ) -> Option<NonNull<vm::Func>> {
        NonNull::new(0x3f3f3f3f3f3f3f3fusize as *mut vm::Func)
    }
}

pub struct SinglePassCompiler {}
impl SinglePassCompiler {
    pub fn new() -> Self {
        Self {}
    }
}

impl Compiler for SinglePassCompiler {
    fn compile(&self, wasm: &[u8], _: Token) -> CompileResult<ModuleInner> {
        let mut mcg = codegen_x64::X64ModuleCodeGenerator::new();
        let info = parse::read_module(wasm, Backend::Dynasm, &mut mcg)?;
        let (ec, resolver) = mcg.finalize(&info)?;
        Ok(ModuleInner {
            cache_gen: Box::new(Placeholder),
            func_resolver: Box::new(resolver),
            protected_caller: Box::new(ec),
            info: info,
        })
    }

    unsafe fn from_cache(&self, _artifact: Artifact, _: Token) -> Result<ModuleInner, CacheError> {
        unimplemented!("the dynasm backend doesn't support caching yet")
    }
}

impl From<CodegenError> for CompileError {
    fn from(other: CodegenError) -> CompileError {
        CompileError::InternalError {
            msg: other.message.into(),
        }
    }
}

impl From<LoadError> for CompileError {
    fn from(other: LoadError) -> CompileError {
        CompileError::InternalError {
            msg: format!("{:?}", other),
        }
    }
}
