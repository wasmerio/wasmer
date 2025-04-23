//! This module mainly outputs the `Compiler` trait that custom
//! compilers will need to implement.

use crate::types::{module::CompileModuleInfo, symbols::SymbolRegistry};
use crate::{
    lib::std::{boxed::Box, sync::Arc},
    translator::ModuleMiddleware,
    types::function::Compilation,
    FunctionBodyData, ModuleTranslationState,
};
use enumset::EnumSet;
use wasmer_types::{
    entity::PrimaryMap,
    error::CompileError,
    target::{CpuFeature, Target, UserCompilerOptimizations},
    Features, LocalFunctionIndex,
};
#[cfg(feature = "translator")]
use wasmparser::{Validator, WasmFeatures};

/// The compiler configuration options.
pub trait CompilerConfig {
    /// Enable Position Independent Code (PIC).
    ///
    /// This is required for shared object generation (Native Engine),
    /// but will make the JIT Engine to fail, since PIC is not yet
    /// supported in the JIT linking phase.
    fn enable_pic(&mut self) {
        // By default we do nothing, each backend will need to customize this
        // in case they do something special for emitting PIC code.
    }

    /// Enable compiler IR verification.
    ///
    /// For compilers capable of doing so, this enables internal consistency
    /// checking.
    fn enable_verifier(&mut self) {
        // By default we do nothing, each backend will need to customize this
        // in case they create an IR that they can verify.
    }

    /// Enable generation of perfmaps to sample the JIT compiled frames.
    fn enable_perfmap(&mut self) {
        // By default we do nothing, each backend will need to customize this
        // in case they create an IR that they can verify.
    }

    /// Enable NaN canonicalization.
    ///
    /// NaN canonicalization is useful when trying to run WebAssembly
    /// deterministically across different architectures.
    fn canonicalize_nans(&mut self, _enable: bool) {
        // By default we do nothing, each backend will need to customize this
        // in case they create an IR that they can verify.
    }

    /// Gets the custom compiler config
    fn compiler(self: Box<Self>) -> Box<dyn Compiler>;

    /// Gets the default features for this compiler in the given target
    fn default_features_for_target(&self, target: &Target) -> Features {
        self.supported_features_for_target(target)
    }

    /// Gets the supported features for this compiler in the given target
    fn supported_features_for_target(&self, _target: &Target) -> Features {
        Features::default()
    }

    /// Pushes a middleware onto the back of the middleware chain.
    fn push_middleware(&mut self, middleware: Arc<dyn ModuleMiddleware>);
}

impl<T> From<T> for Box<dyn CompilerConfig + 'static>
where
    T: CompilerConfig + 'static,
{
    fn from(other: T) -> Self {
        Box::new(other)
    }
}

/// An implementation of a Compiler from parsed WebAssembly module to Compiled native code.
pub trait Compiler: Send + std::fmt::Debug {
    /// Returns a descriptive name for this compiler.
    ///
    /// Note that this is an API breaking change since 3.0
    fn name(&self) -> &str;

    /// Returns the deterministic id of this compiler. Same compilers with different
    /// optimizations map to different deterministic IDs.
    fn deterministic_id(&self) -> String;

    /// Add suggested optimizations to this compiler.
    ///
    /// # Note
    ///
    /// Not every compiler supports every optimization. This function may fail (i.e. not set the
    /// suggested optimizations) silently if the underlying compiler does not support one or
    /// more optimizations.
    fn with_opts(
        &mut self,
        suggested_compiler_opts: &UserCompilerOptimizations,
    ) -> Result<(), CompileError> {
        _ = suggested_compiler_opts;
        Ok(())
    }

    /// Validates a module.
    ///
    /// It returns the a succesful Result in case is valid, `CompileError` in case is not.
    #[cfg(feature = "translator")]
    fn validate_module(&self, features: &Features, data: &[u8]) -> Result<(), CompileError> {
        let mut wasm_features = WasmFeatures::default();
        wasm_features.set(WasmFeatures::BULK_MEMORY, features.bulk_memory);
        wasm_features.set(WasmFeatures::THREADS, features.threads);
        wasm_features.set(WasmFeatures::REFERENCE_TYPES, features.reference_types);
        wasm_features.set(WasmFeatures::MULTI_VALUE, features.multi_value);
        wasm_features.set(WasmFeatures::SIMD, features.simd);
        wasm_features.set(WasmFeatures::TAIL_CALL, features.tail_call);
        wasm_features.set(WasmFeatures::MULTI_MEMORY, features.multi_memory);
        wasm_features.set(WasmFeatures::MEMORY64, features.memory64);
        wasm_features.set(WasmFeatures::EXCEPTIONS, features.exceptions);
        wasm_features.set(WasmFeatures::EXTENDED_CONST, features.extended_const);
        wasm_features.set(WasmFeatures::RELAXED_SIMD, features.relaxed_simd);
        wasm_features.set(WasmFeatures::MUTABLE_GLOBAL, true);
        wasm_features.set(WasmFeatures::SATURATING_FLOAT_TO_INT, true);
        wasm_features.set(WasmFeatures::FLOATS, true);
        wasm_features.set(WasmFeatures::SIGN_EXTENSION, true);
        wasm_features.set(WasmFeatures::GC_TYPES, true);

        // Not supported
        wasm_features.set(WasmFeatures::COMPONENT_MODEL, false);
        wasm_features.set(WasmFeatures::FUNCTION_REFERENCES, false);
        wasm_features.set(WasmFeatures::MEMORY_CONTROL, false);
        wasm_features.set(WasmFeatures::GC, false);
        wasm_features.set(WasmFeatures::COMPONENT_MODEL_VALUES, false);
        wasm_features.set(WasmFeatures::COMPONENT_MODEL_NESTED_NAMES, false);

        let mut validator = Validator::new_with_features(wasm_features);
        validator
            .validate_all(data)
            .map_err(|e| CompileError::Validate(format!("{e}")))?;
        Ok(())
    }

    /// Compiles a parsed module.
    ///
    /// It returns the [`Compilation`] or a [`CompileError`].
    fn compile_module(
        &self,
        target: &Target,
        module: &CompileModuleInfo,
        module_translation: &ModuleTranslationState,
        // The list of function bodies
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
    ) -> Result<Compilation, CompileError>;

    /// Compiles a module into a native object file.
    ///
    /// It returns the bytes as a `&[u8]` or a [`CompileError`].
    fn experimental_native_compile_module(
        &self,
        _target: &Target,
        _module: &CompileModuleInfo,
        _module_translation: &ModuleTranslationState,
        // The list of function bodies
        _function_body_inputs: &PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
        _symbol_registry: &dyn SymbolRegistry,
        // The metadata to inject into the wasmer_metadata section of the object file.
        _wasmer_metadata: &[u8],
    ) -> Option<Result<Vec<u8>, CompileError>> {
        None
    }

    /// Get the middlewares for this compiler
    fn get_middlewares(&self) -> &[Arc<dyn ModuleMiddleware>];

    /// Get the CpuFeatues used by the compiler
    fn get_cpu_features_used(&self, cpu_features: &EnumSet<CpuFeature>) -> EnumSet<CpuFeature> {
        *cpu_features
    }

    /// Get whether `perfmap` is enabled or not.
    fn get_perfmap_enabled(&self) -> bool {
        false
    }
}
