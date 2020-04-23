//! This module defines the parser and translator from wasmparser
//! to a common structure `Module`.
//!
//! It's derived from [cranelift-wasm] but architected for multiple
//! compilers rather than just Cranelift.
//!
//! [cranelift-wasm]: https://crates.io/crates/cranelift-wasm/
mod environ;
mod module;
mod state;
#[macro_use]
mod error;
mod sections;

pub use self::environ::{FunctionBodyData, ModuleEnvironment, ModuleTranslation};
pub use self::error::{to_wasm_error, WasmError, WasmResult};
pub use self::module::translate_module;
pub use self::state::ModuleTranslationState;
