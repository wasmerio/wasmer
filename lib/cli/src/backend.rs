//! Common module with common used structures across different
//! commands.

// NOTE: A lot of this code depends on feature flags.
// To not go crazy with annotations, some lints are disabled for the whole
// module.
#![allow(dead_code, unused_imports, unused_variables)]

use std::num::NonZero;
use std::string::ToString;
use std::sync::Arc;
use std::{path::PathBuf, str::FromStr};

use anyhow::{bail, Result};
#[cfg(feature = "sys")]
use wasmer::sys::*;
use wasmer::*;
use wasmer_types::{target::Target, Features};

#[cfg(feature = "compiler")]
use wasmer_compiler::CompilerConfig;

use wasmer::Engine;

#[derive(Debug, clap::Parser, Clone, Default)]
/// The WebAssembly features that can be passed through the
/// Command Line args.
pub struct WasmFeatures {
    /// Enable support for the SIMD proposal.
    #[clap(long = "enable-simd")]
    pub simd: bool,

    /// Disable support for the threads proposal.
    #[clap(long = "disable-threads")]
    pub disable_threads: bool,

    /// Deprecated, threads are enabled by default.
    #[clap(long = "enable-threads")]
    pub _threads: bool,

    /// Enable support for the reference types proposal.
    #[clap(long = "enable-reference-types")]
    pub reference_types: bool,

    /// Enable support for the multi value proposal.
    #[clap(long = "enable-multi-value")]
    pub multi_value: bool,

    /// Enable support for the bulk memory proposal.
    #[clap(long = "enable-bulk-memory")]
    pub bulk_memory: bool,

    /// Enable support for the tail call proposal.
    #[clap(long = "enable-tail-call")]
    pub tail_call: bool,

    /// Enable support for the module linking proposal.
    #[clap(long = "enable-module-linking")]
    pub module_linking: bool,

    /// Enable support for the multi memory proposal.
    #[clap(long = "enable-multi-memory")]
    pub multi_memory: bool,

    /// Enable support for the memory64 proposal.
    #[clap(long = "enable-memory64")]
    pub memory64: bool,

    /// Enable support for the exceptions proposal.
    #[clap(long = "enable-exceptions")]
    pub exceptions: bool,

    /// Enable support for the relaxed SIMD proposal.
    #[clap(long = "enable-relaxed-simd")]
    pub relaxed_simd: bool,

    /// Enable support for the extended constant expressions proposal.
    #[clap(long = "enable-extended-const")]
    pub extended_const: bool,

    /// Enable support for all pre-standard proposals.
    #[clap(long = "enable-all")]
    pub all: bool,
}

