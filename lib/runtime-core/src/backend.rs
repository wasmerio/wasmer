use crate::{
    error::CompileResult,
    module::ModuleInner,
    state::ModuleStateMap,
    typed_func::Wasm,
    types::{LocalFuncIndex, SigIndex},
    vm,
};

use crate::{
    cache::{Artifact, Error as CacheError},
    codegen::BreakpointMap,
    module::ModuleInfo,
    sys::Memory,
};
use std::fmt;
use std::{any::Any, ptr::NonNull};

use std::collections::HashMap;

use rkyv::{
    Archive,
    Serialize as RkyvSerialize,
    Deserialize as RkyvDeserialize,
};

pub mod sys {
    pub use crate::sys::*;
}
pub use crate::sig_registry::SigRegistry;

/// The target architecture for code generation.
#[derive(Copy, Clone, Debug)]
pub enum Architecture {
    /// x86-64.
    X64,

    /// Aarch64 (ARM64).
    Aarch64,
}

/// The type of an inline breakpoint.
#[repr(u8)]
#[derive(Copy, Clone, Debug)]
pub enum InlineBreakpointType {
    /// A middleware invocation breakpoint.
    Middleware,
}

/// Information of an inline breakpoint.
#[derive(Clone, Debug)]
pub struct InlineBreakpoint {
    /// Size in bytes taken by this breakpoint's instruction sequence.
    pub size: usize,

    /// Type of the inline breakpoint.
    pub ty: InlineBreakpointType,
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

/// Controls which experimental features will be enabled.
/// Features usually have a corresponding [WebAssembly proposal][wasm-props].
///
/// [wasm-props]: https://github.com/WebAssembly/proposals
#[derive(Debug, Default)]
pub struct Features {
    /// Whether support for the [SIMD proposal][simd-prop] is enabled.
    ///
    /// [simd-prop]: https://github.com/webassembly/simd
    pub simd: bool,
    /// Whether support for the [threads proposal][threads-prop] is enabled.
    ///
    /// [threads-prop]: https://github.com/webassembly/threads
    pub threads: bool,
}

/// Use this to point to a compiler config struct provided by the backend.
/// The backend struct must support runtime reflection with `Any`, which is any
/// struct that does not contain a non-`'static` reference.
#[derive(Debug)]
pub struct BackendCompilerConfig(pub Box<dyn Any + 'static>);

impl BackendCompilerConfig {
    /// Obtain the backend-specific compiler config struct.
    pub fn get_specific<T: 'static>(&self) -> Option<&T> {
        self.0.downcast_ref::<T>()
    }
}

/// Configuration data for the compiler
#[derive(Debug, Default)]
pub struct CompilerConfig {
    /// Symbol information generated from emscripten; used for more detailed debug messages
    pub symbol_map: Option<HashMap<u32, String>>,

    /// How to make the decision whether to emit bounds checks for memory accesses.
    pub memory_bound_check_mode: MemoryBoundCheckMode,

    /// Whether to generate explicit native stack checks against `stack_lower_bound` in `InternalCtx`.
    ///
    /// Usually it's adequate to use hardware memory protection mechanisms such as `mprotect` on Unix to
    /// prevent stack overflow. But for low-level environments, e.g. the kernel, faults are generally
    /// not expected and relying on hardware memory protection would add too much complexity.
    pub enforce_stack_check: bool,

    /// Whether to enable state tracking. Necessary for managed mode.
    pub track_state: bool,

    /// Whether to enable full preemption checkpoint generation.
    ///
    /// This inserts checkpoints at critical locations such as loop backedges and function calls,
    /// allowing preemptive unwinding/task switching.
    ///
    /// When enabled there can be a small amount of runtime performance overhead.
    pub full_preemption: bool,

    pub features: Features,

    // Target info. Presently only supported by LLVM.
    pub triple: Option<String>,
    pub cpu_name: Option<String>,
    pub cpu_features: Option<String>,

    pub backend_specific_config: Option<BackendCompilerConfig>,

