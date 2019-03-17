#![feature(proc_macro_hygiene)]

#[cfg(not(any(
    all(target_os = "macos", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "x86_64"),
)))]
compile_error!("This crate doesn't yet support compiling on operating systems other than linux and macos and architectures other than x86_64");

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
mod protect_unix;

use crate::codegen::{CodegenError, ModuleCodeGenerator};
use crate::parse::LoadError;
use std::ptr::NonNull;
use wasmer_runtime_core::{
    backend::{sys::Memory, Backend, CacheGen, Compiler, FuncResolver, Token},
    cache::{Artifact, Error as CacheError},
    error::{CompileError, CompileResult},
    module::{ModuleInfo, ModuleInner},
    types::{
        LocalFuncIndex,
    },
    vm,
};

struct Placeholder;
impl CacheGen for Placeholder {
    fn generate_cache(
        &self,
        _module: &ModuleInner,
    ) -> Result<(Box<ModuleInfo>, Box<[u8]>, Memory), CacheError> {
        // unimplemented!()
        Err(CacheError::Unknown("the dynasm backend doesn't support caching yet".to_string()))
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
        Err(CacheError::Unknown("the dynasm backend doesn't support caching yet".to_string()))
        // unimplemented!("the dynasm backend doesn't support caching yet")
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
