//! Common module with common used structures across different
//! commands.

use crate::common::WasmFeatures;
use anyhow::{bail, Error, Result};
use std::str::FromStr;
use std::string::ToString;
use std::sync::Arc;
use structopt::StructOpt;
use wasmer::*;
#[cfg(feature = "engine-jit")]
use wasmer_engine_jit::JITEngine;

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

#[cfg(feature = "compiler")]
impl StoreOptions {
    fn get_compiler(&self) -> Result<Compiler> {
        if self.cranelift {
            return Ok(Compiler::Cranelift);
        } else if self.llvm {
            return Ok(Compiler::LLVM);
        } else if self.singlepass {
            return Ok(Compiler::Singlepass);
        } else if let Some(backend) = self.backend.clone() {
            eprintln!(
                "warning: the `--backend={0}` flag is deprecated, please use `--{0}` instead",
                backend
            );
            return Compiler::from_str(&backend);
        } else {
            // Auto mode, we choose the best compiler for that platform
            if cfg!(feature = "compiler-cranelift") && cfg!(target_arch = "x86_64") {
                return Ok(Compiler::Cranelift);
            } else if cfg!(feature = "compiler-singlepass") && cfg!(target_arch = "x86_64") {
                return Ok(Compiler::Singlepass);
            } else if cfg!(feature = "compiler-llvm") {
                return Ok(Compiler::LLVM);
            } else {
                bail!("There are no available compilers for your architecture")
            }
        }
    }

    /// Get the Compiler Config for the current options
    #[allow(unused_variables)]
    fn get_config(&self, compiler: Compiler) -> Result<Box<dyn CompilerConfig>> {
        let config: Box<dyn CompilerConfig> = match compiler {
            #[cfg(feature = "compiler-singlepass")]
            Compiler::Singlepass => {
                let config = SinglepassConfig::default();
                Box::new(config)
            }
            #[cfg(feature = "compiler-cranelift")]
            Compiler::Cranelift => {
                let config = CraneliftConfig::default();
                Box::new(config)
            }
            #[cfg(feature = "compiler-llvm")]
            Compiler::LLVM => {
                let config = LLVMConfig::default();
                Box::new(config)
            }
            #[cfg(not(all(
                feature = "compiler-singlepass",
                feature = "compiler-cranelift",
                feature = "compiler-llvm",
            )))]
            compiler => bail!(
                "The `{}` compiler is not included in this binary.",
                compiler.to_string()
            ),
        };
        return Ok(config);
    }

    /// Get's the compiler config
    fn get_compiler_config(&self) -> Result<(Box<dyn CompilerConfig>, String)> {
        let compiler = self.get_compiler()?;
        let compiler_name = compiler.to_string();
        let compiler_config = self.get_config(compiler)?;
        Ok((compiler_config, compiler_name))
    }

    /// Get's the tunables for the compiler target
    pub fn get_tunables(&self, compiler_config: &dyn CompilerConfig) -> Tunables {
        Tunables::for_target(compiler_config.target().triple())
    }

    /// Get's the store
    pub fn get_store(&self) -> Result<(Store, String)> {
        let (compiler_config, compiler_name) = self.get_compiler_config()?;
        let tunables = self.get_tunables(&*compiler_config);
        #[cfg(feature = "engine-jit")]
        let engine = JITEngine::new(&*compiler_config, tunables);
        let store = Store::new(Arc::new(engine));
        Ok((store, compiler_name))
    }
}

#[cfg(not(feature = "compiler"))]
impl StoreOptions {
    /// Get the store (headless engine)
    pub fn get_store(&self) -> Result<(Store, String)> {
        // Get the tunables for the current host
        let tunables = Tunables::default();
        let engine = Engine::headless(tunables);
        let store = Store::new(&engine);
        Ok((store, "headless".to_string()))
    }
}
