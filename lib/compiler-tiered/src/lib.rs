//! A WebAssembly `Compiler` implementation using Tiered compilation.

mod compiler;
mod config;

pub use crate::compiler::TieredCompiler;
pub use crate::config::Tiered;
