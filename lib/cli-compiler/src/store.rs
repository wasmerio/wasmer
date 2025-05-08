//! Common module with common used structures across different
//! commands.

use crate::common::WasmFeatures;
use anyhow::Result;
use clap::Parser;
#[cfg(doc)]
use std::path::PathBuf;
use std::string::ToString;
#[allow(unused_imports)]
use std::sync::Arc;
use wasmer_compiler::{CompilerConfig, EngineBuilder, Features};
use wasmer_types::target::{PointerWidth, Target};
#[cfg(doc)]
use wasmer_types::Type;
use wasmer_types::{MemoryStyle, MemoryType, Pages, TableStyle, TableType};

/// Minimul Subset of Tunable parameters for WebAssembly compilation.
#[derive(Clone)]
pub struct SubsetTunables {
    /// For static heaps, the size in wasm pages of the heap protected by bounds checking.
    pub static_memory_bound: Pages,

    /// The size in bytes of the offset guard for static heaps.
    pub static_memory_offset_guard_size: u64,

    /// The size in bytes of the offset guard for dynamic heaps.
    pub dynamic_memory_offset_guard_size: u64,
}

impl SubsetTunables {
    /// Get the `BaseTunables` for a specific Target
    pub fn for_target(target: &Target) -> Self {
        let triple = target.triple();
        let pointer_width: PointerWidth = triple.pointer_width().unwrap();
        let (static_memory_bound, static_memory_offset_guard_size): (Pages, u64) =
            match pointer_width {
                PointerWidth::U16 => (0x400.into(), 0x1000),
                PointerWidth::U32 => (0x4000.into(), 0x1_0000),
                // Static Memory Bound:
                //   Allocating 4 GiB of address space let us avoid the
                //   need for explicit bounds checks.
                // Static Memory Guard size:
                //   Allocating 2 GiB of address space lets us translate wasm
                //   offsets into x86 offsets as aggressively as we can.
                PointerWidth::U64 => (0x1_0000.into(), 0x8000_0000),
            };

        // Allocate a small guard to optimize common cases but without
        // wasting too much memory.
        // The Windows memory manager seems more laxed than the other ones
        // And a guard of just 1 page may not be enough is some borderline cases
        // So using 2 pages for guard on this platform
        #[cfg(target_os = "windows")]
        let dynamic_memory_offset_guard_size: u64 = 0x2_0000;
        #[cfg(not(target_os = "windows"))]
        let dynamic_memory_offset_guard_size: u64 = 0x1_0000;

        Self {
            static_memory_bound,
            static_memory_offset_guard_size,
            dynamic_memory_offset_guard_size,
        }
    }
    /// Get a `MemoryStyle` for the provided `MemoryType`
    pub fn memory_style(&self, memory: &MemoryType) -> MemoryStyle {
        // A heap with a maximum that doesn't exceed the static memory bound specified by the
        // tunables make it static.
        //
        // If the module doesn't declare an explicit maximum treat it as 4GiB.
        let maximum = memory.maximum.unwrap_or_else(Pages::max_value);
        if maximum <= self.static_memory_bound {
            MemoryStyle::Static {
                // Bound can be larger than the maximum for performance reasons
                bound: self.static_memory_bound,
                offset_guard_size: self.static_memory_offset_guard_size,
            }
        } else {
            MemoryStyle::Dynamic {
                offset_guard_size: self.dynamic_memory_offset_guard_size,
            }
        }
    }

    /// Get a [`TableStyle`] for the provided [`TableType`].
    pub fn table_style(&self, _table: &TableType) -> TableStyle {
        TableStyle::CallerChecksSignature
    }
}

#[derive(Debug, Clone, Parser, Default)]
/// The compiler and engine options
pub struct StoreOptions {
    #[clap(flatten)]
    compiler: CompilerOptions,
}

#[derive(Debug, Clone, Parser, Default)]
/// The compiler options
pub struct CompilerOptions {
    /// Use Singlepass compiler.
    #[clap(long, conflicts_with_all = &["cranelift", "llvm"])]
    singlepass: bool,

    /// Use Cranelift compiler.
    #[clap(long, conflicts_with_all = &["singlepass", "llvm"])]
    cranelift: bool,

    /// Use LLVM compiler.
    #[clap(long, conflicts_with_all = &["singlepass", "cranelift"])]
    llvm: bool,

    /// Enable compiler internal verification.
    #[allow(unused)]
    #[clap(long)]
    #[allow(dead_code)]
    enable_verifier: bool,

    /// LLVM debug directory, where IR and object files will be written to.
    #[allow(unused)]
    #[cfg(feature = "llvm")]
    #[cfg_attr(feature = "llvm", clap(long, parse(from_os_str)))]
    llvm_debug_dir: Option<PathBuf>,

    #[clap(flatten)]
    features: WasmFeatures,
}

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
                else if #[cfg(all(feature = "singlepass", any(target_arch = "x86_64", target_arch = "aarch64")))] {
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

    fn get_engine_by_type(
        &self,
        target: Target,
        compiler_config: Box<dyn CompilerConfig>,
    ) -> Result<EngineBuilder> {
        let features = self.get_features(compiler_config.default_features_for_target(&target))?;
        let engine: EngineBuilder = EngineBuilder::new(compiler_config)
            .set_target(Some(target))
            .set_features(Some(features));

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
                    CompiledKind, InkwellMemoryBuffer, InkwellModule, LLVMCallbacks,
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
                Box::new(config)
            }
            #[cfg(not(all(feature = "singlepass", feature = "cranelift", feature = "llvm")))]
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
    #[allow(dead_code)]
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

impl std::fmt::Display for CompilerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Singlepass => "singlepass",
                Self::Cranelift => "cranelift",
                Self::LLVM => "llvm",
                Self::Headless => "headless",
            }
        )
    }
}

impl StoreOptions {
    /// Get a EngineBulder for the Target
    pub fn get_engine_for_target(&self, target: Target) -> Result<(EngineBuilder, CompilerType)> {
        let (compiler_config, compiler_type) = self.compiler.get_compiler_config()?;
        let engine = self.get_engine_with_compiler(target, compiler_config)?;
        Ok((engine, compiler_type))
    }

    fn get_engine_with_compiler(
        &self,
        target: Target,
        compiler_config: Box<dyn CompilerConfig>,
    ) -> Result<EngineBuilder> {
        self.compiler.get_engine_by_type(target, compiler_config)
    }

    /// Get (Subset)Tunables for the Target
    pub fn get_tunables_for_target(&self, target: &Target) -> Result<SubsetTunables> {
        let tunables = SubsetTunables::for_target(target);
        Ok(tunables)
    }
}
