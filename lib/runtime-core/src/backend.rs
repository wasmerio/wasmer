use crate::{
    error::CompileResult,
    module::ModuleInner,
    typed_func::Wasm,
    types::{LocalFuncIndex, SigIndex},
    vm,
};

use crate::{
    cache::{Artifact, Error as CacheError},
    module::ModuleInfo,
    sys::Memory,
};
use std::{any::Any, ptr::NonNull};

use hashbrown::HashMap;

pub mod sys {
    pub use crate::sys::*;
}
pub use crate::sig_registry::SigRegistry;

#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq)]
pub enum Backend {
    Cranelift,
    Singlepass,
    LLVM,
}

/// This type cannot be constructed from
/// outside the runtime crate.
pub struct Token {
    _private: (),
}

impl Token {
    pub(crate) fn generate() -> Self {
        Self { _private: () }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum MemoryBoundCheckMode {
    Default,
    Enable,
    Disable,
}

impl Default for MemoryBoundCheckMode {
    fn default() -> MemoryBoundCheckMode {
        MemoryBoundCheckMode::Default
    }
}

/// Configuration data for the compiler
#[derive(Default)]
pub struct CompilerConfig {
    /// Symbol information generated from emscripten; used for more detailed debug messages
    pub symbol_map: Option<HashMap<u32, String>>,
    pub memory_bound_check_mode: MemoryBoundCheckMode,
    pub enforce_stack_check: bool,
}

pub trait Compiler {
    /// Compiles a `Module` from WebAssembly binary format.
    /// The `CompileToken` parameter ensures that this can only
    /// be called from inside the runtime.
    fn compile(
        &self,
        wasm: &[u8],
        comp_conf: CompilerConfig,
        _: Token,
    ) -> CompileResult<ModuleInner>;

    unsafe fn from_cache(&self, cache: Artifact, _: Token) -> Result<ModuleInner, CacheError>;
}

pub trait RunnableModule: Send + Sync {
    /// This returns a pointer to the function designated by the `local_func_index`
    /// parameter.
    fn get_func(
        &self,
        info: &ModuleInfo,
        local_func_index: LocalFuncIndex,
    ) -> Option<NonNull<vm::Func>>;

    /// A wasm trampoline contains the necesarry data to dynamically call an exported wasm function.
    /// Given a particular signature index, we are returned a trampoline that is matched with that
    /// signature and an invoke function that can call the trampoline.
    fn get_trampoline(&self, info: &ModuleInfo, sig_index: SigIndex) -> Option<Wasm>;

    unsafe fn do_early_trap(&self, data: Box<dyn Any>) -> !;

    /// Returns the machine code associated with this module.
    fn get_code(&self) -> Option<&[u8]> {
        None
    }

    /// Returns the beginning offsets of all functions, including import trampolines.
    fn get_offsets(&self) -> Option<Vec<usize>> {
        None
    }
}

pub trait CacheGen: Send + Sync {
    fn generate_cache(&self) -> Result<(Box<[u8]>, Memory), CacheError>;
}
