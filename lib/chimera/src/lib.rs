#![feature(stdsimd, futures_api)]
#![cfg_attr(nightly, feature(unwind_attributes))]

#[macro_use]
extern crate log;

// use wasmer_runtime_core::{
//     backend::{Compiler, Token},
//     cache::{Artifact, Error as CacheError},
//     error::CompileError,
//     module::ModuleInner,
// };

pub mod alloc_pool;
pub mod clif;
pub mod code;
pub mod compile_pool;
pub mod llvm;
pub mod module_ctx;
pub mod pipeline;
pub mod state;
pub mod thunks;
pub mod utils;

pub trait TieredCompiler {}

pub struct Chimera {}
