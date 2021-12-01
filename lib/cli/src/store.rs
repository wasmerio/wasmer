//! Common module with common used structures across different
//! commands.

use crate::common::WasmFeatures;
use anyhow::Result;
use std::path::PathBuf;
use std::string::ToString;
#[allow(unused_imports)]
use std::sync::Arc;
use structopt::StructOpt;
use wasmer::*;
#[cfg(feature = "compiler")]
use wasmer_compiler::CompilerConfig;

#[derive(Debug, Clone, StructOpt, Default)]
/// The compiler and engine options
pub struct StoreOptions {
    #[cfg(feature = "compiler")]
    #[structopt(flatten)]
    compiler: CompilerOptions,

    /// Use the Universal Engine.
    #[structopt(long, conflicts_with_all = &["dylib", "staticlib", "jit", "native", "object_file"])]
    universal: bool,

    /// Use the Dylib Engine.
    #[structopt(long, conflicts_with_all = &["universal", "staticlib", "jit", "native", "object_file"])]
    dylib: bool,

    /// Use the Staticlib Engine.
    #[structopt(long, conflicts_with_all = &["universal", "dylib", "jit", "native", "object_file"])]
    staticlib: bool,

    /// Use the JIT (Universal) Engine.
    #[structopt(long, hidden = true, conflicts_with_all = &["universal", "dylib", "staticlib", "native", "object_file"])]
    jit: bool,

    /// Use the Native (Dylib) Engine.
    #[structopt(long, hidden = true, conflicts_with_all = &["universal", "dylib", "staticlib", "jit", "object_file"])]
    native: bool,

    /// Use the ObjectFile (Staticlib) Engine.
    #[structopt(long, hidden = true, conflicts_with_all = &["universal", "dylib", "staticlib", "jit", "native"])]
    object_file: bool,
}

#[cfg(feature = "compiler")]
#[derive(Debug, Clone, StructOpt, Default)]
/// The compiler options
pub struct CompilerOptions {
    /// Use Singlepass compiler.
    #[structopt(long, conflicts_with_all = &["cranelift", "llvm"])]
    singlepass: bool,

    /// Use Cranelift compiler.
    #[structopt(long, conflicts_with_all = &["singlepass", "llvm"])]
    cranelift: bool,

    /// Use LLVM compiler.
    #[structopt(long, conflicts_with_all = &["singlepass", "cranelift"])]
    llvm: bool,

    /// Enable compiler internal verification.
    #[structopt(long)]
    enable_verifier: bool,

    /// LLVM debug directory, where IR and object files will be written to.
    #[cfg(feature = "llvm")]
    #[structopt(long, parse(from_os_str))]
    llvm_debug_dir: Option<PathBuf>,

    #[structopt(flatten)]
    features: WasmFeatures,
}

