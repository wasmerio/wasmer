//! Wasmer Runtime Core Library
//!
//! The runtime core library provides common data structures which are shared by compiler backends
//! to implement a Web Assembly runtime.
//!
//! The runtime core also provides an API for users who use wasmer as an embedded wasm runtime which
//! allows operations like compiling, instantiating, providing imports, access exports, memories,
//! and tables for example.
//!
//! The runtime core library is recommended to be used by only power users who wish to customize the
//! wasmer runtime.  Most wasmer users should prefer the API which is re-exported by the wasmer
//! runtime library which provides common defaults and a friendly API.
//!

#![deny(
    dead_code,
    missing_docs,
    nonstandard_style,
    unused_imports,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
#![cfg_attr(nightly, feature(unwind_attributes))]
#![doc(html_favicon_url = "https://wasmer.io/static/icons/favicon.ico")]
#![doc(html_logo_url = "https://avatars3.githubusercontent.com/u/44205449?s=200&v=4")]

#[macro_use]
extern crate serde_derive;

#[allow(unused_imports)]
#[macro_use]
extern crate lazy_static;

#[macro_use]
mod macros;
#[doc(hidden)]
pub mod backend;
mod backing;

pub mod cache;
pub mod codegen;
pub mod error;
pub mod export;
pub mod global;
pub mod import;
pub mod instance;
pub mod loader;
pub mod memory;
pub mod module;
pub mod parse;
mod sig_registry;
pub mod structures;
mod sys;
pub mod table;
#[cfg(all(unix, target_arch = "x86_64"))]
pub mod trampoline_x64;
pub mod typed_func;
pub mod types;
pub mod units;
pub mod vm;
#[doc(hidden)]
pub mod vmcalls;
#[cfg(all(unix, target_arch = "x86_64"))]
pub use trampoline_x64 as trampoline;
#[cfg(unix)]
pub mod fault;
pub mod state;
#[cfg(feature = "managed")]
pub mod tiering;

use self::error::CompileResult;
#[doc(inline)]
pub use self::error::Result;
#[doc(inline)]
pub use self::import::IsExport;
#[doc(inline)]
pub use self::instance::{DynFunc, Instance};
#[doc(inline)]
pub use self::module::Module;
#[doc(inline)]
pub use self::typed_func::Func;
use std::sync::Arc;

pub use wasmparser;

use self::cache::{Artifact, Error as CacheError};

pub mod prelude {
    //! The prelude module is a helper module used to bring commonly used runtime core imports into
    //! scope.

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
pub fn compile_with(
    wasm: &[u8],
    compiler: &dyn backend::Compiler,
) -> CompileResult<module::Module> {
    let token = backend::Token::generate();
    compiler
        .compile(wasm, Default::default(), token)
        .map(|mut inner| {
            let inner_info: &mut crate::module::ModuleInfo = &mut inner.info;
            inner_info.import_custom_sections(wasm).unwrap();
            module::Module::new(Arc::new(inner))
        })
}

/// The same as `compile_with` but changes the compiler behavior
/// with the values in the `CompilerConfig`
pub fn compile_with_config(
    wasm: &[u8],
    compiler: &dyn backend::Compiler,
    compiler_config: backend::CompilerConfig,
) -> CompileResult<module::Module> {
    let token = backend::Token::generate();
    compiler
        .compile(wasm, compiler_config, token)
        .map(|inner| module::Module::new(Arc::new(inner)))
}

/// Perform validation as defined by the
/// WebAssembly specification. Returns `true` if validation
/// succeeded, `false` if validation failed.
pub fn validate(wasm: &[u8]) -> bool {
    validate_and_report_errors(wasm).is_ok()
}

/// The same as `validate` but with an Error message on failure
pub fn validate_and_report_errors(wasm: &[u8]) -> ::std::result::Result<(), String> {
    validate_and_report_errors_with_features(wasm, Default::default())
}

/// The same as `validate_and_report_errors` but with a Features.
pub fn validate_and_report_errors_with_features(
    wasm: &[u8],
    features: backend::Features,
) -> ::std::result::Result<(), String> {
    use wasmparser::WasmDecoder;
    let config = wasmparser::ValidatingParserConfig {
        operator_config: wasmparser::OperatorValidatorConfig {
            enable_simd: features.simd,
            enable_bulk_memory: false,
            enable_multi_value: false,
            enable_reference_types: false,
            enable_threads: features.threads,

            #[cfg(feature = "deterministic-execution")]
            deterministic_only: true,
        },
    };
    let mut parser = wasmparser::ValidatingParser::new(wasm, Some(config));
    loop {
        let state = parser.read();
        match *state {
            wasmparser::ParserState::EndWasm => break Ok(()),
            wasmparser::ParserState::Error(e) => break Err(format!("{}", e)),
            _ => {}
        }
    }
}

/// Creates a new module from the given cache `Artifact` for the specified compiler backend
pub unsafe fn load_cache_with(
    cache: Artifact,
    compiler: &dyn backend::Compiler,
) -> std::result::Result<module::Module, CacheError> {
    let token = backend::Token::generate();
    compiler
        .from_cache(cache, token)
        .map(|inner| module::Module::new(Arc::new(inner)))
}

/// The current version of this crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
