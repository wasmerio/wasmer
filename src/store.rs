//! Common module with common used structures across different
//! commands.

use crate::common::WasmFeatures;
use anyhow::{Error, Result};
use std::path::PathBuf;
use std::str::FromStr;
use std::string::ToString;
use std::sync::Arc;
use structopt::StructOpt;
use wasmer::*;
use wasmer_compiler::CompilerConfig;

#[derive(Debug, Clone, StructOpt)]
/// The compiler options
pub struct StoreOptions {
    /// Use Singlepass compiler
    #[structopt(long, conflicts_with_all = &["cranelift", "llvm", "backend"])]
    singlepass: bool,

    /// Use Cranelift compiler
    #[structopt(long, conflicts_with_all = &["singlepass", "llvm", "backend"])]
    cranelift: bool,

    /// Use LLVM compiler
    #[structopt(long, conflicts_with_all = &["singlepass", "cranelift", "backend"])]
    llvm: bool,

    /// LLVM debug directory, where IR and object files will be written to.
    #[structopt(long, parse(from_os_str))]
    llvm_debug_dir: Option<PathBuf>,

    /// Use JIT Engine
    #[structopt(long, conflicts_with_all = &["native"])]
    jit: bool,

    /// Use Native Engine
    #[structopt(long, conflicts_with_all = &["jit"])]
    native: bool,

    /// The deprecated backend flag - Please not use
    #[structopt(long = "backend", hidden = true, conflicts_with_all = &["singlepass", "cranelift", "llvm"])]
    backend: Option<String>,

    #[structopt(flatten)]
    features: WasmFeatures,
    // #[structopt(flatten)]
    // llvm_options: LLVMCLIOptions,
}

#[derive(Debug)]
enum Compiler {
    Singlepass,
    Cranelift,
    LLVM,
}

impl ToString for Compiler {
    fn to_string(&self) -> String {
        match self {
            Self::Singlepass => "singlepass".to_string(),
            Self::Cranelift => "cranelift".to_string(),
            Self::LLVM => "llvm".to_string(),
        }
    }
}

impl FromStr for Compiler {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "singlepass" => Ok(Self::Singlepass),
            "cranelift" => Ok(Self::Cranelift),
            "llvm" => Ok(Self::LLVM),
            backend => bail!("The `{}` compiler does not exist.", backend),
        }
    }
}

