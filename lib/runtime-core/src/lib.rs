#[cfg(test)]
#[macro_use]
extern crate field_offset;

#[cfg(feature = "cache")]
#[macro_use]
extern crate serde_derive;

#[macro_use]
mod macros;
#[doc(hidden)]
pub mod backend;
mod backing;
#[cfg(feature = "cache")]
pub mod cache;
pub mod error;
pub mod export;
pub mod global;
pub mod import;
pub mod instance;
pub mod memory;
pub mod module;
mod sig_registry;
pub mod structures;
mod sys;
pub mod table;
mod typed_func;
pub mod types;
pub mod units;
pub mod vm;
#[doc(hidden)]
pub mod vmcalls;

use self::error::CompileResult;
#[doc(inline)]
pub use self::error::Result;
#[doc(inline)]
pub use self::import::IsExport;
#[doc(inline)]
pub use self::instance::Instance;
#[doc(inline)]
pub use self::module::Module;
#[doc(inline)]
pub use self::typed_func::Func;
use std::sync::Arc;

#[cfg(feature = "cache")]
use self::cache::{Cache, Error as CacheError};

pub mod prelude {
    pub use crate::import::{ImportObject, Namespace};
    pub use crate::types::{
        FuncIndex, GlobalIndex, ImportedFuncIndex, ImportedGlobalIndex, ImportedMemoryIndex,
        ImportedTableIndex, LocalFuncIndex, LocalGlobalIndex, LocalMemoryIndex, LocalTableIndex,
        MemoryIndex, TableIndex, Type, Value,
    };
    pub use crate::vm;
    pub use crate::{func, imports};
}

/// Compile a [`Module`] using the provided compiler from
/// WebAssembly binary code. This function is useful if it
/// is necessary to a compile a module before it can be instantiated
/// and must be used if you wish to use a different backend from the default.
///
/// [`Module`]: struct.Module.html
pub fn compile_with(
    wasm: &[u8],
    compiler: &dyn backend::Compiler,
) -> CompileResult<module::Module> {
    let token = backend::Token::generate();
    compiler
        .compile(wasm, token)
        .map(|inner| module::Module::new(Arc::new(inner)))
}

/// Perform validation as defined by the
/// WebAssembly specification. Returns `true` if validation
/// succeeded, `false` if validation failed.
pub fn validate(wasm: &[u8]) -> bool {
    use wasmparser::WasmDecoder;
    let mut parser = wasmparser::ValidatingParser::new(wasm, None);
    loop {
        let state = parser.read();
        match *state {
            wasmparser::ParserState::EndWasm => break true,
            wasmparser::ParserState::Error(_) => break false,
            _ => {}
        }
    }
}

#[cfg(feature = "cache")]
pub fn compile_to_cache_with(
    wasm: &[u8],
    compiler: &dyn backend::Compiler,
) -> CompileResult<Cache> {
    let token = backend::Token::generate();
    let (info, backend_metadata, compiled_code) =
        compiler.compile_to_backend_cache_data(wasm, token)?;

    Ok(Cache::new(wasm, info, backend_metadata, compiled_code))
}

#[cfg(feature = "cache")]
pub unsafe fn load_cache_with(
    cache: Cache,
    compiler: &dyn backend::Compiler,
) -> std::result::Result<module::Module, CacheError> {
    let token = backend::Token::generate();
    compiler
        .from_cache(cache, token)
        .map(|inner| module::Module::new(Arc::new(inner)))
}

/// The current version of this crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
