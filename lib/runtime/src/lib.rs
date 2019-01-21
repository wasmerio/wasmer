#[cfg(test)]
#[macro_use]
extern crate field_offset;

#[macro_use]
mod macros;
#[doc(hidden)]
pub mod backend;
mod backing;
pub mod error;
pub mod export;
pub mod import;
pub mod instance;
pub mod memory;
pub mod module;
mod sig_registry;
pub mod structures;
mod sys;
pub mod table;
pub mod types;
pub mod vm;
#[doc(hidden)]
pub mod vmcalls;

use self::error::CompileResult;
#[doc(inline)]
pub use self::error::Result;
#[doc(inline)]
pub use self::instance::Instance;
#[doc(inline)]
pub use self::module::Module;
use std::rc::Rc;

pub mod prelude {
    pub use crate::export_func;
    pub use crate::import::{ImportObject, Namespace};
    pub use crate::types::{
        FuncIndex, GlobalIndex, ImportedFuncIndex, ImportedGlobalIndex, ImportedMemoryIndex,
        ImportedTableIndex, LocalFuncIndex, LocalGlobalIndex, LocalMemoryIndex, LocalTableIndex,
        MemoryIndex, TableIndex, Type, Value,
    };
    pub use crate::vm;
}

/// Compile a webassembly module using the provided compiler.
pub fn compile(wasm: &[u8], compiler: &dyn backend::Compiler) -> CompileResult<module::Module> {
    let token = backend::Token::generate();
    compiler
        .compile(wasm, token)
        .map(|inner| module::Module::new(Rc::new(inner)))
}