#[derive(Debug, Clone, clap::Parser, Default)]
/// The compiler options
pub struct RuntimeOptions {
    /// Use Singlepass compiler.
    #[cfg(feature = "singlepass")]
    #[clap(long, conflicts_with_all = &Vec::<&str>::from_iter([
        #[cfg(feature = "llvm")]
        "llvm", 
        #[cfg(feature = "v8")]
        "v8", 
        #[cfg(feature = "cranelift")]
        "cranelift", 
        #[cfg(feature = "wamr")]
        "wamr", 
        #[cfg(feature = "wasmi")]
        "wasmi"
    ]))]
    singlepass: bool,

    /// Use Cranelift compiler.
    #[cfg(feature = "cranelift")]
    #[clap(long, conflicts_with_all = &Vec::<&str>::from_iter([
        #[cfg(feature = "llvm")]
        "llvm", 
        #[cfg(feature = "v8")]
        "v8", 
        #[cfg(feature = "singlepass")]
        "singlepass", 
        #[cfg(feature = "wamr")]
        "wamr", 
        #[cfg(feature = "wasmi")]
        "wasmi"
    ]))]
    cranelift: bool,

    /// Use LLVM compiler.
    #[cfg(feature = "llvm")]
    #[clap(long, conflicts_with_all = &Vec::<&str>::from_iter([
        #[cfg(feature = "cranelift")]
        "cranelift", 
        #[cfg(feature = "v8")]
        "v8", 
        #[cfg(feature = "singlepass")]
        "singlepass", 
        #[cfg(feature = "wamr")]
        "wamr", 
        #[cfg(feature = "wasmi")]
        "wasmi"
    ]))]
    llvm: bool,

    /// Use the V8 runtime.
    #[cfg(feature = "v8")]
    #[clap(long, conflicts_with_all = &Vec::<&str>::from_iter([
        #[cfg(feature = "cranelift")]
        "cranelift", 
        #[cfg(feature = "llvm")]
        "llvm", 
        #[cfg(feature = "singlepass")]
        "singlepass", 
        #[cfg(feature = "wamr")]
        "wamr", 
        #[cfg(feature = "wasmi")]
        "wasmi"
    ]))]
    v8: bool,

    /// Use WAMR.
    #[cfg(feature = "wamr")]
    #[clap(long, conflicts_with_all = &Vec::<&str>::from_iter([
        #[cfg(feature = "cranelift")]
        "cranelift", 
        #[cfg(feature = "llvm")]
        "llvm", 
        #[cfg(feature = "singlepass")]
        "singlepass", 
        #[cfg(feature = "v8")]
        "v8", 
        #[cfg(feature = "wasmi")]
        "wasmi"
    ]))]
    wamr: bool,

    /// Use the wasmi runtime.
    #[cfg(feature = "wasmi")]
    #[clap(long, conflicts_with_all = &Vec::<&str>::from_iter([
        #[cfg(feature = "cranelift")]
        "cranelift", 
        #[cfg(feature = "llvm")]
        "llvm", 
        #[cfg(feature = "singlepass")]
        "singlepass", 
        #[cfg(feature = "v8")]
        "v8", 
        #[cfg(feature = "wamr")]
        "wamr"
    ]))]
    wasmi: bool,

    /// Enable compiler internal verification.
    ///
    /// Available for cranelift, LLVM and singlepass.
    #[clap(long)]
    enable_verifier: bool,

    /// Enable a profiler.
    ///
    /// Available for cranelift, LLVM and singlepass.
    #[clap(long, value_enum)]
    profiler: Option<Profiler>,

    /// LLVM debug directory, where IR and object files will be written to.
    ///
    /// Only available for the LLVM compiler.
    #[clap(long)]
    llvm_debug_dir: Option<PathBuf>,

    /// Only available for the LLVM compiler. Enable the "pass-params" optimization, where the first (#0)
    /// global and the first (#0) memory passed between guest functions as explicit parameters.
    #[clap(long)]
    enable_pass_params_opt: bool,

    /// Only available for the LLVM compiler. Sets the number of threads used to compile the
    /// input module(s).
    #[clap(long)]
    llvm_num_threads: Option<NonZero<usize>>,

    #[clap(flatten)]
    features: WasmFeatures,
}

#[derive(Clone, Debug)]
pub enum Profiler {
    /// Perfmap-based profilers.
    Perfmap,
}

impl FromStr for Profiler {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "perfmap" => Ok(Self::Perfmap),
            _ => Err(anyhow::anyhow!("Unrecognized profiler: {s}")),
        }
    }
}

impl RuntimeOptions {
    pub fn get_available_backends(&self) -> Result<Vec<BackendType>> {
        // If a specific backend is explicitly requested, use it
        #[cfg(feature = "cranelift")]
        {
            if self.cranelift {
                return Ok(vec![BackendType::Cranelift]);
            }
        }

        #[cfg(feature = "llvm")]
        {
            if self.llvm {
                return Ok(vec![BackendType::LLVM]);
            }
        }

        #[cfg(feature = "singlepass")]
        {
            if self.singlepass {
                return Ok(vec![BackendType::Singlepass]);
            }
        }

        #[cfg(feature = "wamr")]
        {
            if self.wamr {
                return Ok(vec![BackendType::Wamr]);
            }
        }

        #[cfg(feature = "v8")]
        {
            if self.v8 {
                return Ok(vec![BackendType::V8]);
            }
        }

        #[cfg(feature = "wasmi")]
        {
            if self.wasmi {
                return Ok(vec![BackendType::Wasmi]);
            }
        }

        Ok(BackendType::enabled())
    }

    /// Filter enabled backends based on required WebAssembly features
    pub fn filter_backends_by_features(
        backends: Vec<BackendType>,
        required_features: &Features,
        target: &Target,
    ) -> Vec<BackendType> {
        backends
            .into_iter()
            .filter(|backend| backend.supports_features(required_features, target))
            .collect()
    }

    pub fn get_store(&self) -> Result<Store> {
        let engine = self.get_engine(&Target::default())?;
        Ok(Store::new(engine))
    }