#[cfg(feature = "compiler")]
impl CompilerOptions {
    fn get_compiler(&self) -> Result<CompilerType> {
        if self.cranelift {
            Ok(CompilerType::Cranelift)
        } else if self.llvm {
            Ok(CompilerType::LLVM)
        } else if self.singlepass {
            Ok(CompilerType::Singlepass)
        } else {
            // Auto mode, we choose the best compiler for that platform
            cfg_if::cfg_if! {
                if #[cfg(all(feature = "cranelift", any(target_arch = "x86_64", target_arch = "aarch64")))] {
                    Ok(CompilerType::Cranelift)
                }
                else if #[cfg(all(feature = "singlepass", target_arch = "x86_64"))] {
                    Ok(CompilerType::Singlepass)
                }
                else if #[cfg(feature = "llvm")] {
                    Ok(CompilerType::LLVM)
                } else {
                    bail!("There are no available compilers for your architecture");
                }
            }
        }
    }

    /// Get the enaled Wasm features.
    pub fn get_features(&self, mut features: Features) -> Result<Features> {
        if self.features.threads || self.features.all {
            features.threads(true);
        }
        if self.features.multi_value || self.features.all {
            features.multi_value(true);
        }
        if self.features.simd || self.features.all {
            features.simd(true);
        }
        if self.features.bulk_memory || self.features.all {
            features.bulk_memory(true);
        }
        if self.features.reference_types || self.features.all {
            features.reference_types(true);
        }
        Ok(features)
    }

    /// Gets the Store for a given target and engine.
    pub fn get_store_for_target_and_engine(
        &self,
        target: Target,
        engine_type: EngineType,
    ) -> Result<(Store, CompilerType)> {
        let (compiler_config, compiler_type) = self.get_compiler_config()?;
        let engine = self.get_engine_by_type(target, compiler_config, engine_type)?;
        let store = Store::new(&*engine);
        Ok((store, compiler_type))
    }

    fn get_engine_by_type(
        &self,
        target: Target,
        compiler_config: Box<dyn CompilerConfig>,
        engine_type: EngineType,
    ) -> Result<Box<dyn Engine + Send + Sync>> {
        let features = self.get_features(compiler_config.default_features_for_target(&target))?;
        let engine: Box<dyn Engine + Send + Sync> = match engine_type {
            #[cfg(feature = "universal")]
            EngineType::Universal => Box::new(
                wasmer_engine_universal::Universal::new(compiler_config)
                    .features(features)
                    .target(target)
                    .engine(),
            ),
            #[cfg(feature = "dylib")]
            EngineType::Dylib => Box::new(
                wasmer_engine_dylib::Dylib::new(compiler_config)
                    .target(target)
                    .features(features)
                    .engine(),
            ),
            #[cfg(feature = "staticlib")]
            EngineType::Staticlib => Box::new(
                wasmer_engine_staticlib::Staticlib::new(compiler_config)
                    .target(target)
                    .features(features)
                    .engine(),
            ),
            #[cfg(not(all(feature = "universal", feature = "dylib", feature = "staticlib")))]
            engine => bail!(
                "The `{}` engine is not included in this binary.",
                engine.to_string()
            ),
        };

        Ok(engine)
    }

    /// Get the Compiler Config for the current options
    #[allow(unused_variables)]
    pub(crate) fn get_compiler_config(&self) -> Result<(Box<dyn CompilerConfig>, CompilerType)> {
        let compiler = self.get_compiler()?;
        let compiler_config: Box<dyn CompilerConfig> = match compiler {
            CompilerType::Headless => bail!("The headless engine can't be chosen"),
            #[cfg(feature = "singlepass")]
            CompilerType::Singlepass => {
                let mut config = wasmer_compiler_singlepass::Singlepass::new();
                if self.enable_verifier {
                    config.enable_verifier();
                }
                Box::new(config)
            }
            #[cfg(feature = "cranelift")]
            CompilerType::Cranelift => {
                let mut config = wasmer_compiler_cranelift::Cranelift::new();
                if self.enable_verifier {
                    config.enable_verifier();
                }
                Box::new(config)
            }
            #[cfg(feature = "llvm")]
            CompilerType::LLVM => {
                use std::fmt;
                use std::fs::File;
                use std::io::Write;
                use wasmer_compiler_llvm::{
                    CompiledKind, InkwellMemoryBuffer, InkwellModule, LLVMCallbacks, LLVM,
                };
                use wasmer_types::entity::EntityRef;
                let mut config = LLVM::new();
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
                            types_to_signature(&func_type.params()),
                            types_to_signature(&func_type.results())
                        ),
                        CompiledKind::DynamicFunctionTrampoline(func_type) => format!(
                            "trampoline_dynamic_{}_{}",
                            types_to_signature(&func_type.params()),
                            types_to_signature(&func_type.results())
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
                Box::new(config)
            }
            #[cfg(not(all(feature = "singlepass", feature = "cranelift", feature = "llvm",)))]
            compiler => {
                bail!(
                    "The `{}` compiler is not included in this binary.",
                    compiler.to_string()
                )
            }
        };

        #[allow(unreachable_code)]
        Ok((compiler_config, compiler))
    }
}

/// The compiler used for the store
#[derive(Debug, PartialEq, Eq)]
pub enum CompilerType {
    /// Singlepass compiler
    Singlepass,
    /// Cranelift compiler
    Cranelift,
    /// LLVM compiler
    LLVM,
    /// Headless compiler
    Headless,
}

impl CompilerType {
    /// Return all enabled compilers
    pub fn enabled() -> Vec<CompilerType> {
        vec![
            #[cfg(feature = "singlepass")]
            Self::Singlepass,
            #[cfg(feature = "cranelift")]
            Self::Cranelift,
            #[cfg(feature = "llvm")]
            Self::LLVM,
        ]
    }
}

impl ToString for CompilerType {
    fn to_string(&self) -> String {
        match self {
            Self::Singlepass => "singlepass".to_string(),
            Self::Cranelift => "cranelift".to_string(),
            Self::LLVM => "llvm".to_string(),
            Self::Headless => "headless".to_string(),
        }
    }
}

