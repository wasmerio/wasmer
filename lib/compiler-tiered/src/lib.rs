//! A WebAssembly `Compiler` implementation using Tiered compilation.

mod caching;
mod compiler;
mod config;

pub use crate::caching::DefaultTieredCaching;
pub use crate::caching::TieredCaching;
pub use crate::compiler::TieredCompiler;
pub use crate::config::Tiered;