    pub fn get_engine(&self, target: &Target) -> Result<Engine> {
        let backends = self.get_available_backends()?;
        let backend = backends.first().unwrap();
        let backend_kind = wasmer::BackendKind::from(backend);
        let required_features = wasmer::Engine::default_features_for_backend(&backend_kind, target);
        backend.get_engine(target, &required_features, self)
    }

    pub fn get_engine_for_module(&self, module_contents: &[u8], target: &Target) -> Result<Engine> {
        let required_features = self
            .detect_features_from_wasm(module_contents)
            .unwrap_or_default();

        self.get_engine_for_features(&required_features, target)
    }

    pub fn get_engine_for_features(
        &self,
        required_features: &Features,
        target: &Target,
    ) -> Result<Engine> {
        let backends = self.get_available_backends()?;
        let filtered_backends =
            Self::filter_backends_by_features(backends.clone(), required_features, target);

        if filtered_backends.is_empty() {
            let enabled_backends = BackendType::enabled();
            if backends.len() == 1 && enabled_backends.len() > 1 {
                // If the user has chosen an specific backend, we can suggest to use another one
                let filtered_backends =
                    Self::filter_backends_by_features(enabled_backends, required_features, target);
                let extra_text: String = if !filtered_backends.is_empty() {
                    format!(". You can use --{} instead", filtered_backends[0])
                } else {
                    "".to_string()
                };
                bail!(
                    "The {} backend does not support the required features for the Wasm module{}",
                    backends[0],
                    extra_text
                );
            } else {
                bail!("No backends support the required features for the Wasm module. Feel free to open an issue at https://github.com/wasmerio/wasmer/issues");
            }
        }
        filtered_backends
            .first()
            .unwrap()
            .get_engine(target, required_features, self)
    }

    #[cfg(feature = "compiler")]
    /// Get the enaled Wasm features.
    pub fn get_features(&self, features: &Features) -> Result<Features> {
        let mut result = features.clone();
        if !self.features.disable_threads || self.features.all {
            result.threads(true);
        }
        if self.features.disable_threads && !self.features.all {
            result.threads(false);
        }
        if self.features.multi_value || self.features.all {
            result.multi_value(true);
        }
        if self.features.simd || self.features.all {
            result.simd(true);
        }
        if self.features.bulk_memory || self.features.all {
            result.bulk_memory(true);
        }
        if self.features.reference_types || self.features.all {
            result.reference_types(true);
        }
        Ok(result)
    }

    #[cfg(feature = "compiler")]
    /// Get a copy of the default features with user-configured options
    pub fn get_configured_features(&self) -> Result<Features> {
        let features = Features::default();
        self.get_features(&features)
    }

    /// Detect features from a WebAssembly module binary.
    pub fn detect_features_from_wasm(
        &self,
        wasm_bytes: &[u8],
    ) -> Result<Features, wasmparser::BinaryReaderError> {
        let mut features = Features::detect_from_wasm(wasm_bytes)?;

        // Merge with user-configured features
        if !self.features.disable_threads || self.features.all {
            features.threads(true);
        }
        if self.features.reference_types || self.features.all {
            features.reference_types(true);
        }
        if self.features.simd || self.features.all {
            features.simd(true);
        }
        if self.features.bulk_memory || self.features.all {
            features.bulk_memory(true);
        }
        if self.features.multi_value || self.features.all {
            features.multi_value(true);
        }
        if self.features.tail_call || self.features.all {
            features.tail_call(true);
        }
        if self.features.module_linking || self.features.all {
            features.module_linking(true);
        }
        if self.features.multi_memory || self.features.all {
            features.multi_memory(true);
        }
        if self.features.memory64 || self.features.all {
            features.memory64(true);
        }
        if self.features.exceptions || self.features.all {
            features.exceptions(true);
        }

        Ok(features)
    }

    #[cfg(feature = "compiler")]
    pub fn get_sys_compiler_engine_for_target(
        &self,
        target: Target,
    ) -> std::result::Result<Engine, anyhow::Error> {
        let backends = self.get_available_backends()?;
        let compiler_config = self.get_sys_compiler_config(backends.first().unwrap())?;
        let default_features = compiler_config.default_features_for_target(&target);
        let features = self.get_features(&default_features)?;
        Ok(wasmer_compiler::EngineBuilder::new(compiler_config)
            .set_features(Some(features))
            .set_target(Some(target))
            .engine()
            .into())
    }