/// The engine used for the store
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum EngineType {
    /// Universal Engine
    Universal,
    /// Dylib Engine
    Dylib,
    /// Static Engine
    Staticlib,
}

impl ToString for EngineType {
    fn to_string(&self) -> String {
        match self {
            Self::Universal => "universal".to_string(),
            Self::Dylib => "dylib".to_string(),
            Self::Staticlib => "staticlib".to_string(),
        }
    }
}

#[cfg(all(feature = "compiler", feature = "engine"))]
impl StoreOptions {
    /// Gets the store for the host target, with the engine name and compiler name selected
    pub fn get_store(&self) -> Result<(Store, EngineType, CompilerType)> {
        let target = Target::default();
        self.get_store_for_target(target)
    }

    /// Gets the store for a given target, with the engine name and compiler name selected, as
    pub fn get_store_for_target(
        &self,
        target: Target,
    ) -> Result<(Store, EngineType, CompilerType)> {
        let (compiler_config, compiler_type) = self.compiler.get_compiler_config()?;
        let (engine, engine_type) = self.get_engine_with_compiler(target, compiler_config)?;
        let store = Store::new(&*engine);
        Ok((store, engine_type, compiler_type))
    }

    fn get_engine_with_compiler(
        &self,
        target: Target,
        compiler_config: Box<dyn CompilerConfig>,
    ) -> Result<(Box<dyn Engine + Send + Sync>, EngineType)> {
        let engine_type = self.get_engine()?;
        let engine = self
            .compiler
            .get_engine_by_type(target, compiler_config, engine_type)?;

        Ok((engine, engine_type))
    }
}

#[cfg(feature = "engine")]
impl StoreOptions {
    fn get_engine(&self) -> Result<EngineType> {
        if self.universal || self.jit {
            Ok(EngineType::Universal)
        } else if self.dylib || self.native {
            Ok(EngineType::Dylib)
        } else if self.staticlib || self.object_file {
            Ok(EngineType::Staticlib)
        } else {
            // Auto mode, we choose the best engine for that platform
            if cfg!(feature = "universal") {
                Ok(EngineType::Universal)
            } else if cfg!(feature = "dylib") {
                Ok(EngineType::Dylib)
            } else if cfg!(feature = "staticlib") {
                Ok(EngineType::Staticlib)
            } else {
                bail!("There are no available engines for your architecture")
            }
        }
    }
}

// If we don't have a compiler, but we have an engine
#[cfg(all(not(feature = "compiler"), feature = "engine"))]
impl StoreOptions {
    fn get_engine_headless(&self) -> Result<(Arc<dyn Engine + Send + Sync>, EngineType)> {
        let engine_type = self.get_engine()?;
        let engine: Arc<dyn Engine + Send + Sync> = match engine_type {
            #[cfg(feature = "universal")]
            EngineType::Universal => {
                Arc::new(wasmer_engine_universal::Universal::headless().engine())
            }
            #[cfg(feature = "dylib")]
            EngineType::Dylib => Arc::new(wasmer_engine_dylib::Dylib::headless().engine()),
            #[cfg(feature = "staticlib")]
            EngineType::Staticlib => {
                Arc::new(wasmer_engine_staticlib::Staticlib::headless().engine())
            }
            #[cfg(not(all(feature = "universal", feature = "dylib", feature = "staticlib")))]
            engine => bail!(
                "The `{}` engine is not included in this binary.",
                engine.to_string()
            ),
        };
        Ok((engine, engine_type))
    }

    /// Get the store (headless engine)
    pub fn get_store(&self) -> Result<(Store, EngineType, CompilerType)> {
        let (engine, engine_type) = self.get_engine_headless()?;
        let store = Store::new(&*engine);
        Ok((store, engine_type, CompilerType::Headless))
    }

    /// Gets the store for provided host target
    pub fn get_store_for_target(
        &self,
        _target: Target,
    ) -> Result<(Store, EngineType, CompilerType)> {
        bail!("You need compilers to retrieve a store for a specific target");
    }
}

// If we don't have any engine enabled
#[cfg(not(feature = "engine"))]
impl StoreOptions {
    /// Get the store (headless engine)
    pub fn get_store(&self) -> Result<(Store, EngineType, CompilerType)> {
        bail!("No engines are enabled");
    }

    /// Gets the store for the host target
    pub fn get_store_for_target(
        &self,
        _target: Target,
    ) -> Result<(Store, EngineType, CompilerType)> {
        bail!("No engines are enabled");
    }
}
