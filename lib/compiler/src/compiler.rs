//! This module mainly outputs the `Compiler` trait that custom
//! compilers will need to implement.

use std::cmp::Reverse;
use std::sync::Mutex;

use crate::progress::ProgressContext;
use crate::types::{module::CompileModuleInfo, symbols::SymbolRegistry};
use crate::{
    FunctionBodyData, ModuleTranslationState,
    lib::std::{boxed::Box, sync::Arc},
    translator::ModuleMiddleware,
    types::function::Compilation,
};
use crossbeam_channel::unbounded;
use enumset::EnumSet;
use itertools::Itertools;
use wasmer_types::{
    CompilationProgressCallback, Features, LocalFunctionIndex,
    entity::PrimaryMap,
    error::CompileError,
    target::{CpuFeature, Target, UserCompilerOptimizations},
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
        wasm_features.set(WasmFeatures::CM_VALUES, false);
        wasm_features.set(WasmFeatures::CM_NESTED_NAMES, false);

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
        progress_callback: Option<&CompilationProgressCallback>,
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

/// A bucket containing a group of functions and their total size, used to balance compilation units for parallel compilation.
pub struct FunctionBucket<'a> {
    functions: Vec<(LocalFunctionIndex, &'a FunctionBodyData<'a>)>,
    /// IR size of the bucket (in bytes).
    pub size: usize,
}

impl<'a> FunctionBucket<'a> {
    /// Creates a new, empty `FunctionBucket`.
    pub fn new() -> Self {
        Self {
            functions: Vec::new(),
            size: 0,
        }
    }
}

/// Build buckets sized by function length to keep compilation units balanced for parallel compilation.
pub fn build_function_buckets<'a>(
    function_body_inputs: &'a PrimaryMap<LocalFunctionIndex, FunctionBodyData<'a>>,
    bucket_threshold_size: u64,
) -> Vec<FunctionBucket<'a>> {
    let mut function_bodies = function_body_inputs
        .iter()
        .sorted_by_key(|(id, body)| Reverse((body.data.len(), id.as_u32())))
        .collect_vec();

    let mut buckets = Vec::new();

    while !function_bodies.is_empty() {
        let mut next_function_body = Vec::with_capacity(function_bodies.len());
        let mut bucket = FunctionBucket::new();

        for (fn_index, fn_body) in function_bodies.into_iter() {
            if bucket.size + fn_body.data.len() <= bucket_threshold_size as usize
                // Huge functions must fit into a bucket!
                || bucket.size == 0
            {
                bucket.size += fn_body.data.len();
                bucket.functions.push((fn_index, fn_body));
            } else {
                next_function_body.push((fn_index, fn_body));
            }
        }

        function_bodies = next_function_body;
        buckets.push(bucket);
    }

    buckets
}

/// Represents a function that has been compiled by the backend compiler.
pub trait CompiledFunction {}

/// Translates a function from its input representation to a compiled form.
pub trait FuncTranslator {}

use perfetto_recorder::ThreadTraceData;
use perfetto_recorder::TraceBuilder;
use perfetto_recorder::scope;

/// Compile function buckets largest-first via the channel (instead of Rayon's par_iter).
#[allow(clippy::too_many_arguments)]
pub fn translate_function_buckets<'a, C, T, F, G>(
    pool: &rayon::ThreadPool,
    func_translator_builder: F,
    translate_fn: G,
    progress: Option<ProgressContext>,
    buckets: &[FunctionBucket<'a>],
) -> Result<Vec<C>, CompileError>
where
    T: FuncTranslator,
    C: CompiledFunction + Send + Sync,
    F: Fn() -> T + Send + Sync + Copy,
    G: Fn(&mut T, &LocalFunctionIndex, &FunctionBodyData) -> Result<C, CompileError>
        + Send
        + Sync
        + Copy,
{
    let progress = progress.as_ref();
    perfetto_recorder::start().unwrap();

    let mut trace = TraceBuilder::new().unwrap();

    // Record data from the main thread.
    trace.process_thread_data(&ThreadTraceData::take_current_thread());

    let trace = Arc::new(Mutex::new(trace));

    let functions = pool.install(|| {
        let (bucket_tx, bucket_rx) = unbounded::<&FunctionBucket<'a>>();
        for bucket in buckets {
            bucket_tx.send(bucket).map_err(|e| {
                CompileError::Resource(format!("cannot allocate crossbeam channel item: {e}"))
            })?;
        }
        drop(bucket_tx);

        let (result_tx, result_rx) =
            unbounded::<Result<Vec<(LocalFunctionIndex, C)>, CompileError>>();

        pool.scope(|s| {
            let worker_count = pool.current_num_threads().max(1);
            for _ in 0..worker_count {
                let bucket_rx = bucket_rx.clone();
                let result_tx = result_tx.clone();
                s.spawn(move |_| {
                    let mut func_translator = func_translator_builder();

                    while let Ok(bucket) = bucket_rx.recv() {
                        scope!(
                            "translate bucket",
                            bucket_size = bucket.size,
                            functions = bucket.functions.len()
                        );
                        let bucket_result = (|| {
                            let mut translated_functions = Vec::new();
                            for (i, input) in bucket.functions.iter() {
                                scope!("translate function", body_size = input.data.len());
                                let translated = translate_fn(&mut func_translator, i, input)?;
                                if let Some(progress) = progress {
                                    progress.notify_steps(input.data.len() as u64)?;
                                }
                                translated_functions.push((*i, translated));
                            }
                            Ok(translated_functions)
                        })();

                        if result_tx.send(bucket_result).is_err() {
                            break;
                        }
                    }
                });
            }
        });

        drop(result_tx);
        let mut functions = Vec::with_capacity(buckets.iter().map(|b| b.functions.len()).sum());
        for _ in 0..buckets.len() {
            match result_rx.recv().map_err(|e| {
                CompileError::Resource(format!("cannot allocate crossbeam channel item: {e}"))
            })? {
                Ok(bucket_functions) => functions.extend(bucket_functions),
                Err(err) => return Err(err),
            }
        }

        let trace = trace.clone();
        pool.spawn_broadcast(move |_| {
            let thread_trace = ThreadTraceData::take_current_thread();
            trace.lock().unwrap().process_thread_data(&thread_trace);
        });

        Ok(functions)
    })?;

    let trace_file = "trace.ptrace";
    eprintln!("Saving to: {trace_file}");
    trace.lock().unwrap().write_to_file(&trace_file).unwrap();

    Ok(functions
        .into_iter()
        .sorted_by_key(|x| x.0)
        .map(|(_, body)| body)
        .collect_vec())
}

/// Byte size threshold for a function that is considered large.
pub const WASM_LARGE_FUNCTION_THRESHOLD: u64 = 100_000;

/// Estimated byte size of a trampoline (used for progress bar reporting).
pub const WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE: u64 = 1_000;
