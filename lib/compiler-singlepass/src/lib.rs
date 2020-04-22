//! A WebAssembly `Compiler` implementation using Singlepass.
//!
//! Singlepass is a super-fast assembly generator that generates
//! assembly code in just one pass. This is useful for different applications
//! including Blockchains and Edge computing where quick compilation
//! times are a must, and JIT bombs should never happen.
//!
//! Compared to Cranelift and LLVM, Singlepass is much faster to compile.
//! > Note: Singlepass currently depends on Rust nightly features.

mod compiler;
mod config;

pub use crate::compiler::SinglepassCompiler;
pub use crate::config::SinglepassConfig;
