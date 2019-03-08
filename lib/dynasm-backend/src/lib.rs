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
        NonNull::new(0x3f3f3f3f3f3f3f3fusize as *mut vm::Func)
    }
}

pub struct SinglePassCompiler {}

impl Compiler for SinglePassCompiler {
    fn compile(&self, wasm: &[u8], _: Token) -> CompileResult<ModuleInner> {
        let mut mcg = codegen_x64::X64ModuleCodeGenerator::new();
        let info = parse::read_module(wasm, Backend::Dynasm, &mut mcg)?;
        let ec = mcg.finalize()?;
        Ok(ModuleInner {
            func_resolver: Box::new(Placeholder),
            protected_caller: Box::new(ec),
            info: info,
        })
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