    #[allow(unused_variables)]
    #[cfg(feature = "compiler")]
    pub(crate) fn get_sys_compiler_config(
        &self,
        rt: &BackendType,
    ) -> Result<Box<dyn CompilerConfig>> {
        let compiler_config: Box<dyn CompilerConfig> = match rt {
            BackendType::Headless => bail!("The headless engine can't be chosen"),
            #[cfg(feature = "singlepass")]
            BackendType::Singlepass => {
                let mut config = wasmer_compiler_singlepass::Singlepass::new();
                if self.enable_verifier {
                    config.enable_verifier();
                }
                if let Some(p) = &self.profiler {
                    match p {
                        Profiler::Perfmap => config.enable_perfmap(),
                    }
                }

                Box::new(config)
            }
            #[cfg(feature = "cranelift")]
            BackendType::Cranelift => {
                let mut config = wasmer_compiler_cranelift::Cranelift::new();
                if self.enable_verifier {
                    config.enable_verifier();
                }
                if let Some(p) = &self.profiler {
                    match p {
                        Profiler::Perfmap => config.enable_perfmap(),
                    }
                }
                Box::new(config)
            }
            #[cfg(feature = "llvm")]
            BackendType::LLVM => {
                use std::{fmt, fs::File, io::Write};

                use wasmer_compiler_llvm::{
                    CompiledKind, InkwellMemoryBuffer, InkwellModule, LLVMCallbacks, LLVM,
                };
                use wasmer_types::entity::EntityRef;
                let mut config = LLVM::new();

                if self.enable_pass_params_opt {
                    config.enable_pass_params_opt();
                }

                if let Some(num_threads) = self.llvm_num_threads {
                    config.num_threads(num_threads);
                }

                struct Callbacks {
                    debug_dir: PathBuf,
                }
                impl Callbacks {
                    fn new(debug_dir: PathBuf) -> Result<Self> {
                        // Create the debug dir in case it doesn't exist
                        std::fs::create_dir_all(&debug_dir)?;
                        Ok(Self { debug_dir })
                    }
                }
                // Converts a kind into a filename, that we will use to dump
                // the contents of the IR object file to.
                fn types_to_signature(types: &[Type]) -> String {
                    types
                        .iter()
                        .map(|ty| match ty {
                            Type::I32 => "i".to_string(),
                            Type::I64 => "I".to_string(),
                            Type::F32 => "f".to_string(),
                            Type::F64 => "F".to_string(),
                            Type::V128 => "v".to_string(),
                            Type::ExternRef => "e".to_string(),
                            Type::FuncRef => "r".to_string(),
                            Type::ExceptionRef => "x".to_string(),
                        })
                        .collect::<Vec<_>>()
                        .join("")
                }
                // Converts a kind into a filename, that we will use to dump
                // the contents of the IR object file to.
                fn function_kind_to_filename(kind: &CompiledKind) -> String {
                    match kind {
                        CompiledKind::Local(local_index) => {
                            format!("function_{}", local_index.index())
                        }
                        CompiledKind::FunctionCallTrampoline(func_type) => format!(
                            "trampoline_call_{}_{}",
                            types_to_signature(func_type.params()),
                            types_to_signature(func_type.results())
                        ),
                        CompiledKind::DynamicFunctionTrampoline(func_type) => format!(
                            "trampoline_dynamic_{}_{}",
                            types_to_signature(func_type.params()),
                            types_to_signature(func_type.results())
                        ),
                        CompiledKind::Module => "module".into(),
                    }
                }
                impl LLVMCallbacks for Callbacks {
                    fn preopt_ir(&self, kind: &CompiledKind, module: &InkwellModule) {
                        let mut path = self.debug_dir.clone();
                        path.push(format!("{}.preopt.ll", function_kind_to_filename(kind)));
                        module
                            .print_to_file(&path)
                            .expect("Error while dumping pre optimized LLVM IR");
                    }
                    fn postopt_ir(&self, kind: &CompiledKind, module: &InkwellModule) {
                        let mut path = self.debug_dir.clone();
                        path.push(format!("{}.postopt.ll", function_kind_to_filename(kind)));
                        module
                            .print_to_file(&path)
                            .expect("Error while dumping post optimized LLVM IR");
                    }
                    fn obj_memory_buffer(
                        &self,
                        kind: &CompiledKind,
                        memory_buffer: &InkwellMemoryBuffer,
                    ) {
                        let mut path = self.debug_dir.clone();
                        path.push(format!("{}.o", function_kind_to_filename(kind)));
                        let mem_buf_slice = memory_buffer.as_slice();
                        let mut file = File::create(path)
                            .expect("Error while creating debug object file from LLVM IR");
                        let mut pos = 0;
                        while pos < mem_buf_slice.len() {
                            pos += file.write(&mem_buf_slice[pos..]).unwrap();
                        }
                    }
                    fn asm_memory_buffer(
                        &self,
                        kind: &CompiledKind,
                        asm_memory_buffer: &InkwellMemoryBuffer,
                    ) {
                        let mut path = self.debug_dir.clone();
                        path.push(format!("{}.s", function_kind_to_filename(kind)));
                        let mem_buf_slice = asm_memory_buffer.as_slice();
                        let mut file = File::create(path)
                            .expect("Error while creating debug object file from LLVM IR");
                        let mut pos = 0;
                        while pos < mem_buf_slice.len() {
                            pos += file.write(&mem_buf_slice[pos..]).unwrap();
                        }
                    }
                }

                impl fmt::Debug for Callbacks {
                    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                        write!(f, "LLVMCallbacks")
                    }
                }

                if let Some(ref llvm_debug_dir) = self.llvm_debug_dir {
                    config.callbacks(Some(Arc::new(Callbacks::new(llvm_debug_dir.clone())?)));
                }
                if self.enable_verifier {
                    config.enable_verifier();
                }
                if let Some(p) = &self.profiler {
                    match p {
                        Profiler::Perfmap => config.enable_perfmap(),
                    }
                }

                Box::new(config)
            }
            BackendType::V8 | BackendType::Wamr | BackendType::Wasmi => unreachable!(),
            #[cfg(not(all(feature = "singlepass", feature = "cranelift", feature = "llvm")))]
            compiler => {
                bail!(
                    "The `{}` compiler is not included in this binary.",
                    compiler.to_string()
                )
            }
        };

