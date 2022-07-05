//! Universal backend for Wasmer compilers.
//!
//! Given a compiler (such as `CraneliftCompiler` or `LLVMCompiler`)
//! it generates the compiled machine code, and publishes it into
//! memory so it can be used externally.

mod artifact;
mod builder;
mod code_memory;
mod engine;
mod link;
mod unwind;

pub use self::artifact::Artifact;
pub use self::builder::Universal;
pub use self::code_memory::CodeMemory;
pub use self::engine::Engine;
pub use self::link::link_module;
