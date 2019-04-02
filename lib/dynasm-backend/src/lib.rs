#![deny(warnings)]
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
mod protect_unix;
mod stack;

use crate::codegen::{CodegenError, ModuleCodeGenerator};
use crate::parse::LoadError;
use wasmer_runtime_core::{
    backend::{sys::Memory, Backend, CacheGen, Compiler, CompilerConfig, Token},
    cache::{Artifact, Error as CacheError},
    error::{CompileError, CompileResult},
    module::{ModuleInfo, ModuleInner},
};

struct Placeholder;
impl CacheGen for Placeholder {
    fn generate_cache(
        &self,
        _module: &ModuleInner,
    ) -> Result<(Box<ModuleInfo>, Box<[u8]>, Memory), CacheError> {
        Err(CacheError::Unknown(
            "the dynasm backend doesn't support caching yet".to_string(),
        ))
    }
}

pub struct SinglePassCompiler {}
impl SinglePassCompiler {
    pub fn new() -> Self {
        Self {}
    }
}

impl Compiler for SinglePassCompiler {
    fn compile(
        &self,
        wasm: &[u8],
        compiler_config: CompilerConfig,
        _: Token,
    ) -> CompileResult<ModuleInner> {
        let mut mcg = codegen_x64::X64ModuleCodeGenerator::new();
        let info = parse::read_module(wasm, Backend::Dynasm, &mut mcg, &compiler_config)?;
        let (ec, resolver) = mcg.finalize(&info)?;
        Ok(ModuleInner {
            cache_gen: Box::new(Placeholder),
            func_resolver: Box::new(resolver),
            protected_caller: Box::new(ec),
            info: info,
        })
    }

    unsafe fn from_cache(&self, _artifact: Artifact, _: Token) -> Result<ModuleInner, CacheError> {
        Err(CacheError::Unknown(
            "the dynasm backend doesn't support caching yet".to_string(),
        ))
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