#[cfg(all(feature = "compiler", feature = "engine"))]
impl StoreOptions {
    fn get_compiler(&self) -> Result<Compiler> {
        if self.cranelift {
            Ok(Compiler::Cranelift)
        } else if self.llvm {
            Ok(Compiler::LLVM)
        } else if self.singlepass {
            Ok(Compiler::Singlepass)
        } else if let Some(backend) = self.backend.clone() {
            warning!(
                "the `--backend={0}` flag is deprecated, please use `--{0}` instead",
                backend
            );
            Compiler::from_str(&backend)
        } else {
            // Auto mode, we choose the best compiler for that platform
            cfg_if::cfg_if! {
                if #[cfg(all(feature = "cranelift", target_arch = "x86_64"))] {
                    return Ok(Compiler::Cranelift);
                }
                else if #[cfg(all(feature = "singlepass", target_arch = "x86_64"))] {
                    return Ok(Compiler::Singlepass);
                }
                else if #[cfg(feature = "llvm")] {
                    return Ok(Compiler::LLVM);
                } else {
                    bail!("There are no available compilers for your architecture");
                }
            }
        }
    }

    /// Get the Target architecture
    pub fn get_features(&self) -> Result<Features> {
        let mut features = Features::default();
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

    /// Get the Target architecture
    pub fn get_target(&self) -> Result<Target> {
        Ok(Target::default())
    }

    /// Get the Compiler Config for the current options
    #[allow(unused_variables)]
    fn get_config(&self, compiler: Compiler) -> Result<Box<dyn CompilerConfig>> {
        let features = self.get_features()?;
        let target = self.get_target()?;
        let config: Box<dyn CompilerConfig> = match compiler {
            #[cfg(feature = "singlepass")]
            Compiler::Singlepass => {
                let config = wasmer_compiler_singlepass::SinglepassConfig::new(features, target);
                Box::new(config)
            }
            #[cfg(feature = "cranelift")]
            Compiler::Cranelift => {
                let config = wasmer_compiler_cranelift::CraneliftConfig::new(features, target);
                Box::new(config)
            }
            #[cfg(feature = "llvm")]
            Compiler::LLVM => {
                use std::fs::File;
                use std::io::Write;
                use wasm_common::entity::EntityRef;
                use wasmer_compiler_llvm::{
                    CompiledFunctionKind, InkwellMemoryBuffer, InkwellModule, LLVMCallbacks,
                    LLVMConfig,
                };
                let mut config = LLVMConfig::new(features, target);
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
                    types.iter().map(|ty| {
                        match ty {
                            Type::I32 => "i".to_string(),
                            Type::I64 => "I".to_string(),
                            Type::F32 => "f".to_string(),
                            Type::F64 => "F".to_string(),
                            _ => {
                                unimplemented!("Function type not yet supported for generated signatures in debugging");
                            }
                        }
                    }).collect::<Vec<_>>().join("")
                }
                // Converts a kind into a filename, that we will use to dump
                // the contents of the IR object file to.
                fn function_kind_to_filename(kind: &CompiledFunctionKind) -> String {
                    match kind {
                        CompiledFunctionKind::Local(local_index) => {
                            format!("function_{}", local_index.index())
                        }
                        CompiledFunctionKind::FunctionCallTrampoline(func_type) => format!(
                            "trampoline_call_{}_{}",
                            types_to_signature(&func_type.params()),
                            types_to_signature(&func_type.results())
                        ),
                        CompiledFunctionKind::DynamicFunctionTrampoline(func_type) => format!(
                            "trampoline_dynamic_{}_{}",
                            types_to_signature(&func_type.params()),
                            types_to_signature(&func_type.results())
                        ),
                    }
                }
                impl LLVMCallbacks for Callbacks {
                    fn preopt_ir(&self, kind: &CompiledFunctionKind, module: &InkwellModule) {
                        let mut path = self.debug_dir.clone();
                        path.push(format!("{}.preopt.ir", function_kind_to_filename(kind)));
                        module
                            .print_to_file(&path)
                            .expect("Error while dumping pre optimized LLVM IR");
                    }
                    fn postopt_ir(&self, kind: &CompiledFunctionKind, module: &InkwellModule) {
                        let mut path = self.debug_dir.clone();
                        path.push(format!("{}.postopt.ir", function_kind_to_filename(kind)));
                        module
                            .print_to_file(&path)
                            .expect("Error while dumping post optimized LLVM IR");
                    }
                    fn obj_memory_buffer(
                        &self,
                        kind: &CompiledFunctionKind,
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
                if let Some(ref llvm_debug_dir) = self.llvm_debug_dir {
                    config.callbacks = Some(Arc::new(Callbacks::new(llvm_debug_dir.clone())?));
                }
                Box::new(config)
            }
            #[cfg(not(all(feature = "singlepass", feature = "cranelift", feature = "llvm",)))]
            compiler => bail!(
                "The `{}` compiler is not included in this binary.",
                compiler.to_string()
            ),
        };
        Ok(config)
    }

    /// Gets the compiler config
    fn get_compiler_config(&self) -> Result<(Box<dyn CompilerConfig>, String)> {
        let compiler = self.get_compiler()?;
        let compiler_name = compiler.to_string();
        let compiler_config = self.get_config(compiler)?;
        Ok((compiler_config, compiler_name))
    }

    /// Gets the tunables for the compiler target
    pub fn get_tunables(&self, compiler_config: &dyn CompilerConfig) -> Tunables {
        Tunables::for_target(compiler_config.target().triple())
    }

    /// Gets the store, with the engine name and compiler name selected
    pub fn get_store(&self) -> Result<(Store, String, String)> {
        let (compiler_config, compiler_name) = self.get_compiler_config()?;
        let tunables = self.get_tunables(&*compiler_config);
        let (engine, engine_name) = self.get_engine_with_compiler(tunables, compiler_config)?;
        let store = Store::new(engine);
        Ok((store, engine_name, compiler_name))
    }

    fn get_engine_with_compiler(
        &self,
        tunables: Tunables,
        compiler_config: Box<dyn CompilerConfig>,
    ) -> Result<(Arc<dyn Engine + Send + Sync>, String)> {
        let engine_type = self.get_engine()?;
        let engine: Arc<dyn Engine + Send + Sync> = match engine_type {
            #[cfg(feature = "jit")]
            EngineOptions::JIT => {
                Arc::new(wasmer_engine_jit::JITEngine::new(compiler_config, tunables))
            }
            #[cfg(feature = "native")]
            EngineOptions::Native => Arc::new(wasmer_engine_native::NativeEngine::new(
                compiler_config,
                tunables,
            )),
            #[cfg(not(all(feature = "jit", feature = "native",)))]
            engine => bail!(
                "The `{}` engine is not included in this binary.",
                engine.to_string()
            ),
        };
        return Ok((engine, engine_type.to_string()));
    }
}

enum EngineOptions {
    JIT,
    Native,
}

impl ToString for EngineOptions {
    fn to_string(&self) -> String {
        match self {
            Self::JIT => "jit".to_string(),
            Self::Native => "native".to_string(),
        }
    }
}

#[cfg(feature = "engine")]
impl StoreOptions {
    fn get_engine(&self) -> Result<EngineOptions> {
        if self.jit {
            Ok(EngineOptions::JIT)
        } else if self.native {
            Ok(EngineOptions::Native)
        } else {
            // Auto mode, we choose the best engine for that platform
            if cfg!(feature = "jit") {
                Ok(EngineOptions::JIT)
            } else if cfg!(feature = "native") {
                Ok(EngineOptions::Native)
            } else {
                bail!("There are no available engines for your architecture")
            }
        }
    }
}

// If we don't have a compiler, but we have an engine
#[cfg(all(not(feature = "compiler"), feature = "engine"))]
impl StoreOptions {
    fn get_engine_headless(
        &self,
        tunables: Tunables,
    ) -> Result<(Arc<dyn Engine + Send + Sync>, String)> {
        let engine_type = self.get_engine()?;
        let engine: Arc<dyn Engine + Send + Sync> = match engine_type {
            #[cfg(feature = "jit")]
            EngineOptions::JIT => Arc::new(wasmer_engine_jit::JITEngine::headless(tunables)),
            #[cfg(feature = "native")]
            EngineOptions::Native => {
                Arc::new(wasmer_engine_native::NativeEngine::headless(tunables))
            }
            #[cfg(not(all(feature = "jit", feature = "native",)))]
            engine => bail!(
                "The `{}` engine is not included in this binary.",
                engine.to_string()
            ),
        };
        return Ok((engine, engine_type.to_string()));
    }

    /// Get the Target architecture
    pub fn get_target(&self) -> Result<Target> {
        Ok(Target::default())
    }

    /// Get the store (headless engine)
    pub fn get_store(&self) -> Result<(Store, String, String)> {
        // Get the tunables for the current host
        let tunables = Tunables::default();
        let (engine, engine_name) = self.get_engine_headless(tunables)?;
        let store = Store::new(engine);
        Ok((store, engine_name, "headless".to_string()))
    }
}

// If we don't have any engine enabled
#[cfg(not(feature = "engine"))]
impl StoreOptions {
    /// Get the Target architecture
    pub fn get_target(&self) -> Result<Target> {
        bail!("No engines are enabled");
    }

    /// Get the store (headless engine)
    pub fn get_store(&self) -> Result<(Store, String, String)> {
        bail!("No engines are enabled");
    }
}