    pub generate_debug_info: bool,
}

impl CompilerConfig {
    /// Use this to check if we should be generating debug information.
    /// This function takes into account the features that runtime-core was
    /// compiled with in addition to the value of the `generate_debug_info` field.
    pub(crate) fn should_generate_debug_info(&self) -> bool {
        cfg!(feature = "generate-debug-information") && self.generate_debug_info
    }
}

/// An exception table for a `RunnableModule`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct ExceptionTable {
    /// Mappings from offsets in generated machine code to the corresponding exception code.
    pub offset_to_code: HashMap<usize, ExceptionCode>,
}

impl ExceptionTable {
    pub fn new() -> Self {
        Self::default()
    }
}

/// The code of an exception.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub enum ExceptionCode {
    /// An `unreachable` opcode was executed.
    Unreachable = 0,
    /// Call indirect incorrect signature trap.
    IncorrectCallIndirectSignature = 1,
    /// Memory out of bounds trap.
    MemoryOutOfBounds = 2,
    /// Call indirect out of bounds trap.
    CallIndirectOOB = 3,
    /// An arithmetic exception, e.g. divided by zero.
    IllegalArithmetic = 4,
    /// Misaligned atomic access trap.
    MisalignedAtomicAccess = 5,
}

impl fmt::Display for ExceptionCode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                ExceptionCode::Unreachable => "unreachable",
                ExceptionCode::IncorrectCallIndirectSignature => {
                    "incorrect `call_indirect` signature"
                }
                ExceptionCode::MemoryOutOfBounds => "memory out-of-bounds access",
                ExceptionCode::CallIndirectOOB => "`call_indirect` out-of-bounds",
                ExceptionCode::IllegalArithmetic => "illegal arithmetic operation",
                ExceptionCode::MisalignedAtomicAccess => "misaligned atomic access",
            }
        )
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

pub trait RunnableModule: Send + Sync {
    /// This returns a pointer to the function designated by the `local_func_index`
    /// parameter.
    fn get_func(
        &self,
        info: &ModuleInfo,
        local_func_index: LocalFuncIndex,
    ) -> Option<NonNull<vm::Func>>;

    fn get_module_state_map(&self) -> Option<ModuleStateMap> {
        None
    }

    fn get_breakpoints(&self) -> Option<BreakpointMap> {
        None
    }

    fn get_exception_table(&self) -> Option<&ExceptionTable> {
        None
    }

    unsafe fn patch_local_function(&self, _idx: usize, _target_address: usize) -> bool {
        false
    }

    /// A wasm trampoline contains the necessary data to dynamically call an exported wasm function.
    /// Given a particular signature index, we return a trampoline that is matched with that
    /// signature and an invoke function that can call the trampoline.
    fn get_trampoline(&self, info: &ModuleInfo, sig_index: SigIndex) -> Option<Wasm>;

    /// Trap an error.
    unsafe fn do_early_trap(&self, data: Box<dyn Any + Send>) -> !;

    /// Returns the machine code associated with this module.
    fn get_code(&self) -> Option<&[u8]> {
        None
    }

    /// Returns the beginning offsets of all functions, including import trampolines.
    fn get_offsets(&self) -> Option<Vec<usize>> {
        None
    }

    /// Returns the beginning offsets of all local functions.
    fn get_local_function_offsets(&self) -> Option<Vec<usize>> {
        None
    }

    /// Returns the inline breakpoint size corresponding to an Architecture (None in case is not implemented)
    fn get_inline_breakpoint_size(&self, _arch: Architecture) -> Option<usize> {
        None
    }

    /// Attempts to read an inline breakpoint from the code.
    ///
    /// Inline breakpoints are detected by special instruction sequences that never
    /// appear in valid code.
    fn read_inline_breakpoint(
        &self,
        _arch: Architecture,
        _code: &[u8],
    ) -> Option<InlineBreakpoint> {
        None
    }
}

pub trait CacheGen: Send + Sync {
    fn generate_cache(&self) -> Result<(Box<[u8]>, Memory), CacheError>;
}
