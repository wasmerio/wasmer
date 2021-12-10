//! A WebAssembly `Compiler` implementation using Singlepass.
//!
//! Singlepass is a super-fast assembly generator that generates
//! assembly code in just one pass. This is useful for different applications
//! including Blockchains and Edge computing where quick compilation
//! times are a must, and JIT bombs should never happen.
//!
//! Compared to Cranelift and LLVM, Singlepass compiles much faster but has worse
//! runtime performance.

mod address_map;
mod codegen;
mod common_decl;
mod compiler;
mod config;
mod emitter_x64;
mod location;
mod machine;
mod machine_x64;
mod x64_decl;
mod arm64_decl;

pub use crate::compiler::SinglepassCompiler;
pub use crate::config::Singlepass;
