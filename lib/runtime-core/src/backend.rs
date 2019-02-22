use crate::{
    backing::ImportBacking,
    error::CompileResult,
    error::RuntimeResult,
    module::ModuleInner,
    types::{FuncIndex, LocalFuncIndex, Value},
    vm,
};

use crate::{
    cache::{Artifact, Error as CacheError},
    module::ModuleInfo,
    sys::Memory,
};
use std::ptr::NonNull;

pub mod sys {
    pub use crate::sys::*;
}
pub use crate::sig_registry::SigRegistry;

#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq)]
pub enum Backend {
    Cranelift,
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

pub trait Compiler {
    /// Compiles a `Module` from WebAssembly binary format.
    /// The `CompileToken` parameter ensures that this can only
    /// be called from inside the runtime.
    fn compile(&self, wasm: &[u8], _: Token) -> CompileResult<ModuleInner>;

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
    fn call(
        &self,
        module: &ModuleInner,
        func_index: FuncIndex,
        params: &[Value],
        import_backing: &ImportBacking,
        vmctx: *mut vm::Ctx,
        _: Token,
    ) -> RuntimeResult<Vec<Value>>;

    fn get_early_trapper(&self) -> Box<dyn UserTrapper>;
}

pub trait UserTrapper {
    unsafe fn do_early_trap(&self, msg: String) -> !;
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
