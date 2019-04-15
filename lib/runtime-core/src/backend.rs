use crate::{
    backing::ImportBacking,
    error::CompileResult,
    error::RuntimeResult,
    module::ModuleInner,
    typed_func::Wasm,
    types::{FuncIndex, LocalFuncIndex, SigIndex, Value},
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

/// Configuration data for the compiler
pub struct CompilerConfig {
    /// Symbol information generated from emscripten; used for more detailed debug messages
    pub symbol_map: Option<HashMap<u32, String>>,
}

impl Default for CompilerConfig {
    fn default() -> CompilerConfig {
        CompilerConfig { symbol_map: None }
    }
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

/// The functionality exposed by this trait is expected to be used
/// for calling functions exported by a webassembly module from
/// host code only.
pub trait ProtectedCaller: Send + Sync {
    /// This calls the exported function designated by `local_func_index`.
    /// Important to note, this supports calling imported functions that are
    /// then exported.
    ///
    /// It's invalid to attempt to call a local function that isn't exported and
    /// the implementation is expected to check for that. The implementation
    /// is also expected to check for correct parameter types and correct
    /// parameter number.
    ///
    /// The `returns` parameter is filled with dummy values when passed in and upon function
    /// return, will be filled with the return values of the wasm function, as long as the
    /// call completed successfully.
    ///
    /// The existance of the Token parameter ensures that this can only be called from
    /// within the runtime crate.
    ///
    /// TODO(lachlan): Now that `get_wasm_trampoline` exists, `ProtectedCaller::call`
    /// can be removed. That should speed up calls a little bit, since sanity checks
    /// would only occur once.
    fn call(
        &self,
        module: &ModuleInner,
        func_index: FuncIndex,
        params: &[Value],
        import_backing: &ImportBacking,
        vmctx: *mut vm::Ctx,
        _: Token,
    ) -> RuntimeResult<Vec<Value>>;

    /// A wasm trampoline contains the necesarry data to dynamically call an exported wasm function.
    /// Given a particular signature index, we are returned a trampoline that is matched with that
    /// signature and an invoke function that can call the trampoline.
    fn get_wasm_trampoline(&self, module: &ModuleInner, sig_index: SigIndex) -> Option<Wasm>;

    fn get_early_trapper(&self) -> Box<dyn UserTrapper>;
}

pub trait UserTrapper {
    unsafe fn do_early_trap(&self, data: Box<dyn Any>) -> !;
}

pub trait FuncResolver: Send + Sync {
    /// This returns a pointer to the function designated by the `local_func_index`
    /// parameter.
    fn get(
        &self,
        module: &ModuleInner,
        local_func_index: LocalFuncIndex,
    ) -> Option<NonNull<vm::Func>>;
}

pub trait CacheGen: Send + Sync {
    fn generate_cache(
        &self,
        module: &ModuleInner,
    ) -> Result<(Box<ModuleInfo>, Box<[u8]>, Memory), CacheError>;
}
