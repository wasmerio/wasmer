//! A WebAssembly `Compiler` implementation using Singlepass.
//!
//! Singlepass is a super-fast assembly generator that generates
//! assembly code in just one pass. This is useful for different applications
//! including Blockchains and Edge computing where quick compilation
//! times are a must, and JIT bombs should never happen.
//!
//! Compared to Cranelift and LLVM, Singlepass compiles much faster but has worse
//! runtime performance.
//!
//! > Note: Singlepass currently depends on Rust nightly features.

#![feature(proc_macro_hygiene)]

mod compiler;
mod config;
mod codegen_x64;
mod common_decl;
mod emitter_x64;
mod machine;
mod x64_decl;
mod exception;

pub use crate::compiler::SinglepassCompiler;
pub use crate::config::SinglepassConfig;