        #[allow(unreachable_code)]
        Ok(compiler_config)
    }
}

/// The compiler used for the store
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[allow(clippy::upper_case_acronyms, dead_code)]
pub enum BackendType {
    /// Singlepass compiler
    Singlepass,

    /// Cranelift compiler
    Cranelift,

    /// LLVM compiler
    LLVM,

    /// V8 runtime
    V8,

    /// Wamr runtime
    Wamr,

    /// Wasmi runtime
    Wasmi,

    /// Headless compiler
    #[allow(dead_code)]
    Headless,
}

impl BackendType {
    /// Return all enabled compilers
    pub fn enabled() -> Vec<Self> {
        vec![
            #[cfg(feature = "cranelift")]
            Self::Cranelift,
            #[cfg(feature = "llvm")]
            Self::LLVM,
            #[cfg(feature = "singlepass")]
            Self::Singlepass,
            #[cfg(feature = "v8")]
            Self::V8,
            #[cfg(feature = "wamr")]
            Self::Wamr,
            #[cfg(feature = "wasmi")]
            Self::Wasmi,
        ]
    }

    /// Get an engine for this backend type
    pub fn get_engine(
        &self,
        target: &Target,
        features: &Features,
        runtime_opts: &RuntimeOptions,
    ) -> Result<Engine> {
        match self {
            #[cfg(feature = "singlepass")]
            Self::Singlepass => {
                let mut config = wasmer_compiler_singlepass::Singlepass::new();
                if runtime_opts.enable_verifier {
                    config.enable_verifier();
                }
                if let Some(p) = &runtime_opts.profiler {
                    match p {
                        Profiler::Perfmap => config.enable_perfmap(),
                    }
                }
                let engine = wasmer_compiler::EngineBuilder::new(config)
                    .set_features(Some(features.clone()))
                    .set_target(Some(target.clone()))
                    .engine()
                    .into();
                Ok(engine)
            }
            #[cfg(feature = "cranelift")]
            Self::Cranelift => {
                let mut config = wasmer_compiler_cranelift::Cranelift::new();
                if runtime_opts.enable_verifier {
                    config.enable_verifier();
                }
                if let Some(p) = &runtime_opts.profiler {
                    match p {
                        Profiler::Perfmap => config.enable_perfmap(),
                    }
                }
                let engine = wasmer_compiler::EngineBuilder::new(config)
                    .set_features(Some(features.clone()))
                    .set_target(Some(target.clone()))
                    .engine()
                    .into();
                Ok(engine)
            }
            #[cfg(feature = "llvm")]
            Self::LLVM => {
                use std::{fmt, fs::File, io::Write};

                use wasmer_compiler_llvm::{
                    CompiledKind, InkwellMemoryBuffer, InkwellModule, LLVMCallbacks, LLVM,
                };
                use wasmer_types::entity::EntityRef;

                let mut config = wasmer_compiler_llvm::LLVM::new();

                struct Callbacks {
                    debug_dir: PathBuf,
                }
                impl Callbacks {
                    fn new(debug_dir: PathBuf) -> Result<Self> {
                        // Create the debug dir in case it doesn't exist
                        std::fs::create_dir_all(&debug_dir)?;
                        Ok(Self { debug_dir })
                    }
                }
                // Converts a kind into a filename, that we will use to dump
                // the contents of the IR object file to.
                fn types_to_signature(types: &[Type]) -> String {
                    types
                        .iter()
                        .map(|ty| match ty {
                            Type::I32 => "i".to_string(),
                            Type::I64 => "I".to_string(),
                            Type::F32 => "f".to_string(),
                            Type::F64 => "F".to_string(),
                            Type::V128 => "v".to_string(),
                            Type::ExternRef => "e".to_string(),
                            Type::FuncRef => "r".to_string(),
                            Type::ExceptionRef => "x".to_string(),
                        })
                        .collect::<Vec<_>>()
                        .join("")
                }
                // Converts a kind into a filename, that we will use to dump
                // the contents of the IR object file to.
                fn function_kind_to_filename(kind: &CompiledKind) -> String {
                    match kind {
                        CompiledKind::Local(local_index) => {
                            format!("function_{}", local_index.index())
                        }
                        CompiledKind::FunctionCallTrampoline(func_type) => format!(
                            "trampoline_call_{}_{}",
                            types_to_signature(func_type.params()),
                            types_to_signature(func_type.results())
                        ),
                        CompiledKind::DynamicFunctionTrampoline(func_type) => format!(
                            "trampoline_dynamic_{}_{}",
                            types_to_signature(func_type.params()),
                            types_to_signature(func_type.results())
                        ),
                        CompiledKind::Module => "module".into(),
                    }
                }
                impl LLVMCallbacks for Callbacks {
                    fn preopt_ir(&self, kind: &CompiledKind, module: &InkwellModule) {
                        let mut path = self.debug_dir.clone();
                        path.push(format!("{}.preopt.ll", function_kind_to_filename(kind)));
                        module
                            .print_to_file(&path)
                            .expect("Error while dumping pre optimized LLVM IR");
                    }
                    fn postopt_ir(&self, kind: &CompiledKind, module: &InkwellModule) {
                        let mut path = self.debug_dir.clone();
                        path.push(format!("{}.postopt.ll", function_kind_to_filename(kind)));
                        module
                            .print_to_file(&path)
                            .expect("Error while dumping post optimized LLVM IR");
                    }
                    fn obj_memory_buffer(
                        &self,
                        kind: &CompiledKind,
                        memory_buffer: &InkwellMemoryBuffer,
                    ) {
                        let mut path = self.debug_dir.clone();
                        path.push(format!("{}.o", function_kind_to_filename(kind)));
                        let mem_buf_slice = memory_buffer.as_slice();
                        let mut file = File::create(path)
                            .expect("Error while creating debug object file from LLVM IR");
                        let mut pos = 0;
                        while pos < mem_buf_slice.len() {
                            pos += file.write(&mem_buf_slice[pos..]).unwrap();
                        }
                    }
                    fn asm_memory_buffer(
                        &self,
                        kind: &CompiledKind,
                        asm_memory_buffer: &InkwellMemoryBuffer,
                    ) {
                        let mut path = self.debug_dir.clone();
                        path.push(format!("{}.s", function_kind_to_filename(kind)));
                        let mem_buf_slice = asm_memory_buffer.as_slice();
                        let mut file = File::create(path)
                            .expect("Error while creating debug object file from LLVM IR");
                        let mut pos = 0;
                        while pos < mem_buf_slice.len() {
                            pos += file.write(&mem_buf_slice[pos..]).unwrap();
                        }
                    }
                }

                impl fmt::Debug for Callbacks {
                    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                        write!(f, "LLVMCallbacks")
                    }
                }

                if let Some(ref llvm_debug_dir) = runtime_opts.llvm_debug_dir {
                    config.callbacks(Some(Arc::new(Callbacks::new(llvm_debug_dir.clone())?)));
                }
                if runtime_opts.enable_verifier {
                    config.enable_verifier();
                }

                if runtime_opts.enable_pass_params_opt {
                    config.enable_pass_params_opt();
                }

                if let Some(num_threads) = runtime_opts.llvm_num_threads {
                    config.num_threads(num_threads);
                }

                if let Some(p) = &runtime_opts.profiler {
                    match p {
                        Profiler::Perfmap => config.enable_perfmap(),
                    }
                }

                let engine = wasmer_compiler::EngineBuilder::new(config)
                    .set_features(Some(features.clone()))
                    .set_target(Some(target.clone()))
                    .engine()
                    .into();
                Ok(engine)
            }
            #[cfg(feature = "v8")]
            Self::V8 => Ok(wasmer::v8::V8::new().into()),
            #[cfg(feature = "wamr")]
            Self::Wamr => Ok(wasmer::wamr::Wamr::new().into()),
            #[cfg(feature = "wasmi")]
            Self::Wasmi => Ok(wasmer::wasmi::Wasmi::new().into()),
            Self::Headless => bail!("Headless is not a valid runtime to instantiate directly"),
            #[allow(unreachable_patterns)]
            _ => bail!("Unsupported backend type"),
        }
    }

    /// Check if this backend supports all the required WebAssembly features
    #[allow(unreachable_code)]
    pub fn supports_features(&self, required_features: &Features, target: &Target) -> bool {
        // Map BackendType to the corresponding wasmer::BackendKind
        let backend_kind = match self {
            #[cfg(feature = "singlepass")]
            Self::Singlepass => wasmer::BackendKind::Singlepass,
            #[cfg(feature = "cranelift")]
            Self::Cranelift => wasmer::BackendKind::Cranelift,
            #[cfg(feature = "llvm")]
            Self::LLVM => wasmer::BackendKind::LLVM,
            #[cfg(feature = "v8")]
            Self::V8 => wasmer::BackendKind::V8,
            #[cfg(feature = "wamr")]
            Self::Wamr => wasmer::BackendKind::Wamr,
            #[cfg(feature = "wasmi")]
            Self::Wasmi => wasmer::BackendKind::Wasmi,
            Self::Headless => return false, // Headless can't compile
            #[allow(unreachable_patterns)]
            _ => return false,
        };

        // Get the supported features from the backend
        let supported = wasmer::Engine::supported_features_for_backend(&backend_kind, target);

        // Check if the backend supports all required features
        if !supported.contains_features(required_features) {
            return false;
        }

        true
    }
}

impl From<&BackendType> for wasmer::BackendKind {
    fn from(backend_type: &BackendType) -> Self {
        match backend_type {
            #[cfg(feature = "singlepass")]
            BackendType::Singlepass => wasmer::BackendKind::Singlepass,
            #[cfg(feature = "cranelift")]
            BackendType::Cranelift => wasmer::BackendKind::Cranelift,
            #[cfg(feature = "llvm")]
            BackendType::LLVM => wasmer::BackendKind::LLVM,
            #[cfg(feature = "v8")]
            BackendType::V8 => wasmer::BackendKind::V8,
            #[cfg(feature = "wamr")]
            BackendType::Wamr => wasmer::BackendKind::Wamr,
            #[cfg(feature = "wasmi")]
            BackendType::Wasmi => wasmer::BackendKind::Wasmi,
            _ => {
                #[cfg(feature = "sys")]
                {
                    wasmer::BackendKind::Headless
                }
                #[cfg(not(feature = "sys"))]
                {
                    unreachable!("No backend enabled!")
                }
            }
        }
    }
}

impl std::fmt::Display for BackendType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Singlepass => "singlepass",
                Self::Cranelift => "cranelift",
                Self::LLVM => "llvm",
                Self::V8 => "v8",
                Self::Wamr => "wamr",
                Self::Wasmi => "wasmi",
                Self::Headless => "headless",
            }
        )
    }
}
